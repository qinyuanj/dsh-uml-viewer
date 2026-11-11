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
import type { CardModel, SourceModel } from './overlays.js'
import { validateCardModel, validateSourceModel } from './overlays.js'
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

/** One level of the tree path the panel's top bar shows (#45). */
export interface BreadcrumbLevel {
  /** The component at this level; `null` is the source diagram's root. */
  id: string | null
  /** The level's name as the tree gives it: a title, a segment, a label. */
  name: string
}

/**
 * The canvas status as a code the panel localizes (#45): the resident names
 * the state, the panel's language files own the text. A code carries exactly
 * the parameters its text needs.
 */
export type SessionStatus =
  | { code: 'waiting-display' }
  | { code: 'missing-file'; path: string }

/** The status codes this version knows; anything else is a broken resident. */
export const STATUS_CODES: readonly SessionStatus['code'][] = ['waiting-display', 'missing-file']

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
  | { version: 1; seq: unknown; event: 'state'; state: SessionState
      breadcrumbs: BreadcrumbLevel[]; status: SessionStatus | null; proposals: ProposalEntry[] }
  /** `card` is null when no class card is open (#47). */
  | { version: 1; seq: unknown; event: 'card'; card: CardModel | null }
  | { version: 1; seq: unknown; event: 'source'; source: SourceModel }
  | { version: 1; seq: unknown; event: 'quality-target'; target: QualityTarget }
  | { version: 1; seq: unknown; event: 'ack' }
  | { version: 1; seq: unknown; event: 'error'; message: string }

const EVENTS: ReadonlySet<string> = new Set(['ready', 'frame', 'search', 'context', 'state', 'ack', 'error',
  // #27 overlays: additive, version 1 is unchanged. #47 deleted `inspector`.
  'card', 'source',
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
 * One proposal of the shown diagram, as the discussion-context switcher lists
 * it (#47): its EDN id and its display name.
 */
export interface ProposalEntry { id: string; name: string }

/**
 * Validate the proposal list a state event carries (#47), in the diagram's
 * order. Each entry is a non-empty EDN id and a non-empty name.
 * @param value the event's `proposals` (absent from a resident before #47).
 * @param label the document name used in failure messages.
 * @returns the proposals, or none.
 */
export function validateProposalEntries(value: unknown, label = 'proposals'): ProposalEntry[] {
  if (value === null || value === undefined) return []
  try {
    return list(value, label).map((entry, index) => {
      const row = record(entry, `${label}[${index}]`)
      const id = str(row.id, `${label}[${index}].id`)
      if (!id) at(`${label}[${index}].id`, 'must not be empty')
      const name = str(row.name, `${label}[${index}].name`)
      if (!name) at(`${label}[${index}].name`, 'must not be empty')
      return { id, name }
    })
  } catch (error) {
    if (error instanceof Invalid) throw new Error(error.message)
    throw error
  }
}

/**
 * Validate the breadcrumb path a state event carries (#45): root first, each
 * level a component id (or null for the source diagram's root) and a name.
 * @param value the event's `breadcrumbs` (absent from a resident before #45).
 * @param label the document name used in failure messages.
 * @returns the levels, root first.
 */
export function validateBreadcrumbs(value: unknown, label = 'breadcrumbs'): BreadcrumbLevel[] {
  if (value === null || value === undefined) return []
  try {
    return list(value, label).map((entry, index) => {
      const row = record(entry, `${label}[${index}]`)
      if (row.id === undefined) at(`${label}[${index}].id`, 'must be an EDN id or null')
      const id = row.id === null ? null : str(row.id, `${label}[${index}].id`)
      const name = str(row.name, `${label}[${index}].name`)
      if (!name) at(`${label}[${index}].name`, 'must not be empty')
      return { id, name }
    })
  } catch (error) {
    if (error instanceof Invalid) throw new Error(error.message)
    throw error
  }
}

/**
 * Validate the canvas status code a state event carries (#45). The panel
 * localizes the code, so an unknown one is a contract failure, not a message
 * to show; its parameters are the ones that code declares.
 * @param value the event's `status` (absent from a resident before #45).
 * @param label the document name used in failure messages.
 * @returns the code, or null when there is nothing to say.
 */
export function validateSessionStatus(value: unknown, label = 'status'): SessionStatus | null {
  if (value === null || value === undefined) return null
  try {
    const source = record(value, label)
    const code = str(source.code, `${label}.code`)
    if (!(STATUS_CODES as readonly string[]).includes(code)) {
      at(`${label}.code`, `unknown status code ${JSON.stringify(code)}`)
    }
    const allowed = code === 'missing-file' ? ['path'] : []
    for (const key of Object.keys(source)) {
      if (key !== 'code' && !allowed.includes(key)) at(`${label}.${key}`, `not a parameter of ${code}`)
    }
    return code === 'missing-file'
      ? { code: 'missing-file', path: str(source.path, `${label}.path`) }
      : { code: 'waiting-display' }
  } catch (error) {
    if (error instanceof Invalid) throw new Error(error.message)
    throw error
  }
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
        // #45: the breadcrumb path and the status code ride on this event;
        // the persisted session state keeps its own contract untouched.
        return { version: RESIDENT_VERSION, seq, event, state: validateSessionState(source.state, `${label}.state`),
          breadcrumbs: validateBreadcrumbs(source.breadcrumbs, `${label}.breadcrumbs`),
          status: validateSessionStatus(source.status, `${label}.status`),
          proposals: validateProposalEntries(source.proposals, `${label}.proposals`) }
      case 'card':
        return { version: RESIDENT_VERSION, seq, event,
          card: source.card === null ? null : validateCardModel(source.card, `${label}.card`) }
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
