import type { Context } from '@deepseek-ai/cordis'
import {
  DISPLAY_PARAMETERS, DISPLAY_RESULT_SCHEMA, IR_PARAMETERS, IR_RESULT_SCHEMA, REMOTE_DESCRIPTORS,
  REMOTE_SERVICE, parseDisplayRequest, parseIrRequest,
} from '@dsh-uml-viewer/contracts'
import type { Config } from './binary.js'
import { packageRoot } from './binary.js'
import { displayResult, runDisplay } from './display.js'
import type { HostServices } from './dsh.js'
import { registerFontRoute } from './font.js'
import { IrGate } from './gate.js'
import { PanelPrompts } from './prompt.js'
import { ResidentSessions } from './resident.js'
import { irResult, runIr } from './runner.js'
import { SessionStates, pluginHome } from './state.js'
import { ViewerService } from './viewer.js'

export type { Config } from './binary.js'
export const name = 'dsh-uml-viewer'
export const inject = ['tools', 'sandboxPolicy', 'workingDirectory', 'subprocess']

/** The DSH services this plugin uses; `sandbox`, `typert`, `webServer` and `dshHomePath` are optional. */
const SERVICE_KEYS = new Set(['tools', 'sandboxPolicy', 'workingDirectory', 'subprocess', 'sandbox',
  'typert', 'webServer', 'dshHomePath'])

export function apply(ctx: Context, config: Config = {}): void {

  // Every service is resolved when it is used, never snapshotted at apply
  // time: a profile may activate an optional row (sandbox, typert, webServer)
  // after this plugin, and a captured `undefined` would silently disable the
  // panel's Remote, the Sarasa route or confinement for the whole session.
  const services = new Proxy({}, {
    get: (_target, key) => (typeof key === 'string' && SERVICE_KEYS.has(key) ? ctx.get(key) : undefined),
  }) as unknown as HostServices

  const gate = new IrGate()
  const states = new SessionStates(pluginHome(config.home, () => {
    // Resolved lazily: `dshHomePath` may arrive after this plugin.
    const resolve = ctx.get('dshHomePath') as ((...segments: string[]) => string) | undefined
    return resolve ? resolve('dsh-uml-viewer') : undefined
  }))
  const sessions = new ResidentSessions(services, config, states)
  const prompts = new PanelPrompts()
  const viewer = new ViewerService(sessions, prompts)

  ctx.effect(() => services.tools.register({
    name: 'uml_ir',
    description: 'Generate UML IR from the session project’s policy and source tree. Writes the IR output; requires workspace-write or full access. Returns the actual exit code, validResult and bounded stdout/stderr.',
    parameters: IR_PARAMETERS,
    timeoutMs: 10 * 60_000,
    isConcurrencySafe: () => false,
    output: { schema: IR_RESULT_SCHEMA, render: (_args, value) => [{ type: 'text', text: JSON.stringify(value) }] },
    async execute(args, exec) {
      const sessionId = exec.agent?.session.id
      try {
        const request = parseIrRequest(args)
        const argv: string[] = []
        if (request.policy !== undefined) argv.push('--policy', request.policy)
        if (request.output !== undefined) argv.push('--output', request.output)
        const result = await runIr(services, config, exec, argv)
        // The display gate follows this plugin's own IR calls, per session.
        if (sessionId !== undefined && result.status !== 'refused') {
          gate.record(sessionId, { validResult: result.validResult, exitCode: result.exitCode, reason: result.reason })
        }
        return result
      } catch (error) {
        return irResult(exec.agent ? services.workingDirectory.get(exec.agent.session) : '', {
          reason: error instanceof Error ? error.message : String(error),
        })
      }
    },
  }), 'dsh-uml-viewer: uml_ir')

  ctx.effect(() => services.tools.register({
    name: 'uml_display',
    description: 'Show, in this session’s viewer panel, the diagram the latest successful uml_ir wrote. Refused until uml_ir has succeeded in this session, and in a read-only sandbox. Reports what the resident found instead of assuming the panel refreshed.',
    parameters: DISPLAY_PARAMETERS,
    timeoutMs: 60_000,
    isConcurrencySafe: () => true,
    output: { schema: DISPLAY_RESULT_SCHEMA, render: (_args, value) => [{ type: 'text', text: JSON.stringify(value) }] },
    async execute(args, exec) {
      try {
        return await runDisplay(services, sessions, gate, exec, parseDisplayRequest(args))
      } catch (error) {
        return displayResult(exec.agent ? services.workingDirectory.get(exec.agent.session) : '', {
          reason: error instanceof Error ? error.message : String(error),
        })
      }
    },
  }), 'dsh-uml-viewer: uml_display')

  // The typed Remote the browser half mounts. This is a fiber on `typert`, so a
  // registry that activates later still gets the service and the descriptors;
  // a profile without it keeps the tools and simply has no panel.
  ctx.inject(['typert'], (fiber) => {
    const typert = fiber.get('typert') as HostServices['typert']
    if (!typert) return
    const unprovide = fiber.provide(REMOTE_SERVICE, viewer)
    const dispose = typert.register({
      package: 'dsh-uml-viewer',
      face: 'host',
      schemas: [],
      model: { services: [], events: [], objects: [] },
      invocations: REMOTE_DESCRIPTORS,
    })
    return () => {
      void dispose()
      unprovide()
    }
  })

  // The packaged Sarasa route follows the web server's lifetime, likewise late.
  ctx.inject(['webServer'], () => registerFontRoute(services, packageRoot()) ?? (() => {}))

  ctx.effect(() => async () => {
    prompts.disposeAll()
    await sessions.dispose()
  }, 'dsh-uml-viewer: panel lifetime')
}
