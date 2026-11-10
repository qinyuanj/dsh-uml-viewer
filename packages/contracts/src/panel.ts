/**
 * The panel's host↔client seam: one stream of scene/display events and one
 * action call, plus the events and actions themselves.
 *
 * The descriptors are hand-written Typert `InvocationDescriptor`s (no
 * generator in this package). They are plain data with `create(): { parse }`
 * codecs, so the host and the browser bundle share one declaration.
 */
import type { BreadcrumbLevel, DiscussionContext, SearchResult, SessionState, SessionStatus } from './resident.js'
import type { SceneFrame } from './scene-frame.js'
import { validateSceneFrame } from './scene-frame.js'
import { validateBreadcrumbs, validateSessionState, validateSessionStatus } from './resident.js'
import { normalizeRelativePath } from './tools.js'
import type { CardModel, InspectorModel, SourceModel } from './overlays.js'
import { validateCardModel, validateInspectorModel, validateSourceModel } from './overlays.js'
import type { QualityRequestOp } from './requests.js'
import { parseQualityRequestOp } from './requests.js'
import { parseElementId } from './tools.js'
import { parseProposalName } from './view-requests.js'
import type { RequestStatusEntry } from './request-status.js'
import { validateRequestStatusList } from './request-status.js'

/** The Remote namespace the panel mounts. */
export const REMOTE_SERVICE = 'umlViewer'

/** Where the host serves the packaged Sarasa face (a real `font/ttf` route). */
export const PANEL_FONT_PATH = '/dsh-uml-viewer/font/SarasaMonoSC-Regular.ttf'

export type PanelEvent =
  | { kind: 'ready'; actions: string[] }
  | { kind: 'frame'; seq: number | null; frame: SceneFrame }
  | { kind: 'context'; context: DiscussionContext }
  | { kind: 'state'; state: SessionState; breadcrumbs: BreadcrumbLevel[]; status: SessionStatus | null }
  | { kind: 'search'; query: string; results: SearchResult[] }
  | { kind: 'refused'; reason: string }
  | { kind: 'failed'; reason: string }
  // #51: this session's whole request-status list, oldest first; sent when the
  // stream opens and again on every change.
  | { kind: 'request-status'; requests: RequestStatusEntry[] }

/**
 * The declutter modes the resident knows (#46), from `scene::hierarchy`. The
 * More menu names one instead of cycling to it.
 */
export type DeclutterMode = 'full' | 'arrows' | 'triangles' | 'elements' | 'classes'

/** Every declutter mode, in the resident's cycle order. */
export const DECLUTTER_MODES: readonly DeclutterMode[] = ['full', 'arrows', 'triangles', 'elements', 'classes']

export type PanelAction =
  | { action: 'viewport'; viewport: { width: number; height: number } }
  | { action: 'camera'; x: number; y: number; zoom: number }
  | { action: 'display'; path?: string }
  | { action: 'reload' } | { action: 'namespace-tree' } | { action: 'save' }
  // #46: the More menu's Remove arrows toggle names the declutter mode it
  // wants; no mode cycles as the inspector's button did.
  | { action: 'declutter'; mode?: DeclutterMode }
  // #45: a breadcrumb click names the level to return to; no target is one
  // level up, and `null` is the source diagram's root.
  | { action: 'back'; target?: string | null }
  | { action: 'drill-in'; id?: string; at?: [number, number] }
  | { action: 'select'; at: [number, number] | null }
  | { action: 'search'; query: string }
  | { action: 'locate'; id: string }
  | { action: 'context'; proposalId: string | null }
  // #27 overlays: the answers come back on the ack, so one panel's card or
  // source window never appears in another panel of the same session.
  | { action: 'inspector' }
  | { action: 'card'; width?: number; viewHeight?: number }
  | { action: 'open-card'; id?: string; at?: [number, number] }
  | { action: 'close-card' }
  | { action: 'scroll-card'; delta: number; viewHeight: number }
  | { action: 'open-source'; id: string; member?: string }
  // #28: a right-click quality refresh. The host resolves the files and
  // queues the visible request message in this panel's session.
  | { action: 'quality-request'; request: QualityRequestOp; id: string }
  // #29: hiding an element and adding a proposal are messages to this panel's
  // session too; the companion edits the policy and regenerates the IR.
  | { action: 'omit'; id: string }
  | { action: 'new-proposal'; name?: string }
  // #44: DSH switched language; the resident lays its text out again.
  | { action: 'language'; language: string }

