/**
 * Quality panel requests (#28): the right-click refreshes a panel delivers to
 * its bound session as one visible, queued user message.
 *
 * The panel names only the element (`quality-request` with an EDN id); the
 * host resolves its source files and renders the structured request below, so
 * no client input ever becomes a file path.
 */
import { normalizeRelativePath } from './tools.js'

export type QualityRequestOp = 'refresh-crap' | 'refresh-mutate' | 'refresh-mutate-all'

/** The element menu's quality entries, in menu order. */
export const QUALITY_REQUESTS: readonly QualityRequestOp[] = Object.freeze(['refresh-crap', 'refresh-mutate', 'refresh-mutate-all'])

/** What the request is about: one class, or a namespace component (package). */
export interface PanelRequestTarget { id: string; kind: 'class' | 'component' }

/** The structured request in the delivered message. */
export interface PanelRequest {
  op: QualityRequestOp
  target: PanelRequestTarget
  /** Project-relative source files of the target, in diagram order, each once. */
  files: string[]
}

/** The first line of every panel request message; the EDN request follows. */
export const PANEL_REQUEST_HEADER = 'UML viewer panel request:'

/** A scene/IR id: one EDN keyword without whitespace or EDN delimiters. */
const KEYWORD = /^:[^\s,()[\]{}"';@^`~\\:][^\s,()[\]{}"';@^`~\\]*$/

/**
 * Validate an element id the panel sends or the request carries.
 * @param value the id.
 * @param label the field name used in failure messages.
 * @returns the same id.
 */
export function parseElementId(value: unknown, label: string): string {
  if (typeof value !== 'string') throw new Error(`${label} must be a string`)
  if (!KEYWORD.test(value)) throw new Error(`${label} must be one EDN keyword id such as :core.order`)
  return value
}

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
  const id = parseElementId(target.id, 'panel request target id')
  if (target.kind !== 'class' && target.kind !== 'component') throw new Error('panel request target kind must be class or component')
  if (!Array.isArray(source.files) || source.files.length === 0) throw new Error('panel request files must be a nonempty array')
  const seen = new Set<string>()
  const files = source.files.map((file, index) => {
    const normalized = normalizeRelativePath(`panel request files[${index}]`, file)
    if (seen.has(normalized)) throw new Error(`panel request files has a duplicate: ${normalized}`)
    seen.add(normalized)
    return normalized
  })
  return { op, target: { id, kind: target.kind }, files }
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
