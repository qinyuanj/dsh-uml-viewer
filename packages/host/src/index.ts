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
import type { HostServices, SessionEventFeed, ToolRunContext } from './dsh.js'
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

const SERVICE_KEYS = new Set(['tools', 'sandboxPolicy', 'workingDirectory', 'subprocess', 'sandbox',
  'typert', 'webServer', 'dshHomePath', 'sessionController'])

export type ActivationPlatform = BinaryPlatform

export function apply(ctx: Context, config: Config = {}): void {
  applyWithPlatform(ctx, config, process)
}

export function applyWithPlatform(ctx: Context, config: Config = {}, platform: ActivationPlatform = process): void {
  const home = pluginHome(config.home, () => {
    const resolve = ctx.get('dshHomePath') as ((...segments: string[]) => string) | undefined
    return resolve ? resolve('dsh-uml-viewer') : undefined
  })
  const pinned = activationBinary(config, home, platform)
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
  const services = new Proxy({}, {
    get: (_target, key) => {
      if (typeof key !== 'string' || !SERVICE_KEYS.has(key)) return undefined
      const service = ctx.get(key)
      if (key !== 'subprocess' || !service) return service
      return { spawn: (spec: Parameters<HostServices['subprocess']['spawn']>[0]) => {
        const release = pinned.retain()
        try {
          const child = (service as HostServices['subprocess']).spawn(spec)
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

  // #52: a turn boundary is the one host event that says the companion is done
  const feed = ctx as unknown as SessionEventFeed
  feed.on('session/event', (session, event) => {
    if (event?.type === 'turn/start') {
      requests.statuses.turnStarted(session.id)
    } else if (event?.type === 'turn/end') {
      requests.statuses.turnEnded(session.id)
    } else if (event?.type === 'user/message') {
      const rpcId = event.data?.source?.rpcId
      if (typeof rpcId === 'string') requests.statuses.claim(session.id, rpcId)
    }
  })

  ctx.effect(() => services.tools.register({
    name: 'uml_ir',
    description: 'Generate UML IR from the session project’s policy and source tree. Writes the IR output; requires workspace-write or full access. Runs after any quality call already in progress in this session, and is refused while the latest one has validResult false, unless `request` names the UML viewer panel request (:omit or :new-proposal) this IR answers; that id is single-use and valid only in this session. Returns the actual exit code, validResult and bounded stdout/stderr.',
    parameters: IR_PARAMETERS,
    timeoutMs: 10 * 60_000,
    isConcurrencySafe: () => false,
    output: { schema: IR_RESULT_SCHEMA, render: (_args, value) => [{ type: 'text', text: JSON.stringify(value) }] },
    async execute(args, exec) {
      const sessionId = exec.agent?.session.id
      const request = presentedRequest(args)
      const answered = sessionId === undefined ? null : requests.statuses.begin(sessionId, request)?.id ?? request ?? null
      const result = sessionId === undefined ? await generate(args, exec, answered) : await quality.run(sessionId, () => generate(args, exec, answered))
      if (sessionId !== undefined && !result.validResult && answered !== null) {
        requests.statuses.end(sessionId, { state: 'failed',
          reason: result.reason ?? 'the IR generation failed', exitCode: result.exitCode }, answered)
      }
      return result
    },
  }), 'dsh-uml-viewer: uml_ir')

  async function generate(args: unknown, exec: ToolRunContext, answered: string | null): Promise<ReturnType<typeof irResult>> {
    const sessionId = exec.agent?.session.id
    const root = exec.agent ? services.workingDirectory.get(exec.agent.session) : ''
    let result: ReturnType<typeof irResult>
    try {
      const request = parseIrRequest(args)
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
    if (sessionId !== undefined) {
      gate.record(sessionId, { validResult: result.validResult, exitCode: result.exitCode, reason: result.reason,
        requestId: answered })
    }
    return result
  }

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
        if (sessionId !== undefined) requests.statuses.begin(sessionId)
        const attempt = async (): Promise<QualityToolResult> => {
          const root = exec.agent ? services.workingDirectory.get(exec.agent.session) : ''
          let result: QualityToolResult
          try {
            result = await runQuality(services, effectiveConfig, exec, operation, parseQualityToolRequest(operation, args))
          } catch (error) {
            result = qualityResult(operation, root, { reason: error instanceof Error ? error.message : String(error) })
          }
          if (sessionId !== undefined) quality.record(sessionId, result)
          return result
        }
        const result = sessionId === undefined ? await attempt() : await quality.run(sessionId, attempt)
        if (sessionId !== undefined && !result.validResult) {
          requests.statuses.end(sessionId, { state: 'failed',
            reason: result.reason ?? QUALITY_FAILED_REASON, exitCode: result.exitCode })
        }
        return result
      },
    }), `dsh-uml-viewer: ${name}`)
  }

  registerQuality('crap', 'uml_crap',
    'Refresh CRAP quality snapshots for the session project by running `uml-viewer quality crap`. Writes project metrics; in a read-only sandbox it refuses without starting the binary. Returns the binary’s version-1 quality record, including the actual exit code, validResult and the reason it failed — a dispatcher refusal, or the bounded stderr tail of the tool itself. A validResult false result blocks uml_ir until a later quality call succeeds.',
    CRAP_PARAMETERS, 10 * 60_000)
  registerQuality('mutate', 'uml_mutate',
    'Run differential mutation for the session project by running `uml-viewer quality mutate`. Pass every target file in the one `targets` array so they share one coverage run and baseline, as the binary requires. Writes mutation snapshots; in a read-only sandbox it refuses without starting the binary. Returns the binary’s version-1 quality record, including the actual exit code, validResult and the reason it failed — a dispatcher refusal, or the bounded stderr tail of the tool itself. A validResult false result blocks uml_ir until a later quality call succeeds.',
    MUTATE_PARAMETERS, 60 * 60_000)
  registerQuality('dry', 'uml_dry',
    'Report duplicate structure for the session project by running `uml-viewer quality dry`; the report is written to project metrics. In a read-only sandbox it refuses without starting the binary. Returns the binary’s version-1 quality record, including the actual exit code, validResult and the reason it failed — a dispatcher refusal, or the bounded stderr tail of the tool itself. A validResult false result blocks uml_ir until a later quality call succeeds.',
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
      const answered = sessionId === undefined ? null : gate.answered(sessionId)
      if (sessionId !== undefined && answered !== null) requests.statuses.begin(sessionId, answered)
      try {
        const result = await runDisplay(services, sessions, gate, exec, parseDisplayRequest(args))
        if (result.status !== 'ok' || sessionId === undefined) return result
        const entry = answered === null ? undefined : requests.statuses.end(sessionId, { state: 'done' }, answered)
        return { ...result, target: entry?.target ?? null }
      } catch (error) {
        return displayResult(exec.agent ? services.workingDirectory.get(exec.agent.session) : '', {
          reason: error instanceof Error ? error.message : String(error),
        })
      }
    },
  }), 'dsh-uml-viewer: uml_display')

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

  ctx.inject(['webServer'], () => registerFontRoute(services, packageRoot()) ?? (() => {}))

  ctx.effect(() => async () => {
    grants.clear()
    try {
      await sessions.checkpoint()
    } finally {
      pinned.release()
    }
  }, 'dsh-uml-viewer: activation version lease')
}

function presentedRequest(args: unknown): string | undefined {
  if (!args || typeof args !== 'object' || Array.isArray(args)) return undefined
  const request = (args as Record<string, unknown>).request
  return typeof request === 'string' && /^[0-9a-f]{32}$/.test(request) ? request : undefined
}

const QUALITY_FAILED_REASON = 'the quality run reported validResult false'