/** Presentation ownership, scoped by the Remote's authoritative agent lookup. */
export interface PanelOwner {
  /** Stable for one browser document, including client HMR and surface moves. */
  pageId: string
  /** Unique for one mount/open; a stale mount cannot close its replacement. */
  holderId: string
}

export function parsePanelOwner(value: unknown): PanelOwner {
  const source = record(value, 'panel owner')
  const pageId = str(source.pageId, 'panel owner.pageId')
  const holderId = str(source.holderId, 'panel owner.holderId')
  for (const id of [pageId, holderId]) {
    if (!/^[a-zA-Z0-9_-]{1,128}$/.test(id)) throw new Error('panel owner ids must contain 1–128 ASCII letters, digits, underscores or hyphens')
  }
  return { pageId, holderId }
}

/**
 * The DSH language a panel passes to its resident (#44): a locale tag such as
 * `zh-CN` or `en`. The resident reads any Chinese tag as Chinese and the rest
 * as English; the tag only has to be short and plain.
 * @param value the tag, or nothing when an older panel passes none.
 * @param label the field name used in failure messages.
 * @returns the tag, or `null` for none.
 */
export function parsePanelLanguage(value: unknown, label = 'language'): string | null {
  if (value === undefined || value === null) return null
  if (typeof value !== 'string' || !/^[A-Za-z0-9_-]{1,35}$/.test(value)) {
    throw new Error(`${label}: must be a locale tag of 1–35 ASCII letters, digits, underscores or hyphens`)
  }
  return value
}

/** One action's outcome; the events it caused arrive on the watch stream. */
export interface PanelAck {
  accepted: boolean
  reason: string | null
  /** The inspector model, when the action was `inspector`. */
  inspector?: InspectorModel
  /** The class card paint plan, when the action was `card`. */
  card?: CardModel
  /** The opened source, when the action was `open-source`. */
  source?: SourceModel
}

/**
 * The answer to the panel's explicit `close`. Only this call is a definite
 * close: a watch stream ending on its own is a carrier loss (a reconnect, an
 * HMR client swap, the sidebar/main reparent) and keeps the resident in its
 * bounded grace (D13, #32).
 */
export interface PanelCloseAck {
  /** Whether a resident was still there to close; false when it was already gone. */
  closed: boolean
}

export interface RemoteSchema { parse(value: unknown): unknown }
export interface RemoteCodec {
  readonly mode: 'strict'
  readonly typeSymbol: string
  readonly create: () => RemoteSchema
}
export interface RemoteParameter {
  readonly name: string
  readonly wire: string
  readonly source: 'json' | 'lookup'
  readonly lookup?: string
  readonly codec: RemoteCodec
}
export interface RemoteDescriptor {
  readonly id: string
  readonly service: string
  readonly namespace: string
  readonly method: string
  readonly mode?: 'stream'
  readonly invocation: { readonly kind: 'direct' }
  readonly scope?: { readonly context: string; readonly wire: string }
  readonly parameters: readonly RemoteParameter[]
  readonly cancellation?: { readonly parameter: 'signal' }
  readonly result: RemoteCodec
}

class Invalid extends Error {}

function at(label: string, message: string): never {
  throw new Invalid(`${label}: ${message}`)
}

function record(value: unknown, label: string): Record<string, unknown> {
  if (!value || typeof value !== 'object' || Array.isArray(value)) at(label, 'must be an object')
  return value as Record<string, unknown>
}

function str(value: unknown, label: string): string {
  if (typeof value !== 'string') at(label, 'must be a string')
  return value
}

function num(value: unknown, label: string): number {
  if (typeof value !== 'number' || !Number.isFinite(value)) at(label, 'must be a finite number')
  return value
}

/** A world point or `null`, as panel actions carry it. */
function pointOrNull(value: unknown, label: string): [number, number] | null {
  if (value === null || value === undefined) return null
  if (!Array.isArray(value) || value.length !== 2) at(label, 'must be [x, y] or null')
  return [num(value[0], `${label}[0]`), num(value[1], `${label}[1]`)]
}

