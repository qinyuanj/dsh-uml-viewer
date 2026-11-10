/**
 * Quality panel requests (#28): the right-click refreshes a panel delivers to
 * its bound session as one visible, queued user message.
 *
 * The panel names only the element (`quality-request` with an EDN id); the
 * resident resolves what that element covers from the diagram it has loaded
 * (`quality-target`, `docs/resident-protocol.md`), the host validates and
 * queues that answer, so no client input ever becomes a file path and no
 * TypeScript re-derives the scene's grouping.
 */
import { normalizeRelativePath, parseElementId } from './tools.js'

export type QualityRequestOp = 'refresh-crap' | 'refresh-mutate' | 'refresh-mutate-all'

/** The element menu's quality entries, in menu order. */
export const QUALITY_REQUESTS: readonly QualityRequestOp[] = Object.freeze(['refresh-crap', 'refresh-mutate', 'refresh-mutate-all'])

/** What the request is about: one class, or a namespace component (package). */
export interface PanelRequestTarget { id: string; kind: 'class' | 'component' }

/** The structured request in the delivered message. */
export interface PanelRequest {
  op: QualityRequestOp
  target: PanelRequestTarget
  /** Project-relative source files of the target, sorted by path, each once. */
  files: string[]
}

/**
 * One resolved quality target: the resident's `quality-target` answer, which
 * is also the target part of the delivered request.
 */
export interface QualityTarget {
  id: string
  kind: PanelRequestTarget['kind']
  /** Project-relative source files, sorted by UTF-8 path bytes, each once. */
  files: string[]
}

/**
 * The producer's bounds (`viewer_session::targets`, `QUALITY_TARGET_*`). An
 * answer past them is refused, never silently truncated: the numbers must
 * stay equal to the Rust constants.
 */
export const QUALITY_TARGET_LIMITS = Object.freeze({
  files: 4096,
  pathBytes: 4096,
  eventBytes: 1024 * 1024,
})

/** The first line of every panel request message; the EDN request follows. */
export const PANEL_REQUEST_HEADER = 'UML viewer panel request:'

/**
 * Validate the request op.
 * @param value the op name without its leading colon.
 * @param label the field name used in failure messages.
 */
export function parseQualityRequestOp(value: unknown, label: string): QualityRequestOp {
  if (typeof value !== 'string' || !(QUALITY_REQUESTS as readonly string[]).includes(value)) {
    throw new Error(`${label} must be one of ${QUALITY_REQUESTS.join(', ')}`)
  }
  return value as QualityRequestOp
}

/**
 * Compare two paths by their UTF-8 bytes: Rust's `String` order, which is how
 * the resident sorts its answer.
 * @param a the first path's bytes.
 * @param b the second path's bytes.
 */
function byteOrder(a: Uint8Array, b: Uint8Array): number {
  const shared = Math.min(a.length, b.length)
  for (let index = 0; index < shared; index += 1) {
    const difference = a[index]! - b[index]!
    if (difference !== 0) return difference
  }
  return a.length - b.length
}

/**
 * Validate one resolved target (`quality-target`) before anything is queued.
 * Bounded like the producer, deduplicated and in the producer's order: an
 * answer that is not is a refusal, never a reordered or truncated message.
 * @param value the resident's target, or the target part of a request.
 * @param label the document name used in failure messages.
 * @param expectedId the id the caller asked about, when there is one.
 * @returns the same target, narrowed.
 */
export function validateQualityTarget(value: unknown, label = 'quality target', expectedId?: string): QualityTarget {
  if (!value || typeof value !== 'object' || Array.isArray(value)) throw new Error(`${label} must be an object`)
  const source = value as Record<string, unknown>
  const id = parseElementId(source.id, `${label} id`)
  const encoder = new TextEncoder()
  if (encoder.encode(id).length > QUALITY_TARGET_LIMITS.pathBytes) throw new Error(`${label} id exceeds ${QUALITY_TARGET_LIMITS.pathBytes} bytes`)
  if (expectedId !== undefined && id !== expectedId) {
    throw new Error(`${label} id ${id} does not match the requested ${expectedId}`)
  }
  const kind = source.kind
  if (kind !== 'class' && kind !== 'component') {
    throw new Error(`${label} kind must be class or component`)
  }
  if (!Array.isArray(source.files) || source.files.length === 0) {
    throw new Error(`${label} files must be a nonempty array`)
  }
  if (source.files.length > QUALITY_TARGET_LIMITS.files) {
    throw new Error(`${label} files has ${source.files.length} entries; the limit is ${QUALITY_TARGET_LIMITS.files}`)
  }
  const files = source.files.map((file, index) => normalizeRelativePath(`${label} files[${index}]`, file))
  let previous: Uint8Array | null = null
  files.forEach((file, index) => {
    // Everything but the separator spelling is `normalizeRelativePath`'s rule
    // (#review M1a): no second, weaker validator lives here.
    if (file.includes('\\')) throw new Error(`${label} files[${index}] must use forward slashes`)
    if (file.startsWith('-')) throw new Error(`${label} files[${index}] must not be a command option`)
    const bytes = encoder.encode(file)
    if (bytes.length > QUALITY_TARGET_LIMITS.pathBytes) {
      throw new Error(`${label} files[${index}] is ${bytes.length} bytes; the limit is ${QUALITY_TARGET_LIMITS.pathBytes}`)
    }
    if (previous) {
      const order = byteOrder(previous, bytes)
      if (order === 0) throw new Error(`${label} files has a duplicate: ${file}`)
      if (order > 0) throw new Error(`${label} files must be in ascending path order: ${files[index - 1]} then ${file}`)
    }
    previous = bytes
  })
  const target: QualityTarget = { id, kind, files }
  if (encoder.encode(JSON.stringify(target)).length > QUALITY_TARGET_LIMITS.eventBytes - 256) {
    throw new Error(`${label} exceeds ${QUALITY_TARGET_LIMITS.eventBytes - 256} payload bytes`)
  }
  return target
}

/**
 * Validate a structured request before it is delivered.
 * @param value the request.
 * @returns the same request, narrowed.
 */
export function validatePanelRequest(value: unknown): PanelRequest {
  if (!value || typeof value !== 'object' || Array.isArray(value)) throw new Error('panel request must be an object')
  const source = value as Record<string, unknown>
  const op = parseQualityRequestOp(source.op, 'panel request op')
  const target = source.target as Record<string, unknown> | null | undefined
  if (!target || typeof target !== 'object' || Array.isArray(target)) throw new Error('panel request target must be an object')
  const valid = validateQualityTarget({ id: target.id, kind: target.kind, files: source.files }, 'panel request target')
  return { op, target: { id: valid.id, kind: valid.kind }, files: valid.files }
}

function ednString(text: string): string {
  return `"${text.replaceAll('\\', '\\\\').replaceAll('"', '\\"')}"`
}

/**
 * The visible message text: a header line, then the EDN request
 * `{:op … :target {:id … :kind …} :files [...]}` the companion rules name.
 * @param request a request; it is validated first.
 */
export function renderPanelRequest(request: PanelRequest): string {
  const valid = validatePanelRequest(request)
  const files = valid.files.map(ednString).join(' ')
  return `${PANEL_REQUEST_HEADER}\n{:op :${valid.op} :target {:id ${valid.target.id} :kind :${valid.target.kind}} :files [${files}]}`
}
