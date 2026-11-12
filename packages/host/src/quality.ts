import path from 'node:path'
import { parseQualityResult } from '@dsh-uml-viewer/contracts'
import type { QualityOperation, QualityToolResult } from '@dsh-uml-viewer/contracts'
import { binaryArgv } from './binary.js'
import type { Config } from './binary.js'
import type { HostServices, ToolRunContext } from './dsh.js'
import { confinementError, prepareConfinedCommand, spawnCommand } from './process.js'

const ATTEMPTED_TOOL: Record<QualityOperation, string> = {
  crap: 'crapper', mutate: 'mutator', dry: 'dryer',
}

/** A failed invocation still has the ADR 0001 shape; only a host refusal has no exit. */
export function qualityResult(
  operation: QualityOperation,
  projectRoot: string,
  patch: Partial<QualityToolResult> = {},
): QualityToolResult {
  return {
    version: 1, operation, projectRoot, exitCode: null, validResult: false,
    crapScope: null, configured: false, tool: ATTEMPTED_TOOL[operation], reason: null, ...patch,
  }
}

/**
 * How much of a failed tool's stderr `reason` carries (#39): the last
 * `STDERR_TAIL_LINES` non-blank lines, at most `STDERR_TAIL_CHARS` characters.
 * A tool can print megabytes before it exits; the record is a tool result the
 * model reads, so the bound is part of the contract, not an optimisation.
 */
const STDERR_TAIL_LINES = 10
const STDERR_TAIL_CHARS = 2000

/**
 * The bounded stderr tail a failed tool contributes to `reason` (#39).
 *
 * A refusal is the dispatcher's own sentence and is not this; only a tool
 * that really ran and failed has stderr worth quoting. Blank lines carry no
 * diagnosis and are dropped, so a tool that wrote only newlines contributes
 * nothing and its record keeps `reason: null`. When lines are dropped, the
 * text says how many, so a truncated tail is never mistaken for the whole
 * report.
 *
 * @param stderr what the child wrote to stderr.
 * @returns the tail, or null when stderr held no non-blank line.
 */
function stderrTail(stderr: string): string | null {
  const lines = stderr.split(/\r?\n/).map(line => line.trimEnd()).filter(line => line.trim() !== '')
  if (lines.length === 0) return null
  let shown = lines.slice(-STDERR_TAIL_LINES)
  let text = shown.join('\n')
  if (text.length > STDERR_TAIL_CHARS) {
    // Cut at a line boundary when the tail is longer than the character bound.
    const cut = text.slice(text.length - STDERR_TAIL_CHARS)
    const boundary = cut.indexOf('\n')
    text = boundary === -1 ? cut : cut.slice(boundary + 1)
    shown = text.split('\n')
  }
  const omitted = lines.length - shown.length
  return omitted > 0 ? `last ${shown.length} of ${lines.length} stderr lines:\n${text}` : text
}

/** Compare the binary's canonical project root with ours without re-spawning. */
function sameRoot(left: string, right: string): boolean {
  const normalize = (value: string) => {
    const resolved = path.resolve(value).replace(/[\\/]+$/, '')
    return process.platform === 'win32' ? resolved.toLowerCase() : resolved
  }
  return normalize(left) === normalize(right)
}

/**
 * Run `uml-viewer quality <operation>` in the session cwd. The binary's stdout
 * is exactly one version-1 record; it is the return value, never a file read.
 * Signals, missing readers, malformed records and a record for another
 * operation/root/exit fail closed and therefore cannot grant `uml_ir`.
 */
export async function runQuality(
  services: HostServices,
  config: Config,
  exec: ToolRunContext,
  operation: QualityOperation,
  args: string[],
): Promise<QualityToolResult> {
  const agent = exec.agent
  if (!agent?.session) return qualityResult(operation, '', { reason: `uml_${operation} requires a live DSH session` })
  let root = services.workingDirectory.get(agent.session)
  try {
    const prepared = await prepareConfinedCommand(
      services, agent, exec.signal,
      () => [...binaryArgv(config), 'quality', operation, ...args],
      (refusal, current) => {
        if (refusal === 'read-only') {
          return `read-only sandbox: uml_${operation} writes quality snapshots. Switch the session to workspace-write to run it; no elevation was attempted.`
        }
        if (refusal === 'invalid-policy') return 'Invalid authoritative sandbox policy; no binary was started'
        return `Sandbox policy changed during preparation (${current?.mode}); run uml_${operation} again under the current mode`
      },
    )
    root = prepared.root
    if (prepared.command === undefined) return qualityResult(operation, root, { reason: prepared.reason })
    const command = prepared.command
    const output = await spawnCommand(services, command.argv, root, exec.signal)
    if ('reason' in output) return qualityResult(operation, root, { reason: output.reason })
    const { signal: terminationSignal, outputError } = output
    const denied = command.confined && confinementError(command.confined, output.exitCode, output.stderr)
    if (denied) {
      return qualityResult(operation, root, {
        exitCode: output.exitCode, reason: denied.reason,
      })
    }
    // A missing or throwing reader erases the record, never the real exit.
    if (outputError) {
      return qualityResult(operation, root, {
        exitCode: output.exitCode,
        reason: `quality ${operation} ${outputError}${terminationSignal ? `; terminated by ${terminationSignal}` : ''}`,
      })
    }
    // A terminated child is never a valid result, whatever it printed first;
    // report the termination instead of a parse or exit mismatch it caused.
    if (terminationSignal) {
      return qualityResult(operation, root, {
        exitCode: output.exitCode, reason: `quality ${operation} terminated by ${terminationSignal}`,
      })
    }

    let parsed
    try {
      parsed = parseQualityResult(output.stdout.trim())
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error)
      return qualityResult(operation, root, {
        exitCode: output.exitCode,
        reason: `uml-viewer quality ${operation} printed no valid version-1 record (${message})`,
      })
    }
    if (parsed.operation !== operation) {
      return qualityResult(operation, root, {
        exitCode: output.exitCode,
        reason: `uml-viewer quality ${operation} returned a record for ${parsed.operation}`,
      })
    }
    if (!sameRoot(parsed.projectRoot, root)) {
      return qualityResult(operation, root, {
        exitCode: output.exitCode,
        reason: `uml-viewer quality ${operation} returned projectRoot ${parsed.projectRoot}, not ${root}`,
      })
    }
    if (output.exitCode === null || parsed.exitCode !== output.exitCode) {
      return qualityResult(operation, root, {
        exitCode: output.exitCode,
        reason: `uml-viewer quality ${operation} reported exit ${parsed.exitCode} but exited ${output.exitCode ?? 'without a code'}`,
      })
    }
    if (exec.signal.aborted) {
      return qualityResult(operation, root, {
        ...parsed, exitCode: output.exitCode, validResult: false,
        reason: `quality ${operation} was interrupted`,
      })
    }
    // #39: a tool that really ran and failed explains itself only on stderr,
    // and the record is all the agent sees. Fill the existing `reason` field
    // from the bounded tail so diagnosing never needs a second, direct run.
    // A dispatcher refusal already carries its own sentence and keeps it, and
    // the wording here stays distinct from that refusal path.
    const tail = parsed.reason === null && !parsed.validResult ? stderrTail(output.stderr) : null
    if (tail !== null) {
      return qualityResult(operation, root, {
        ...parsed, reason: `uml-viewer quality ${operation} failed (exit ${output.exitCode})\n${tail}`,
      })
    }
    return parsed
  } catch (error) {
    return qualityResult(operation, root, {
      reason: error instanceof Error ? error.message : String(error),
    })
  }
}