/**
 * Validate one event of the panel stream.
 * @param value the item the host yielded.
 * @param label the document name used in failure messages.
 * @returns the same event, narrowed.
 */
export function parsePanelEvent(value: unknown, label = 'panel event'): PanelEvent {
  try {
    const source = record(value, label)
    const kind = str(source.kind, `${label}.kind`)
    switch (kind) {
      case 'ready':
        if (!Array.isArray(source.actions)) at(`${label}.actions`, 'must be an array')
        return { kind, actions: source.actions.map((entry, index) => str(entry, `${label}.actions[${index}]`)) }
      case 'frame': {
        const seq = source.seq === null || source.seq === undefined ? null : num(source.seq, `${label}.seq`)
        return { kind, seq, frame: validateSceneFrame(source.frame, `${label}.frame`) }
      }
      case 'context': {
        const context = record(source.context, `${label}.context`)
        const contextKind = str(context.kind, `${label}.context.kind`)
        if (contextKind !== 'real' && contextKind !== 'proposal') at(`${label}.context.kind`, 'must be "real" or "proposal"')
        const parsed: DiscussionContext = { kind: contextKind, edn: str(context.edn, `${label}.context.edn`) }
        if (context.proposalId !== undefined && context.proposalId !== null) {
          parsed.proposalId = str(context.proposalId, `${label}.context.proposalId`)
        }
        if (context.name !== undefined && context.name !== null) {
          parsed.name = str(context.name, `${label}.context.name`)
        }
        return { kind, context: parsed }
      }
      case 'state':
        return { kind, state: validateSessionState(source.state, `${label}.state`),
          breadcrumbs: validateBreadcrumbs(source.breadcrumbs, `${label}.breadcrumbs`),
          status: validateSessionStatus(source.status, `${label}.status`) }
      case 'search':
        if (!Array.isArray(source.results)) at(`${label}.results`, 'must be an array')
        return { kind, query: str(source.query, `${label}.query`),
          results: source.results.map((entry, index) => {
            const row = record(entry, `${label}.results[${index}]`)
            return { id: str(row.id, `${label}.results[${index}].id`), name: str(row.name, `${label}.results[${index}].name`),
              ns: str(row.ns, `${label}.results[${index}].ns`), label: str(row.label, `${label}.results[${index}].label`) }
          }) }
      case 'refused':
        return { kind, reason: str(source.reason, `${label}.reason`) }
      case 'failed':
        return { kind, reason: str(source.reason, `${label}.reason`) }
      case 'request-status':
        return { kind, requests: validateRequestStatusList(source.requests, `${label}.requests`) }
      default:
        return at(`${label}.kind`, `unknown panel event ${JSON.stringify(source.kind)}`)
    }
  } catch (error) {
    if (error instanceof Invalid) throw new Error(error.message)
    throw error
  }
}

/**
 * Validate one panel action before it reaches the resident session.
 * @param value the action the panel sent.
 * @param label the document name used in failure messages.
 * @returns the same action, narrowed.
 */
