/**
 * The panel's host↔client seam: one stream of scene/display events and one
 * action call, plus the events and actions themselves.
 *
 * The descriptors are hand-written Typert `InvocationDescriptor`s (no
 * generator in this package). They are plain data with `create(): { parse }`
 * codecs, so the host and the browser bundle share one declaration.
 */
import type { DiscussionContext, SearchResult, SessionState } from './resident.js'
import type { SceneFrame } from './scene-frame.js'
import { validateSceneFrame } from './scene-frame.js'
import { validateSessionState } from './resident.js'
import { normalizeRelativePath } from './tools.js'

/** The Remote namespace the panel mounts. */
export const REMOTE_SERVICE = 'umlViewer'

/** Where the host serves the packaged Sarasa face (a real `font/ttf` route). */
export const PANEL_FONT_PATH = '/dsh-uml-viewer/font/SarasaMonoSC-Regular.ttf'

export type PanelEvent =
  | { kind: 'ready'; actions: string[] }
  | { kind: 'frame'; seq: number | null; frame: SceneFrame }
  | { kind: 'context'; context: DiscussionContext }
  | { kind: 'state'; state: SessionState }
  | { kind: 'search'; query: string; results: SearchResult[] }
  | { kind: 'refused'; reason: string }
  | { kind: 'failed'; reason: string }

export type PanelAction =
  | { action: 'viewport'; viewport: { width: number; height: number } }
  | { action: 'camera'; x: number; y: number; zoom: number }
  | { action: 'display'; path?: string }
  | { action: 'reload' } | { action: 'back' } | { action: 'namespace-tree' } | { action: 'declutter' } | { action: 'save' }
  | { action: 'drill-in'; id?: string; at?: [number, number] }
  | { action: 'select'; at: [number, number] | null }
  | { action: 'search'; query: string }
  | { action: 'locate'; id: string }
  | { action: 'context'; proposalId: string | null }

/** One action's outcome; the events it caused arrive on the watch stream. */
export interface PanelAck { accepted: boolean; reason: string | null }

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
        return { kind, context: { kind: contextKind, edn: str(context.edn, `${label}.context.edn`) } }
      }
      case 'state':
        return { kind, state: validateSessionState(source.state, `${label}.state`) }
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
      case 'reload': case 'back': case 'namespace-tree': case 'declutter': case 'save':
        return { action }
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
  return { accepted, reason }
}

const codec = (name: string, parse: (value: unknown) => unknown): RemoteCodec => ({
  mode: 'strict',
  typeSymbol: `dsh-uml-viewer#${name}`,
  create: () => ({ parse }),
})

/** The calling agent's session id, resolved by the gateway's `agent` lookup. */
const agentId = (): RemoteCodec => codec('SessionId', value => str(value, 'agentId'))

/** One hand-written declaration of the panel's typed Remote. */
export const REMOTE_DESCRIPTORS: readonly RemoteDescriptor[] = Object.freeze([
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
])

/** The mountable contribution the browser mount and the host registry both take. */
export const UML_VIEWER_REMOTE = Object.freeze({ package: 'dsh-uml-viewer', descriptors: REMOTE_DESCRIPTORS })
