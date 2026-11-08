import { realpath } from 'node:fs/promises'
import type { IrResult } from '@dsh-uml-viewer/contracts'
import { binaryArgv } from './binary.js'
import type { Config } from './binary.js'
import type { HostServices, ToolRunContext } from './dsh.js'
import { defaultPolicy } from './policy.js'
import { confinedCommand, confinementError, samePolicy, spawnCommand, validPolicy } from './process.js'

export function irResult(projectRoot: string, patch: Partial<IrResult> = {}): IrResult {
  return { version: 1, operation: 'ir', projectRoot, exitCode: null, validResult: false,
    crapScope: null, configured: false, tool: 'uml-viewer', status: 'refused', reason: null,
    stdout: '', stderr: '', truncated: false, ...patch }
}

/** The IR tool consumes the operation-independent process policy and runner. */
export async function runIr(services: HostServices, config: Config, exec: ToolRunContext, args: string[]): Promise<IrResult> {
  const agent = exec.agent
  if (!agent?.session) return irResult('', { reason: 'uml_ir requires a live DSH session' })
  let root = services.workingDirectory.get(agent.session)
  try {
    const policy = services.sandboxPolicy.resolve({ session: agent.session })
    if (policy.mode === 'read-only') return irResult(root, { reason: 'read-only sandbox: uml_ir writes IR output. Switch the session to workspace-write to run it; no elevation was attempted.' })
    if (!validPolicy(policy)) return irResult(root, { reason: 'Invalid authoritative sandbox policy; no binary was started' })
    root = await realpath(await services.workingDirectory.ensure(agent, exec.signal))
    if (!args.includes('--policy')) args = ['--policy', await defaultPolicy(root), ...args]
    const command = await confinedCommand(services, [...binaryArgv(config), 'ir', ...args], policy, exec.signal)
    if (command.error) return irResult(root, command.error)
    // Directory validation and confinement are asynchronous. A policy change
    // during either step invalidates the prepared command; never reuse authority.
    const current = services.sandboxPolicy.resolve({ session: agent.session })
    if (!validPolicy(current) || !samePolicy(policy, current)) {
      return irResult(root, { reason: `Sandbox policy changed during preparation (${current?.mode}); run uml_ir again under the current mode` })
    }
    exec.signal.throwIfAborted()
    const output = await spawnCommand(services, command.argv!, root, exec.signal)
    if ('reason' in output) return irResult(root, output)
    const denied = command.confined && confinementError(command.confined, output.exitCode, output.stderr)
    if (denied) return irResult(root, { ...output, ...denied })
    const validResult = output.exitCode === 0 && !exec.signal.aborted
    return irResult(root, { ...output, validResult, status: validResult ? 'ok' : 'error',
      reason: validResult ? null : `IR generation failed (exit ${output.exitCode}${exec.signal.aborted ? ', interrupted' : ''})` })
  } catch (error) {
    return irResult(root, { status: 'error', reason: error instanceof Error ? error.message : String(error) })
  }
}