export function parsePanelAction(value: unknown, label = 'panel action'): PanelAction {
  try {
    const source = record(value, label)
    const action = str(source.action, `${label}.action`)
    switch (action) {
      case 'viewport': {
        const viewport = record(source.viewport, `${label}.viewport`)
        return { action, viewport: { width: num(viewport.width, `${label}.viewport.width`), height: num(viewport.height, `${label}.viewport.height`) } }
      }
      case 'camera':
        return { action, x: num(source.x, `${label}.x`), y: num(source.y, `${label}.y`), zoom: num(source.zoom, `${label}.zoom`) }
      case 'display': {
        const path = source.path === undefined ? undefined : normalizeRelativePath('path', source.path)
        return path === undefined ? { action } : { action, path }
      }
      case 'reload': case 'namespace-tree': case 'save':
        return { action }
      case 'declutter': {
        if (source.mode === undefined || source.mode === null) return { action }
        const mode = str(source.mode, `${label}.mode`)
        if (!(DECLUTTER_MODES as readonly string[]).includes(mode)) at(`${label}.mode`, `unknown declutter mode ${JSON.stringify(mode)}`)
        return { action, mode: mode as DeclutterMode }
      }
      case 'back': {
        const target = source.target === undefined ? undefined
          : source.target === null ? null : str(source.target, `${label}.target`)
        return target === undefined ? { action } : { action, target }
      }
      case 'drill-in': {
        const id = source.id === undefined || source.id === null ? undefined : str(source.id, `${label}.id`)
        const point = source.at === undefined || source.at === null ? undefined : pointOrNull(source.at, `${label}.at`)
        if (id === undefined && point === undefined) at(label, 'drill-in needs an id or a point')
        return { action, ...(id === undefined ? {} : { id }), ...(point === undefined ? {} : { at: point! }) }
      }
      case 'select':
        return { action, at: pointOrNull(source.at, `${label}.at`) }
      case 'search':
        return { action, query: str(source.query, `${label}.query`) }
      case 'locate':
        return { action, id: str(source.id, `${label}.id`) }
      case 'context': {
        const proposalId = source.proposalId === null || source.proposalId === undefined
          ? null : str(source.proposalId, `${label}.proposalId`)
        return { action, proposalId }
      }
      case 'inspector': case 'close-card':
        return { action }
      case 'card': {
        const width = source.width === undefined || source.width === null ? undefined : num(source.width, `${label}.width`)
        const viewHeight = source.viewHeight === undefined || source.viewHeight === null
          ? undefined : num(source.viewHeight, `${label}.viewHeight`)
        return { action, ...(width === undefined ? {} : { width }), ...(viewHeight === undefined ? {} : { viewHeight }) }
      }
      case 'open-card': {
        const id = source.id === undefined || source.id === null ? undefined : str(source.id, `${label}.id`)
        const point = source.at === undefined || source.at === null ? undefined : pointOrNull(source.at, `${label}.at`)
        if (id === undefined && point === undefined) at(label, 'open-card needs an id or a point')
        return { action, ...(id === undefined ? {} : { id }), ...(point === undefined ? {} : { at: point! }) }
      }
      case 'scroll-card':
        return { action, delta: num(source.delta, `${label}.delta`), viewHeight: num(source.viewHeight, `${label}.viewHeight`) }
      case 'open-source': {
        const member = source.member === undefined || source.member === null ? undefined : str(source.member, `${label}.member`)
        return { action, id: str(source.id, `${label}.id`), ...(member === undefined ? {} : { member }) }
      }
      case 'omit':
        return { action, id: parseElementId(source.id, `${label}.id`) }
      case 'new-proposal':
        return source.name === undefined || source.name === null ? { action } : { action, name: parseProposalName(source.name) }
      case 'language': {
        const language = parsePanelLanguage(source.language, `${label}.language`)
        if (language === null) at(`${label}.language`, 'must be a locale tag')
        return { action, language: language! }
      }
      case 'quality-request':
        return { action, request: parseQualityRequestOp(source.request, `${label}.request`),
          id: parseElementId(source.id, `${label}.id`) }
      default:
        return at(`${label}.action`, `unknown panel action ${JSON.stringify(source.action)}`)
    }
  } catch (error) {
    if (error instanceof Invalid) throw new Error(error.message)
    throw error
  }
}

function parseAck(value: unknown, label = 'panel ack'): PanelAck {
  const source = record(value, label)
  const accepted = source.accepted
  if (typeof accepted !== 'boolean') at(`${label}.accepted`, 'must be a boolean')
  const reason = source.reason === null || source.reason === undefined ? null : str(source.reason, `${label}.reason`)
  const ack: PanelAck = { accepted, reason }
  if (source.inspector !== undefined && source.inspector !== null) {
    ack.inspector = validateInspectorModel(source.inspector, `${label}.inspector`)
  }
  if (source.card !== undefined && source.card !== null) ack.card = validateCardModel(source.card, `${label}.card`)
  if (source.source !== undefined && source.source !== null) ack.source = validateSourceModel(source.source, `${label}.source`)
  return ack
}

/** Validate the answer to the panel's explicit close. */
function parseCloseAck(value: unknown, label = 'panel close ack'): PanelCloseAck {
  try {
    const source = record(value, label)
    const closed = source.closed
    if (typeof closed !== 'boolean') at(`${label}.closed`, 'must be a boolean')
    return { closed }
  } catch (error) {
    if (error instanceof Invalid) throw new Error(error.message)
    throw error
  }
}

