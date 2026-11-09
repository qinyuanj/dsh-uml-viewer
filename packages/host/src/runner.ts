import type { IrResult } from '@dsh-uml-viewer/contracts'
import { binaryArgv } from './binary.js'
import type { Config } from './binary.js'
import type { HostServices, ToolRunContext } from './dsh.js'
import { defaultPolicy } from './policy.js'
import { confinementError, prepareConfinedCommand, spawnCommand } from './process.js'

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
    const prepared = await prepareConfinedCommand(
      services, agent, exec.signal,
      async canonical => {
        if (!args.includes('--policy')) args = ['--policy', await defaultPolicy(canonical), ...args]
        return [...binaryArgv(config), 'ir', ...args]
      },
      (refusal, current) => {
        if (refusal === 'read-only') return 'read-only sandbox: uml_ir writes IR output. Switch the session to workspace-write to run it; no elevation was attempted.'
        if (refusal === 'invalid-policy') return 'Invalid authoritative sandbox policy; no binary was started'
        return `Sandbox policy changed during preparation (${current?.mode}); run uml_ir again under the current mode`
      },
    )
    root = prepared.root
    if (prepared.command === undefined) return irResult(root, { reason: prepared.reason })
    const command = prepared.command
    const output = await spawnCommand(services, command.argv, root, exec.signal)
    if ('reason' in output) return irResult(root, output)
    const { signal: terminationSignal, outputError, ...result } = output
    const denied = command.confined && confinementError(command.confined, output.exitCode, output.stderr)
    if (denied) return irResult(root, { ...result, ...denied })
    const validResult = output.exitCode === 0 && !terminationSignal && !outputError && !exec.signal.aborted
    const failure = terminationSignal ? `IR generation terminated by ${terminationSignal}${outputError ? `; ${outputError}` : ''}`
      : outputError ?? `IR generation failed (exit ${output.exitCode}${exec.signal.aborted ? ', interrupted' : ''})`
    return irResult(root, { ...result, validResult, status: validResult ? 'ok' : 'error',
      reason: validResult ? null : failure })
  } catch (error) {
    return irResult(root, { status: 'error', reason: error instanceof Error ? error.message : String(error) })
  }
}
