import { realpath } from 'node:fs/promises'
import path from 'node:path'
import type { Agent, HostServices, Policy, ConfinedArgv } from './dsh.js'

/** DSH authority checked once per invocation; the caller re-resolves after I/O. */
export type CommandError = { status: 'refused' | 'error'; reason: string }
export type CommandOutcome = CommandError | {
  exitCode: number | null
  signal: string | null
  stdout: string
  stderr: string
  truncated: boolean
  outputError: string | null
}

export async function confinedCommand(services: HostServices, argv: string[], policy: Policy, signal: AbortSignal): Promise<{ argv?: string[]; confined?: ConfinedArgv; error?: CommandError }> {
  if (policy.mode === 'danger-full-access') return { argv }
  if (!services.sandbox) return { error: { status: 'refused', reason: 'No DSH confinement provider; refusing unconfined execution' } }
  const confined = await services.sandbox.confine(argv, policy, signal)
  if (!['full', 'partial'].includes(confined?.enforcement) || !confined.argv?.length) {
    return { error: { status: 'refused', reason: 'Invalid DSH confinement result; refusing unconfined execution' } }
  }
  return { argv: confined.argv, confined }
}

/** Why the shared authority sequence refused, so the caller can word it. */
export type PrepareRefusal = 'read-only' | 'invalid-policy' | 'changed-policy'

/** A confined command line and the canonical root it must run in. */
export interface PreparedCommand {
  argv: string[]
  /** Where enforcement is not full access, the provider's own record. */
  confined?: ConfinedArgv
}

/** Either the command to spawn, or the reason this invocation was refused. */
export type CommandPreparation =
  | { root: string; command: PreparedCommand; reason?: undefined }
  | { root: string; command?: undefined; reason: string }

/**
 * The one authority sequence every child spawn shares (#review M4): resolve the
 * session's policy, refuse read-only, reject an invalid policy, ensure and
 * canonicalize the session cwd, confine the argv, resolve the policy again
 * after that asynchronous work and require the SAME authority, then honour a
 * cancelled signal. It was copied into `runner.ts`, `quality.ts` and
 * `resident.ts`; a fix to one copy would have missed the others, which is why
 * the sequence lives here once. A caller supplies only its argv — which may
 * depend on the canonical root — and the wording of its own refusals.
 *
 * @param services the host services this invocation uses.
 * @param agent the session whose sandbox authority applies.
 * @param signal the invocation's cancellation signal.
 * @param argvFor the argv to confine, given the canonical root.
 * @param reasonFor the caller's wording for each authority refusal.
 * @returns the canonical root, and either the prepared command or the reason.
 */
export async function prepareConfinedCommand(
  services: HostServices,
  agent: Agent,
  signal: AbortSignal,
  argvFor: (root: string) => string[] | Promise<string[]>,
  reasonFor: (refusal: PrepareRefusal, current: Policy | undefined) => string,
): Promise<CommandPreparation> {
  let root = services.workingDirectory.get(agent.session)
  const policy = services.sandboxPolicy.resolve({ session: agent.session })
  if (policy.mode === 'read-only') return { root, reason: reasonFor('read-only', policy) }
  if (!validPolicy(policy)) return { root, reason: reasonFor('invalid-policy', policy) }
  root = await realpath(await services.workingDirectory.ensure(agent, signal))
  const command = await confinedCommand(services, await argvFor(root), policy, signal)
  // Confinement and directory validation are asynchronous. A policy change
  // during either step invalidates the prepared command; never reuse authority.
  if (command.error) return { root, reason: command.error.reason }
  const current = services.sandboxPolicy.resolve({ session: agent.session })
  if (!validPolicy(current) || !samePolicy(policy, current)) {
    return { root, reason: reasonFor('changed-policy', current) }
  }
  signal.throwIfAborted()
  return { root, command: { argv: command.argv!, ...(command.confined ? { confined: command.confined } : {}) } }
}

export function validPolicy(policy: Policy): boolean {
  return policy && ['read-only', 'workspace-write', 'danger-full-access'].includes(policy.mode) && path.isAbsolute(policy.workspaceRoot)
}
export function samePolicy(left: Policy, right: Policy): boolean {
  return left.mode === right.mode && left.workspaceRoot === right.workspaceRoot && left.sessionId === right.sessionId
    && JSON.stringify(left.additionalWritableRoots ?? []) === JSON.stringify(right.additionalWritableRoots ?? [])
}

/** Nonzero sandbox signatures are distinct from the binary's real exit result. */
export function confinementError(confined: ConfinedArgv, exitCode: number | null, stderr: string): CommandError | undefined {
  if (exitCode === 0) return undefined
  const text = stderr.toLowerCase()
  const failed = (confined.runnerFailureRules ?? []).some(rule =>
    (!rule.allowedExitCodes || (exitCode !== null && rule.allowedExitCodes.includes(exitCode)))
    && text.split(/\r?\n/).some(line => !(rule.informationalLines ?? []).some(s => s.toLowerCase() === line)
      && rule.fatalSignatures.some(s => line.includes(s.toLowerCase()))))
  if (failed) return { status: 'error', reason: 'DSH confinement runner failed' }
  if (confined.denialSignatures?.some(s => text.includes(s.toLowerCase()))) {
    return { status: 'refused', reason: 'DSH sandbox denied project file access' }
  }
  return undefined
}

export async function spawnCommand(services: HostServices, argv: string[], cwd: string, signal: AbortSignal): Promise<CommandOutcome> {
  const child = services.subprocess.spawn({ argv, cwd,
    stdio: { stdin: 'ignore', stdout: { maxBytes: 256 * 1024 }, stderr: { maxBytes: 64 * 1024 } },
    graceMs: 2000, signal })
  const outcome = await child.done
  const read = (key: 'stdout' | 'stderr') => {
    try { return child.collected[key]?.readFrom(0) } catch { return undefined }
  }
  const stdout = read('stdout')
  const stderr = read('stderr')
  return { exitCode: outcome.exitCode, signal: outcome.signal,
    stdout: stdout?.text ?? '', stderr: stderr?.text ?? '', truncated: Boolean(stdout?.lossy || stderr?.lossy),
    outputError: stdout && stderr ? null : 'DSH subprocess output readers are unavailable or failed' }
}
