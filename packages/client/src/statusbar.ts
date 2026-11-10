/**
 * The panel's bottom status bar (#46, ADR 0015): the third section of the
 * skeleton, below the canvas. It shows what used to be painted into the canvas
 * (the waiting / missing-diagram hint, a refusal) as localized DOM text, and it
 * is where the request status of #51 lives: the session's current panel
 * request, as the host's `request-status` list says.
 *
 * The text comes from the locale files, so a DSH language switch relabels the
 * bar in place (#42). The host's UI primitives are never loaded (ADR 0013).
 */
import type { RequestStatusEntry, SessionStatus } from '@dsh-uml-viewer/contracts'
import { ENGLISH_TEXT, type PanelText } from './locale.js'

export const STATUSBAR_STYLE = `
.dsh-uml-viewer-statusbar {
  display: flex; flex: 0 0 auto; align-items: center; gap: 8px; box-sizing: border-box;
  min-height: 22px; padding: 2px 8px; border-top: 1px solid var(--dsw-alias-border-l2);
  background: var(--dsw-alias-bg-layer-2); color: var(--dsw-alias-label-secondary);
  font: 12px/1.4 var(--dsw-font-family, system-ui);
}
.dsh-uml-viewer-status-text { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.dsh-uml-viewer-status-text[hidden] { display: none; }
.dsh-uml-viewer-statusbar[data-tone="error"] .dsh-uml-viewer-status-text { color: var(--dsw-alias-state-error-primary); }
.dsh-uml-viewer-request { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.dsh-uml-viewer-request[hidden] { display: none; }
.dsh-uml-viewer-request[data-state="failed"] { color: var(--dsw-alias-state-error-primary); }
`

export interface StatusBarOptions {
  text?: PanelText
}

export interface StatusBar {
  readonly element: HTMLElement
  /** The canvas status code (#45): waiting for a display, or a missing diagram. */
  setStatus(code: SessionStatus | null): void
  /** The panel's own line (a refused element request); it wins over the code. */
  setMessage(message: string | (() => string) | null): void
  /** The session's request-status list (#51), oldest first; the bar shows the current one. */
  setRequests(requests: readonly RequestStatusEntry[]): void
  dispose(): void
}

/** The localized text of a canvas status code (#45). */
export function statusCodeText(text: PanelText, code: SessionStatus | null): string {
  if (code === null) return ''
  return code.code === 'missing-file'
    ? text.t('status.missingFile', { path: code.path })
    : text.t('status.waitingDisplay')
}

/**
 * The request the bar shows (#51). The one that changed last — added, or
 * moved to another state — is what the user just caused or is waiting on, so
 * a refusal right after a click is seen even behind an older queued request.
 * With nothing changed it keeps the one shown; with no history (a stream just
 * opened, as after a page refresh) it is the oldest request still queued or in
 * progress — the one the companion handles first — else the newest one.
 * @param previous the list before this update; null when there is none.
 * @param next the list now.
 * @param shown the id shown before this update.
 */
export function currentRequest(previous: readonly RequestStatusEntry[] | null, next: readonly RequestStatusEntry[],
  shown: string | null): RequestStatusEntry | null {
  if (previous !== null) {
    const before = new Map(previous.map(entry => [entry.id, entry.state]))
    const changed = next.filter(entry => before.get(entry.id) !== entry.state)
    if (changed.length) return changed.at(-1)!
    const kept = next.find(entry => entry.id === shown)
    if (kept) return kept
  }
  return next.find(entry => entry.state === 'queued' || entry.state === 'running') ?? next.at(-1) ?? null
}

/** The localized line of one request: what it asks, of which element, and how far it got. */
export function requestText(text: PanelText, entry: RequestStatusEntry): string {
  const op = text.t(`request.op.${entry.op}` as const)
  const state = text.t(`request.state.${entry.state}` as const)
  const line = entry.target
    ? text.t('request.line', { op, target: entry.target.id, state })
    : text.t('request.line.untargeted', { op, state })
  if (entry.state !== 'failed' || !entry.reason) return line
  return entry.exitCode === null
    ? text.t('request.failure', { line, reason: entry.reason })
    : text.t('request.failure.exitCode', { line, reason: entry.reason, code: String(entry.exitCode) })
}

/** Attach the status bar to its section of the panel. */
export function mountStatusBar(host: HTMLElement, options: StatusBarOptions = {}): StatusBar {
  const text = options.text ?? ENGLISH_TEXT
  const doc = host.ownerDocument
  const element = doc.createElement('div')
  element.className = 'dsh-uml-viewer-statusbar'
  element.setAttribute('role', 'status')
  element.setAttribute('aria-live', 'polite')
  const line = doc.createElement('span')
  line.className = 'dsh-uml-viewer-status-text'
  element.appendChild(line)
  const request = doc.createElement('span')
  request.className = 'dsh-uml-viewer-request'
  element.appendChild(request)
  let code: SessionStatus | null = null
  let message: string | (() => string) | null = null
  let requests: readonly RequestStatusEntry[] = []
  let current: RequestStatusEntry | null = null
  const render = (): void => {
    request.textContent = current ? requestText(text, current) : ''
    request.hidden = current === null
    request.dataset.state = current?.state ?? ''
    element.dataset.request = current?.state ?? ''
    const said = typeof message === 'function' ? message() : message
    // A refused click is reported by the element menu and, with the same
    // reason, by its failed request status: the bar says it once.
    const reason = said !== null && current?.state === 'failed' && current.reason === said ? null : said
    const value = reason ?? statusCodeText(text, code)
    line.textContent = value
    // The bar is the panel's third section and always there (#46); only its
    // text comes and goes.
    line.hidden = value === ''
    element.dataset.code = code?.code ?? ''
    element.dataset.tone = reason ? 'error' : code === null ? 'none' : 'info'
  }
  const label = (): void => {
    element.setAttribute('aria-label', text.t('statusbar.label'))
    request.setAttribute('aria-label', text.t('request.label'))
  }
  label()
  render()
  const unsubscribe = text.subscribe(() => {
    label()
    render()
  })
  host.appendChild(element)
  return {
    element,
    setStatus(next) {
      code = next
      render()
    },
    setMessage(next) {
      message = next
      render()
    },
    setRequests(next) {
      // An empty list is no history: the next list is read as a fresh one.
      current = currentRequest(requests.length ? requests : null, next, current?.id ?? null)
      requests = next
      render()
    },
    dispose() {
      unsubscribe()
      element.remove()
    },
  }
}
