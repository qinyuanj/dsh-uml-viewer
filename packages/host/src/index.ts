import type { Context } from '@deepseek-ai/cordis'
import {
  CRAP_PARAMETERS, DISPLAY_PARAMETERS, DISPLAY_RESULT_SCHEMA, DRY_PARAMETERS, IR_PARAMETERS,
  IR_RESULT_SCHEMA, MUTATE_PARAMETERS, QUALITY_RESULT_SCHEMA, REMOTE_DESCRIPTORS,
  REMOTE_SERVICE, parseDisplayRequest, parseIrRequest, parseQualityToolRequest,
} from '@dsh-uml-viewer/contracts'
import type { QualityOperation, QualityToolResult } from '@dsh-uml-viewer/contracts'
import type { ActivatedBinary, BinaryPlatform, Config } from './binary.js'
import { activationBinary, packageRoot } from './binary.js'
import { recordActivation } from './activation.js'
import { displayResult, runDisplay } from './display.js'
import type { HostServices, ToolRunContext } from './dsh.js'
import { registerFontRoute } from './font.js'
import { IrGate, QualityGate, RequestGrants } from './gate.js'
import { qualityResult, runQuality } from './quality.js'
import { panelLifetime } from './lifetime.js'
import { PanelRequests } from './requests.js'
import { irResult, runIr } from './runner.js'
import { SessionStates, pluginHome } from './state.js'
import { ViewerService } from './viewer.js'

export type { Config } from './binary.js'
export const name = 'dsh-uml-viewer'
export const inject = ['tools', 'sandboxPolicy', 'workingDirectory', 'subprocess']

/** The DSH services this plugin uses; `sandbox`, `typert`, `webServer`, `dshHomePath` and
 * `sessionController` (panel request messages, #28) are optional. */
const SERVICE_KEYS = new Set(['tools', 'sandboxPolicy', 'workingDirectory', 'subprocess', 'sandbox',
  'typert', 'webServer', 'dshHomePath', 'sessionController'])

export type ActivationPlatform = BinaryPlatform

export function apply(ctx: Context, config: Config = {}): void {
  applyWithPlatform(ctx, config, process)
}

/** Public activation boundary, including platform validation before any registration. */
export function applyWithPlatform(ctx: Context, config: Config = {}, platform: ActivationPlatform = process): void {
  const home = pluginHome(config.home, () => {
    const resolve = ctx.get('dshHomePath') as ((...segments: string[]) => string) | undefined
    return resolve ? resolve('dsh-uml-viewer') : undefined
  })
  // Resolve before any effects, injected fibers or services can be registered.
  // The argv copy also pins trusted dev launchers and environment overrides.
  const pinned = activationBinary(config, home, platform)
  // #32: which build this activation runs — and whether it could pin one at all
  // — is the first fact an upgrade report needs, and an unversioned override
  // leaves no other trace in the store.
  recordActivation(home, { version: pinned.version, source: pinned.source, root: pinned.root, argv: pinned.argv })
  try {
    register(ctx, config, home, pinned)
  } catch (error) {
    pinned.release()
    throw error
  }
}

