import { realpath } from 'node:fs/promises'
import path from 'node:path'
import { parseQualityResult } from '@dsh-uml-viewer/contracts'
import type { QualityOperation, QualityToolResult } from '@dsh-uml-viewer/contracts'
import { binaryArgv } from './binary.js'
import type { Config } from './binary.js'
import type { HostServices, ToolRunContext } from './dsh.js'
import { confinedCommand, confinementError, samePolicy, spawnCommand, validPolicy } from './process.js'

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
    const policy = services.sandboxPolicy.resolve({ session: agent.session })
    if (policy.mode === 'read-only') {
      return qualityResult(operation, root, {
        reason: `read-only sandbox: uml_${operation} writes quality snapshots. Switch the session to workspace-write to run it; no elevation was attempted.`,
      })
    }
    if (!validPolicy(policy)) {
      return qualityResult(operation, root, { reason: 'Invalid authoritative sandbox policy; no binary was started' })
    }
    root = await realpath(await services.workingDirectory.ensure(agent, exec.signal))
    const command = await confinedCommand(services, [...binaryArgv(config), 'quality', operation, ...args], policy, exec.signal)
    if (command.error) return qualityResult(operation, root, { reason: command.error.reason })
    // Confinement and directory validation are asynchronous; never reuse a
    // policy that changed while either was in progress.
    const current = services.sandboxPolicy.resolve({ session: agent.session })
    if (!validPolicy(current) || !samePolicy(policy, current)) {
      return qualityResult(operation, root, {
        reason: `Sandbox policy changed during preparation (${current?.mode}); run uml_${operation} again under the current mode`,
      })
    }
    exec.signal.throwIfAborted()
    const output = await spawnCommand(services, command.argv!, root, exec.signal)
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
    return parsed
  } catch (error) {
    return qualityResult(operation, root, {
      reason: error instanceof Error ? error.message : String(error),
    })
  }
}
