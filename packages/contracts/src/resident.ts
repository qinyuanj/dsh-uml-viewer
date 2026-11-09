/**
 * Validation for the resident protocol (`docs/resident-protocol.md`), schema
 * `dsh-uml-viewer/resident`, version 1.
 *
 * The Rust child owns the wire fields; this module only checks the lines it
 * writes. Unknown events are refused (the host would not know what to do with
 * them) while frames inside `frame` events keep their own additive contract.
 */
import type { SceneFrame } from './scene-frame.js'
import { validateSceneFrame } from './scene-frame.js'
import type { CardModel, InspectorModel, SourceModel } from './overlays.js'
import { validateCardModel, validateInspectorModel, validateSourceModel } from './overlays.js'
import type { QualityTarget } from './requests.js'
import { validateQualityTarget } from './requests.js'

export const RESIDENT_SCHEMA = 'dsh-uml-viewer/resident'
export const RESIDENT_VERSION = 1
export const SESSION_STATE_SCHEMA = 'dsh-uml-viewer/session-state'
export const SESSION_STATE_VERSION = 1

export interface DiscussionContext {
  kind: 'real' | 'proposal'
  edn: string
  name?: string
  proposalId?: string
}

export interface SearchResult { id: string; name: string; ns: string; label: string }

export interface SessionState {
  schema: typeof SESSION_STATE_SCHEMA
  version: typeof SESSION_STATE_VERSION
  diagram: string | null
  /** Additive v1 metadata: hash of the exact bytes parsed by this resident. */
  loadedDiagram?: { version: 1; sha256: string } | null
  context: DiscussionContext | null
  focus: string[]
  proposalId: string | null
  openLayer: string | null
  declutter: string | null
  camera: { x: number; y: number; zoom: number }
  selected: string | null
  detail: string | null
}

export type ResidentEvent =
  | { version: 1; seq: unknown; event: 'ready'; schema: typeof RESIDENT_SCHEMA; frameSchema: string; frameVersion: number
      stateSchema: string; stateVersion: number; actions: string[] }
  | { version: 1; seq: unknown; event: 'frame'; frame: SceneFrame }
  | { version: 1; seq: unknown; event: 'search'; query: string; results: SearchResult[] }
  | { version: 1; seq: unknown; event: 'context'; context: DiscussionContext }
  | { version: 1; seq: unknown; event: 'state'; state: SessionState }
  | { version: 1; seq: unknown; event: 'inspector'; inspector: InspectorModel }
  | { version: 1; seq: unknown; event: 'card'; card: CardModel }
  | { version: 1; seq: unknown; event: 'source'; source: SourceModel }
  | { version: 1; seq: unknown; event: 'quality-target'; target: QualityTarget }
  | { version: 1; seq: unknown; event: 'ack' }
  | { version: 1; seq: unknown; event: 'error'; message: string }

const EVENTS: ReadonlySet<string> = new Set(['ready', 'frame', 'search', 'context', 'state', 'ack', 'error',
  // #27 overlays: additive, version 1 is unchanged.
  'inspector', 'card', 'source',
  // #28 panel request targets: additive, version 1 is unchanged.
  'quality-target'])

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

function list(value: unknown, label: string): unknown[] {
  if (!Array.isArray(value)) at(label, 'must be an array')
  return value
}

function context(value: unknown, label: string): DiscussionContext {
  const source = record(value, label)
  const kind = str(source.kind, `${label}.kind`)
  if (kind !== 'real' && kind !== 'proposal') at(`${label}.kind`, 'must be "real" or "proposal"')
  const parsed: DiscussionContext = { kind, edn: str(source.edn, `${label}.edn`) }
  if (source.name !== undefined && source.name !== null) parsed.name = str(source.name, `${label}.name`)
  if (source.proposalId !== undefined && source.proposalId !== null) parsed.proposalId = str(source.proposalId, `${label}.proposalId`)
  return parsed
}

function optionalString(value: unknown, label: string): string | null {
  return value === null || value === undefined ? null : str(value, label)
}

function loadedDiagram(value: unknown, label: string): { version: 1; sha256: string } | null {
  if (value === null) return null
  const source = record(value, label)
  if (source.version !== 1) at(`${label}.version`, 'must be 1')
  const sha256 = str(source.sha256, `${label}.sha256`)
  if (!/^[a-f0-9]{64}$/.test(sha256)) at(`${label}.sha256`, 'must be a lowercase SHA-256 hex digest')
  return { version: 1, sha256 }
}

/**
 * Validate the persisted view/discussion state the panel restores.
 * @param value the state the host stored.
 * @param label the document name used in failure messages.
 * @returns the same state, narrowed.
 */