function register(ctx: Context, config: Config, home: string | undefined, pinned: ActivatedBinary): void {
  const effectiveConfig: Config = { ...config, binary: pinned.argv }

  // Every service is resolved when it is used, never snapshotted at apply
  // time: a profile may activate an optional row (sandbox, typert, webServer)
  // after this plugin, and a captured `undefined` would silently disable the
  // panel's Remote, the Sarasa route or confinement for the whole session.
  const services = new Proxy({}, {
    get: (_target, key) => {
      if (typeof key !== 'string' || !SERVICE_KEYS.has(key)) return undefined
      const service = ctx.get(key)
      if (key !== 'subprocess' || !service) return service
      return { spawn: (spec: Parameters<HostServices['subprocess']['spawn']>[0]) => {
        const release = pinned.retain()
        try {
          const child = (service as HostServices['subprocess']).spawn(spec)
          // Tool calls, as well as residents, keep their executable until a
          // real exit. A failed observation is not permission to clean up.
          void child.done.then(release, () => {})
          return child
        } catch (error) {
          release()
          throw error
        }
      } }
    },
  }) as unknown as HostServices

  const gate = new IrGate()
  const quality = new QualityGate()
  const grants = new RequestGrants()
  const states = new SessionStates(home)
  const { sessions, prompts } = panelLifetime(ctx, home, services, effectiveConfig, states)
  const requests = new PanelRequests(services, sessions, grants)
  const viewer = new ViewerService(sessions, prompts, requests, requests.statuses)

  ctx.effect(() => services.tools.register({
    name: 'uml_ir',
    description: 'Generate UML IR from the session project’s policy and source tree. Writes the IR output; requires workspace-write or full access. Runs after any quality call already in progress in this session, and is refused while the latest one has validResult false, unless `request` names the UML viewer panel request (:omit or :new-proposal) this IR answers; that id is single-use and valid only in this session. Returns the actual exit code, validResult and bounded stdout/stderr.',
    parameters: IR_PARAMETERS,
    timeoutMs: 10 * 60_000,
    isConcurrencySafe: () => false,
    output: { schema: IR_RESULT_SCHEMA, render: (_args, value) => [{ type: 'text', text: JSON.stringify(value) }] },
    async execute(args, exec) {
      const sessionId = exec.agent?.session.id
      // Queued behind earlier quality calls: IR reads the snapshots they write
      // and must see their outcome, never the one before an in-flight call.
      return sessionId === undefined ? generate(args, exec) : quality.run(sessionId, () => generate(args, exec))
    },
  }), 'dsh-uml-viewer: uml_ir')

  async function generate(args: unknown, exec: ToolRunContext): Promise<ReturnType<typeof irResult>> {
    const sessionId = exec.agent?.session.id
    const root = exec.agent ? services.workingDirectory.get(exec.agent.session) : ''
    let result: ReturnType<typeof irResult>
    try {
      const request = parseIrRequest(args)
      // A panel request that needs no quality refresh (#29) presents its
      // single-use id: it skips only the failed-quality check. The FIFO above
      // and the display gate below stay as they are.
      const granted = request.request === undefined || sessionId === undefined ? null : grants.consume(sessionId, request.request)
      const blocked = granted ? (granted.ok ? null : granted) : sessionId === undefined ? null : quality.check(sessionId)
      if (request.request !== undefined && sessionId === undefined) {
        result = irResult(root, { status: 'refused', reason: 'a panel request id needs the session that received the request' })
      } else if (blocked && !blocked.ok) {
        result = irResult(root, { status: 'refused', reason: blocked.reason })
      } else {
        const argv: string[] = []
        if (request.policy !== undefined) argv.push('--policy', request.policy)
        if (request.output !== undefined) argv.push('--output', request.output)
        result = await runIr(services, effectiveConfig, exec, argv)
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
  }

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
            result = await runQuality(services, effectiveConfig, exec, operation, parseQualityToolRequest(operation, args))
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
      const sessionId = exec.agent?.session.id
      try {
        const result = await runDisplay(services, sessions, gate, exec, parseDisplayRequest(args))
        // #54: a display that answers a panel request names the element that
        // request was about, once, so 「在面板中查看」 can locate it. A failed
        // display leaves the request unanswered and keeps its target.
        if (result.status !== 'ok' || sessionId === undefined) return result
        return { ...result, target: requests.takeTarget(sessionId) }
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

  // Replacement retires this activation, while open panel streams own their
  // residents until last-panel close. The root lifetime stops them on shutdown.
  ctx.effect(() => async () => {
    grants.clear()
    try {
      await sessions.checkpoint()
    } finally {
      pinned.release()
    }
  }, 'dsh-uml-viewer: activation version lease')
}
