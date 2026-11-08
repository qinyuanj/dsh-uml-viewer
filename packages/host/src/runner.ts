import { realpath } from 'node:fs/promises'
import path from 'node:path'
import type { ConfinedArgv, Policy } from './dsh.js'
import type { IrResult } from '@dsh-uml-viewer/contracts'
import { binaryArgv } from './binary.js'
import { defaultPolicy } from './policy.js'
import type { Config } from './binary.js'
import type { HostServices, ToolRunContext } from './dsh.js'

export function irResult(projectRoot: string, patch: Partial<IrResult> = {}): IrResult {
  return { version: 1, operation: 'ir', projectRoot, exitCode: null, validResult: false,
    crapScope: null, configured: false, tool: 'uml-viewer', status: 'refused', reason: null,
    stdout: '', stderr: '', truncated: false, ...patch }
}

function validPolicy(policy: Policy): boolean {
  return policy && ['read-only', 'workspace-write', 'danger-full-access'].includes(policy.mode) && path.isAbsolute(policy.workspaceRoot)
}
function samePolicy(left: Policy, right: Policy): boolean {
  return left.mode === right.mode && left.workspaceRoot === right.workspaceRoot && left.sessionId === right.sessionId
    && JSON.stringify(left.additionalWritableRoots ?? []) === JSON.stringify(right.additionalWritableRoots ?? [])
}

/** Shared subprocess seam for the quality tools and resident host to extend. */
export async function runIr(services: HostServices, config: Config, exec: ToolRunContext, args: string[]): Promise<IrResult> {
  const agent = exec.agent
  if (!agent?.session) return irResult('', { reason: 'uml_ir requires a live DSH session' })
  let root = services.workingDirectory.get(agent.session)
  try {
    const policy = services.sandboxPolicy.resolve({ session: agent.session })
    if (policy.mode === 'read-only') return irResult(root, { reason: 'read-only sandbox: uml_ir writes IR output. Switch the session to workspace-write to run it; no elevation was attempted.' })
    root = await services.workingDirectory.ensure(agent, exec.signal)
    root = await realpath(root)
    if (!validPolicy(policy)) return irResult(root, { reason: 'Invalid authoritative sandbox policy; no binary was started' })
    if (!args.includes('--policy')) args = ['--policy', await defaultPolicy(root), ...args]
    let argv = [...binaryArgv(config), 'ir', ...args]
    let confined: ConfinedArgv | undefined
    if (policy.mode === 'workspace-write') {
      if (!services.sandbox) return irResult(root, { reason: 'No DSH confinement provider; refusing unconfined execution' })
      confined = await services.sandbox.confine(argv, policy, exec.signal)
      if (!['full', 'partial'].includes(confined?.enforcement) || !confined.argv?.length) {
        return irResult(root, { reason: 'Invalid DSH confinement result; refusing unconfined execution' })
      }
      argv = confined.argv
    }
    // Directory validation and confinement are asynchronous. A policy change
    // during either step invalidates the prepared command; never reuse authority.
    const current = services.sandboxPolicy.resolve({ session: agent.session })
    if (!validPolicy(current) || !samePolicy(policy, current)) {
      return irResult(root, { reason: `Sandbox policy changed during preparation (${current?.mode}); run uml_ir again under the current mode` })
    }
    exec.signal.throwIfAborted()
    const child = services.subprocess.spawn({ argv, cwd: root,
      stdio: { stdin: 'ignore', stdout: { maxBytes: 256 * 1024 }, stderr: { maxBytes: 64 * 1024 } },
      graceMs: 2000, signal: exec.signal })
    const outcome = await child.done
    const stdout = child.collected.stdout?.readFrom(0)
    const stderr = child.collected.stderr?.readFrom(0)
    if (confined && outcome.exitCode !== 0) {
      const text = stderr?.text.toLowerCase() ?? ''
      const failed = confined.runnerFailureRules.some(rule =>
        (!rule.allowedExitCodes || (outcome.exitCode !== null && rule.allowedExitCodes.includes(outcome.exitCode)))
        && text.split(/\r?\n/).some(line => !(rule.informationalLines ?? []).some(s => s.toLowerCase() === line)
          && rule.fatalSignatures.some(s => line.includes(s.toLowerCase()))))
      const denied = confined.denialSignatures.some(s => text.includes(s.toLowerCase()))
      if (failed || denied) return irResult(root, { status: denied ? 'refused' : 'error', exitCode: outcome.exitCode,
        reason: failed ? 'DSH confinement runner failed' : 'DSH sandbox denied IR file access',
        stdout: stdout?.text ?? '', stderr: stderr?.text ?? '', truncated: Boolean(stdout?.lossy || stderr?.lossy) })
    }
    const validResult = outcome.exitCode === 0 && !outcome.signal && !exec.signal.aborted && Boolean(stdout && stderr)
    return irResult(root, { exitCode: outcome.exitCode, validResult, status: validResult ? 'ok' : 'error',
      reason: validResult ? null : `IR generation failed (exit ${outcome.exitCode}${outcome.signal ? `, ${outcome.signal}` : ''})`,
      stdout: stdout?.text ?? '', stderr: stderr?.text ?? '', truncated: Boolean(stdout?.lossy || stderr?.lossy) })
  } catch (error) {
    return irResult(root, { status: 'error', reason: error instanceof Error ? error.message : String(error) })
  }
}