export function validateSessionState(value: unknown, label = 'session state'): SessionState {
  try {
    const source = record(value, label)
    if (source.schema !== SESSION_STATE_SCHEMA) at(`${label}.schema`, `must be ${JSON.stringify(SESSION_STATE_SCHEMA)}`)
    if (source.version !== SESSION_STATE_VERSION) at(`${label}.version`, `must be ${SESSION_STATE_VERSION}`)
    const camera = record(source.camera, `${label}.camera`)
    return {
      schema: SESSION_STATE_SCHEMA,
      version: SESSION_STATE_VERSION,
      diagram: optionalString(source.diagram, `${label}.diagram`),
      ...(source.loadedDiagram === undefined ? {} : { loadedDiagram: loadedDiagram(source.loadedDiagram, `${label}.loadedDiagram`) }),
      context: source.context === null || source.context === undefined ? null : context(source.context, `${label}.context`),
      focus: list(source.focus, `${label}.focus`).map((entry, index) => str(entry, `${label}.focus[${index}]`)),
      proposalId: optionalString(source.proposalId, `${label}.proposalId`),
      openLayer: optionalString(source.openLayer, `${label}.openLayer`),
      declutter: optionalString(source.declutter, `${label}.declutter`),
      camera: { x: num(camera.x, `${label}.camera.x`), y: num(camera.y, `${label}.camera.y`), zoom: num(camera.zoom, `${label}.camera.zoom`) },
      selected: optionalString(source.selected, `${label}.selected`),
      detail: optionalString(source.detail, `${label}.detail`),
    }
  } catch (error) {
    if (error instanceof Invalid) throw new Error(error.message)
    throw error
  }
}

/**
 * Validate one resident event line value.
 * @param value the parsed JSON of one stdout line.
 * @param label the document name used in failure messages.
 * @returns the same event, narrowed.
 */
export function validateResidentEvent(value: unknown, label = 'resident event'): ResidentEvent {
  try {
    const source = record(value, label)
    if (source.version !== RESIDENT_VERSION) at(`${label}.version`, `must be ${RESIDENT_VERSION}`)
    const event = str(source.event, `${label}.event`)
    if (!EVENTS.has(event)) at(`${label}.event`, `unknown event ${JSON.stringify(event)}`)
    const seq = source.seq ?? null
    switch (event) {
      case 'ready':
        if (source.schema !== RESIDENT_SCHEMA) at(`${label}.schema`, `must be ${JSON.stringify(RESIDENT_SCHEMA)}`)
        return { version: RESIDENT_VERSION, seq, event,
          schema: RESIDENT_SCHEMA,
          frameSchema: str(source.frameSchema, `${label}.frameSchema`),
          frameVersion: num(source.frameVersion, `${label}.frameVersion`),
          stateSchema: str(source.stateSchema, `${label}.stateSchema`),
          stateVersion: num(source.stateVersion, `${label}.stateVersion`),
          actions: list(source.actions, `${label}.actions`).map((entry, index) => str(entry, `${label}.actions[${index}]`)) }
      case 'frame':
        return { version: RESIDENT_VERSION, seq, event, frame: validateSceneFrame(source.frame, `${label}.frame`) }
      case 'search':
        return { version: RESIDENT_VERSION, seq, event, query: str(source.query, `${label}.query`),
          results: list(source.results, `${label}.results`).map((entry, index) => {
            const row = record(entry, `${label}.results[${index}]`)
            return { id: str(row.id, `${label}.results[${index}].id`), name: str(row.name, `${label}.results[${index}].name`),
              ns: str(row.ns, `${label}.results[${index}].ns`), label: str(row.label, `${label}.results[${index}].label`) }
          }) }
      case 'context':
        return { version: RESIDENT_VERSION, seq, event, context: context(source.context, `${label}.context`) }
      case 'state':
        return { version: RESIDENT_VERSION, seq, event, state: validateSessionState(source.state, `${label}.state`) }
      case 'inspector':
        return { version: RESIDENT_VERSION, seq, event, inspector: validateInspectorModel(source.inspector, `${label}.inspector`) }
      case 'card':
        return { version: RESIDENT_VERSION, seq, event, card: validateCardModel(source.card, `${label}.card`) }
      case 'source':
        return { version: RESIDENT_VERSION, seq, event, source: validateSourceModel(source.source, `${label}.source`) }
      case 'quality-target':
        return { version: RESIDENT_VERSION, seq, event, target: validateQualityTarget(source.target, `${label}.target`) }
      case 'ack':
        return { version: RESIDENT_VERSION, seq, event }
      default:
        return { version: RESIDENT_VERSION, seq, event: 'error', message: str(source.message, `${label}.message`) }
    }
  } catch (error) {
    if (error instanceof Invalid) throw new Error(error.message)
    throw error
  }
}

/**
 * Parse one complete stdout line (no trailing newline) into a resident event.
 * @param line one stdout line.
 * @returns the validated event.
 * @throws {Error} when the line is not one JSON object.
 */
export function parseResidentLine(line: string): ResidentEvent {
  if (line.includes('\n') || line.includes('\r')) throw new Error('resident line must not contain a line break')
  let value: unknown
  try {
    value = JSON.parse(line)
  } catch {
    throw new Error(`resident stdout line is not JSON: ${line.slice(0, 80)}`)
  }
  return validateResidentEvent(value)
}

/**
 * Serialize one action as the single line the child reads.
 * @param action the action name and its fields.
 * @param seq the host's correlation value.
 * @returns the line, without a trailing newline.
 */
export function residentAction(action: { action: string } & Record<string, unknown>, seq: number): string {
  return JSON.stringify({ version: RESIDENT_VERSION, seq, ...action })
}
