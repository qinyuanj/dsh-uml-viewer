import type { Context } from '@deepseek-ai/cordis'
import { IR_PARAMETERS, IR_RESULT_SCHEMA, parseIrRequest } from '@dsh-uml-viewer/contracts'
import type { Config } from './binary.js'
import type { HostServices } from './dsh.js'
import { irResult, runIr } from './runner.js'

export type { Config } from './binary.js'
export const name = 'dsh-uml-viewer'
export const inject = ['tools', 'sandboxPolicy', 'workingDirectory', 'subprocess']

export function apply(ctx: Context, config: Config = {}): void {
  const services = Object.fromEntries(['tools', 'sandboxPolicy', 'workingDirectory', 'subprocess', 'sandbox']
    .map(key => [key, ctx.get(key)])) as unknown as HostServices
  ctx.effect(() => services.tools.register({
    name: 'uml_ir',
    description: 'Generate UML IR from the session project’s policy and source tree. Writes the IR output; requires workspace-write or full access. Returns the actual exit code, validResult and bounded stdout/stderr.',
    parameters: IR_PARAMETERS,
    timeoutMs: 10 * 60_000,
    isConcurrencySafe: () => false,
    output: { schema: IR_RESULT_SCHEMA, render: (_args, value) => [{ type: 'text', text: JSON.stringify(value) }] },
    async execute(args, exec) {
      try {
        const request = parseIrRequest(args)
        const argv: string[] = []
        if (request.policy !== undefined) argv.push('--policy', request.policy)
        if (request.output !== undefined) argv.push('--output', request.output)
        return await runIr(services, config, exec, argv)
      } catch (error) {
        return irResult(exec.agent ? services.workingDirectory.get(exec.agent.session) : '', {
          reason: error instanceof Error ? error.message : String(error),
        })
      }
    },
  }), 'dsh-uml-viewer: uml_ir')
}
