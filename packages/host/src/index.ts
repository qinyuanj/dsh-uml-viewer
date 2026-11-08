import type { Context } from '@deepseek-ai/cordis'
import {
  CRAP_PARAMETERS, DISPLAY_PARAMETERS, DISPLAY_RESULT_SCHEMA, DRY_PARAMETERS, IR_PARAMETERS,
  IR_RESULT_SCHEMA, MUTATE_PARAMETERS, QUALITY_RESULT_SCHEMA, REMOTE_DESCRIPTORS,
  REMOTE_SERVICE, parseDisplayRequest, parseIrRequest, parseQualityToolRequest,
} from '@dsh-uml-viewer/contracts'
import type { QualityOperation, QualityToolResult } from '@dsh-uml-viewer/contracts'
import type { Config } from './binary.js'
import { packageRoot } from './binary.js'
import { displayResult, runDisplay } from './display.js'
import type { HostServices } from './dsh.js'
import { registerFontRoute } from './font.js'
import { IrGate, QualityGate } from './gate.js'
import { PanelPrompts } from './prompt.js'
import { qualityResult, runQuality } from './quality.js'
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
  const quality = new QualityGate()
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
    description: 'Generate UML IR from the session project’s policy and source tree. Writes the IR output; requires workspace-write or full access. Refused while the latest quality call in this session has validResult false. Returns the actual exit code, validResult and bounded stdout/stderr.',
    parameters: IR_PARAMETERS,
    timeoutMs: 10 * 60_000,
    isConcurrencySafe: () => false,
    output: { schema: IR_RESULT_SCHEMA, render: (_args, value) => [{ type: 'text', text: JSON.stringify(value) }] },
    async execute(args, exec) {
      const sessionId = exec.agent?.session.id
      const root = exec.agent ? services.workingDirectory.get(exec.agent.session) : ''
      let result: ReturnType<typeof irResult>
      try {
        const request = parseIrRequest(args)
        const blocked = sessionId === undefined ? null : quality.check(sessionId)
        if (blocked && !blocked.ok) {
          result = irResult(root, { status: 'refused', reason: blocked.reason })
        } else {
          const argv: string[] = []
          if (request.policy !== undefined) argv.push('--policy', request.policy)
          if (request.output !== undefined) argv.push('--output', request.output)
          result = await runIr(services, config, exec, argv)
        }
      } catch (error) {
        result = irResult(root, { reason: error instanceof Error ? error.message : String(error) })
      }
      // Every completed invocation replaces the previous grant. In particular,
      // a parse failure or refusal must not leave an earlier success displayable.
      if (sessionId !== undefined) {
        gate.record(sessionId, { validResult: result.validResult, exitCode: result.exitCode, reason: result.reason })
      }
      return result
    },
  }), 'dsh-uml-viewer: uml_ir')

  /**
   * One quality tool. Every invocation in a session is queued behind the
   * previous one, and its result (including a refusal or malformed arguments)
   * becomes the latest quality outcome that gates `uml_ir`.
   */
  const registerQuality = (
    operation: QualityOperation,
    name: string,
    description: string,
    parameters: unknown,
    timeoutMs: number,
  ): void => {
    ctx.effect(() => services.tools.register({
      name,
      description,
      parameters,
      timeoutMs,
      isConcurrencySafe: () => false,
      output: { schema: QUALITY_RESULT_SCHEMA, render: (_args, value) => [{ type: 'text', text: JSON.stringify(value) }] },
      async execute(args, exec) {
        const sessionId = exec.agent?.session.id
        const attempt = async (): Promise<QualityToolResult> => {
          const root = exec.agent ? services.workingDirectory.get(exec.agent.session) : ''
          let result: QualityToolResult
          try {
            result = await runQuality(services, config, exec, operation, parseQualityToolRequest(operation, args))
          } catch (error) {
            result = qualityResult(operation, root, { reason: error instanceof Error ? error.message : String(error) })
          }
          // Inside the queued task: the next quality call and uml_ir can never
          // observe an older outcome after the newer invocation has finished.
          if (sessionId !== undefined) quality.record(sessionId, result)
          return result
        }
        return sessionId === undefined ? attempt() : quality.run(sessionId, attempt)
      },
    }), `dsh-uml-viewer: ${name}`)
  }

  registerQuality('crap', 'uml_crap',
    'Refresh CRAP quality snapshots for the session project by running `uml-viewer quality crap`. Writes project metrics; in a read-only sandbox it refuses without starting the binary. Returns the binary’s version-1 quality record, including the actual exit code, validResult and any refusal reason. A validResult false result blocks uml_ir until a later quality call succeeds.',
    CRAP_PARAMETERS, 10 * 60_000)
  registerQuality('mutate', 'uml_mutate',
    'Run differential mutation for the session project by running `uml-viewer quality mutate`. Pass every target file in the one `targets` array so they share one coverage run and baseline, as the binary requires. Writes mutation snapshots; in a read-only sandbox it refuses without starting the binary. Returns the binary’s version-1 quality record, including the actual exit code, validResult and any refusal reason. A validResult false result blocks uml_ir until a later quality call succeeds.',
    MUTATE_PARAMETERS, 60 * 60_000)
  registerQuality('dry', 'uml_dry',
    'Report duplicate structure for the session project by running `uml-viewer quality dry`; the report is written to project metrics. In a read-only sandbox it refuses without starting the binary. Returns the binary’s version-1 quality record, including the actual exit code, validResult and any refusal reason. A validResult false result blocks uml_ir until a later quality call succeeds.',
    DRY_PARAMETERS, 10 * 60_000)

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