const codec = (name: string, parse: (value: unknown) => unknown): RemoteCodec => ({
  mode: 'strict',
  typeSymbol: `dsh-uml-viewer#${name}`,
  create: () => ({ parse }),
})

/** The calling agent's session id, resolved by the gateway's `agent` lookup. */
const agentId = (): RemoteCodec => ({
  ...codec('SessionId', value => str(value, 'agentId')),
  // Strict lookup descriptors must use the provider's canonical wire symbol.
  typeSymbol: '@deepseek-ai/dsh-session/types#SessionId',
})

/**
 * The DSH language the panel passes when it opens its owned stream (#44). The
 * gateway calls the host method with the parameters in this order and the
 * cancellation signal last: `watchOwned(agent, owner, language, signal)`.
 */
const language = (): RemoteParameter => ({
  name: 'language', wire: 'language', source: 'json', codec: codec('PanelLanguage', value => parsePanelLanguage(value)),
})

/** One hand-written declaration of the panel's typed Remote. */
export const REMOTE_DESCRIPTORS: readonly RemoteDescriptor[] = Object.freeze([
  ...(['watchOwned', 'closeOwned'] as const).map(method => ({
    id: `dsh-uml-viewer#umlViewer/${method}`,
    service: REMOTE_SERVICE, namespace: REMOTE_SERVICE, method,
    ...(method === 'watchOwned' ? { mode: 'stream' as const, cancellation: { parameter: 'signal' as const } } : {}),
    invocation: { kind: 'direct' as const },
    scope: { context: 'agent' as const, wire: 'agentId' },
    parameters: [
      { name: 'agent', wire: 'agentId', source: 'lookup' as const, lookup: 'agent', codec: agentId() },
      { name: 'owner', wire: 'owner', source: 'json' as const, codec: codec('PanelOwner', parsePanelOwner) },
      ...(method === 'watchOwned' ? [language()] : []),
    ],
    result: method === 'watchOwned' ? codec('PanelEvent', value => parsePanelEvent(value)) : codec('PanelCloseAck', value => parseCloseAck(value)),
  })),
  {
    id: 'dsh-uml-viewer#umlViewer/watch',
    service: REMOTE_SERVICE,
    namespace: REMOTE_SERVICE,
    method: 'watch',
    mode: 'stream',
    invocation: { kind: 'direct' },
    scope: { context: 'agent', wire: 'agentId' },
    parameters: [{ name: 'agent', wire: 'agentId', source: 'lookup', lookup: 'agent', codec: agentId() }],
    cancellation: { parameter: 'signal' },
    result: codec('PanelEvent', value => parsePanelEvent(value)),
  },
  {
    id: 'dsh-uml-viewer#umlViewer/action',
    service: REMOTE_SERVICE,
    namespace: REMOTE_SERVICE,
    method: 'action',
    invocation: { kind: 'direct' },
    scope: { context: 'agent', wire: 'agentId' },
    parameters: [
      { name: 'agent', wire: 'agentId', source: 'lookup', lookup: 'agent', codec: agentId() },
      { name: 'action', wire: 'action', source: 'json', codec: codec('PanelAction', value => parsePanelAction(value)) },
    ],
    result: codec('PanelAck', value => parseAck(value)),
  },
  {
    // Additive to the version-1 panel protocol: the one definite close. A watch
    // stream ending is a carrier loss and is not this call.
    id: 'dsh-uml-viewer#umlViewer/close',
    service: REMOTE_SERVICE,
    namespace: REMOTE_SERVICE,
    method: 'close',
    invocation: { kind: 'direct' },
    scope: { context: 'agent', wire: 'agentId' },
    parameters: [{ name: 'agent', wire: 'agentId', source: 'lookup', lookup: 'agent', codec: agentId() }],
    result: codec('PanelCloseAck', value => parseCloseAck(value)),
  },
])

/** The mountable contribution the browser mount and the host registry both take. */
export const UML_VIEWER_REMOTE = Object.freeze({ package: 'dsh-uml-viewer', descriptors: REMOTE_DESCRIPTORS })
