/**
 * View panel requests (#29): hiding an element, choosing the discussion
 * context and adding a proposal. Like the quality requests (#28) each one is
 * one visible, queued user message in the panel's session; unlike them they
 * need no quality refresh. The companion edits the policy in place and runs
 * `uml_ir`, then `uml_display` (`docs/companion-rules.md`).
 *
 * The requests that regenerate the IR carry a host-issued `:request` id. The
 * host admits `uml_ir({request})` for an id it issued in that session even
 * after a failed quality call, because these requests read no new quality
 * data.
 */
import type { DiscussionContext } from './resident.js'
import type { PanelRequestTarget } from './requests.js'
import { PANEL_REQUEST_HEADER, parseElementId } from './requests.js'

/** A host-issued request id: lowercase hex, as `newRequestId` makes it. */
const REQUEST_ID = /^[0-9a-f]{12}$/

/** The longest `:context` map or proposal name a request quotes, in UTF-8 bytes. */
export const VIEW_REQUEST_LIMITS = Object.freeze({ contextBytes: 4096, nameBytes: 256 })

/**
 * Validate a request id.
 * @param value the id.
 * @param label the field name used in failure messages.
 */
export function parseRequestId(value: unknown, label: string): string {
  if (typeof value !== 'string' || !REQUEST_ID.test(value)) throw new Error(`${label} must be a panel request id (12 lowercase hex digits)`)
  return value
}

/**
 * The `:context` map of a discussion context, checked before it is quoted:
 * one line, an EDN map whose `:context` is the context's own kind.
 * @param context the resident's discussion context.
 * @returns the EDN map text.
 */
export function contextEdn(context: DiscussionContext): string {
  const edn = context.edn
  if (typeof edn !== 'string' || /[\u0000-\u001f\u007f\u2028\u2029]/.test(edn)) throw new Error('discussion context edn must be one line of EDN')
  if (new TextEncoder().encode(edn).length > VIEW_REQUEST_LIMITS.contextBytes) {
    throw new Error(`discussion context edn exceeds ${VIEW_REQUEST_LIMITS.contextBytes} bytes`)
  }
  const kind = context.kind
  if (kind !== 'real' && kind !== 'proposal') throw new Error('discussion context kind must be real or proposal')
  if (!edn.endsWith('}') || !(edn === `{:context :${kind}}` || edn.startsWith(`{:context :${kind} `))) {
    throw new Error(`discussion context edn must be a {:context :${kind} …} map`)
  }
  return edn
}

/** What hiding one element sends. */
export interface OmitRequest {
  target: PanelRequestTarget
  /** The discussion context the element was hidden in. */
  context: DiscussionContext
  request: string
}

function header(body: string): string {
  return `${PANEL_REQUEST_HEADER}\n${body}`
}

/**
 * The visible omit message: the companion adds the id to that context's
 * `:omit` (the proposal's, or the policy's on the source diagram).
 * @param omit the validated parts of the request.
 */
export function renderOmitRequest(omit: OmitRequest): string {
  const id = parseElementId(omit.target.id, 'omit target id')
  const kind = omit.target.kind
  if (kind !== 'class' && kind !== 'component') throw new Error('omit target kind must be class or component')
  const request = parseRequestId(omit.request, 'omit request')
  return header(`{:op :omit :target {:id ${id} :kind :${kind}} :context ${contextEdn(omit.context)} :request "${request}"}`)
}

/**
 * The visible context message: from now on the companion discusses, and
 * edits, the diagram this context names. It changes no file, so it carries
 * no request id.
 * @param context the context the resident switched to.
 */
export function renderContextRequest(context: DiscussionContext): string {
  return header(`{:op :context :context ${contextEdn(context)}}`)
}

/**
 * Validate a proposal name the user typed: trimmed, one line, bounded.
 * @param value the name.
 * @returns the trimmed name.
 */
export function parseProposalName(value: unknown): string {
  if (typeof value !== 'string') throw new Error('proposal name must be a string')
  const name = value.trim()
  if (!name) throw new Error('proposal name must not be empty')
  if (/[\u0000-\u001f\u007f\u2028\u2029]/.test(name)) throw new Error('proposal name must be one line of text')
  if (new TextEncoder().encode(name).length > VIEW_REQUEST_LIMITS.nameBytes) {
    throw new Error(`proposal name exceeds ${VIEW_REQUEST_LIMITS.nameBytes} bytes`)
  }
  return name
}

const pad = (value: number, width = 2): string => String(value).padStart(width, '0')

/**
 * The default proposal name: `%Y-%m-%d %H:%M:%S` in local time, the egui
 * viewer's `clock::proposal_name`.
 * @param date the moment the user asked for the proposal.
 */
export function defaultProposalName(date: Date): string {
  return `${pad(date.getFullYear(), 4)}-${pad(date.getMonth() + 1)}-${pad(date.getDate())} `
    + `${pad(date.getHours())}:${pad(date.getMinutes())}:${pad(date.getSeconds())}`
}

function ednString(text: string): string {
  return `"${text.replaceAll('\\', '\\\\').replaceAll('"', '\\"')}"`
}

/** What adding a proposal sends. */
export interface NewProposalRequest { name: string; request: string }

/**
 * The visible new-proposal message: the companion adds an empty proposal with
 * this name to the policy, in place, and regenerates the IR.
 * @param proposal the validated name and the request id.
 */
export function renderNewProposalRequest(proposal: NewProposalRequest): string {
  const name = parseProposalName(proposal.name)
  const request = parseRequestId(proposal.request, 'new-proposal request')
  return header(`{:op :new-proposal :name ${ednString(name)} :request "${request}"}`)
}
