/**
 * The panel's bottom status bar (#46, ADR 0015): the third section of the
 * skeleton, below the canvas. It shows what used to be painted into the canvas
 * (the waiting / missing-diagram hint, a refusal) as localized DOM text, and it
 * is where the request status lives: the session's current panel request, how
 * many wait behind it (#53), and the whole queue on a click, as the host's
 * `request-status` list says.
 *
 * Completed entries fade on the client — ten seconds after they finish, or as
 * soon as the next request starts. Failed ones stay in the queue until the user
 * dismisses them, which is a panel action the host answers by dropping the
 * entry from the session's list.
 *
 * The text comes from the locale files, so a DSH language switch relabels the
 * bar in place (#42). The host's UI primitives are never loaded (ADR 0013).
 */
import type { RequestStatusEntry, SessionStatus } from '@dsh-uml-viewer/contracts'
import { ENGLISH_TEXT, type PanelText } from './locale.js'

/** How long a completed entry stays on the bar before the client fades it (#53). */
export const DONE_FADE_MS = 10_000

export const STATUSBAR_STYLE = `
.dsh-uml-viewer-statusbar {
  position: relative; display: flex; flex: 0 0 auto; align-items: center; gap: 8px;
  box-sizing: border-box; min-height: 22px; padding: 2px 8px;
  border-top: 1px solid var(--dsw-alias-border-l2);
  background: var(--dsw-alias-bg-layer-2); color: var(--dsw-alias-label-secondary);
  font: 12px/1.4 var(--dsw-font-family, system-ui);
}
.dsh-uml-viewer-status-text { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.dsh-uml-viewer-status-text[hidden] { display: none; }
.dsh-uml-viewer-statusbar[data-tone="error"] .dsh-uml-viewer-status-text { color: var(--dsw-alias-state-error-primary); }
.dsh-uml-viewer-request { flex: 0 1 auto; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.dsh-uml-viewer-request[hidden] { display: none; }
.dsh-uml-viewer-request[data-state="failed"] { color: var(--dsw-alias-state-error-primary); }
.dsh-uml-viewer-request-more {
  flex: 0 0 auto; padding: 0 4px; border: 1px solid var(--dsw-alias-border-l2); border-radius: 4px;
  background: transparent; color: inherit; font: inherit; cursor: pointer;
}
.dsh-uml-viewer-request-more[hidden] { display: none; }
.dsh-uml-viewer-request-more:hover { background: var(--dsw-alias-interactive-bg-hover); }
.dsh-uml-viewer-request-more:focus-visible { outline: 1px solid var(--dsw-focus-ring-color); }
/* The queue opens over the canvas's bottom edge, where the selection action bar
   (#48) sits: it paints above the bar and the drawer (#49), which are transient
   surfaces of the canvas, so neither hides the queue it is opened from. */
.dsh-uml-viewer-request-list {
  position: absolute; left: 4px; right: 4px; bottom: 100%; z-index: 50; max-height: 180px;
  overflow-x: hidden; overflow-y: auto; box-sizing: border-box; padding: 4px; margin-bottom: 2px;
  border: 1px solid var(--dsw-alias-border-l2); border-radius: 6px;
  background: var(--dsw-alias-bg-layer-2); box-shadow: 0 4px 12px rgb(0 0 0 / 18%);
}
.dsh-uml-viewer-request-item {
  display: flex; align-items: center; gap: 6px; padding: 1px 2px; white-space: nowrap;
}
/* The line shrinks to leave the dismissal in view; a long reason is cut, not
   pushed off the panel edge. */
.dsh-uml-viewer-request-item-text { flex: 1 1 auto; min-width: 0; overflow: hidden; text-overflow: ellipsis; }
.dsh-uml-viewer-request-item[data-state="failed"] { color: var(--dsw-alias-state-error-primary); }
.dsh-uml-viewer-request-dismiss {
  flex: 0 0 auto; margin-left: auto; padding: 0 4px; border: 0; border-radius: 4px;
  background: transparent; color: inherit; font: inherit; cursor: pointer;
}
.dsh-uml-viewer-request-dismiss:hover { background: var(--dsw-alias-interactive-bg-hover); }
.dsh-uml-viewer-request-dismiss:focus-visible { outline: 1px solid var(--dsw-focus-ring-color); }
`

export interface StatusBarOptions {
  text?: PanelText
  /** The user dismissed a finished request (#53); the host drops it from the list. */
  onDismiss?: (id: string) => void
}

export interface StatusBar {
  readonly element: HTMLElement
  /** The canvas status code (#45): waiting for a display, or a missing diagram. */
  setStatus(code: SessionStatus | null): void
  /** The panel's own line (a refused element request); it wins over the code. */
  setMessage(message: string | (() => string) | null): void
  /**
   * The session's request-status list (#51), oldest first. The bar shows the
   * current entry, the number waiting behind it and, when open, the whole queue.
   */
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
 * The request the bar shows (#53, F23): an unseen failure first — a failure is
 * what the user must not miss, and it stays until dismissed — else the running
 * head, else the oldest queued request, else the newest entry. All of it comes
 * from the list itself, so a refreshed page and an open page agree.
 * @param entries the visible entries, oldest first.
 * @param seenFailures the ids of the failures this panel has already shown.
 * @returns the current entry, or null for an empty queue.
 */
export function currentRequest(entries: readonly RequestStatusEntry[],
  seenFailures: ReadonlySet<string>): RequestStatusEntry | null {
  const unseen = entries.find(entry => entry.state === 'failed' && !seenFailures.has(entry.id))
  if (unseen) return unseen
  return entries.find(entry => entry.state === 'running')
    ?? entries.find(entry => entry.state === 'queued')
    ?? entries.at(-1)
    ?? null
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

/**
 * Attach the status bar to its section of the panel.
 * @param host the section's parent.
 * @param options the text and the dismissal callback.
 * @returns the bar.
 */
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
  const more = doc.createElement('button')
  more.type = 'button'
  more.className = 'dsh-uml-viewer-request-more'
  more.hidden = true
  element.appendChild(more)
  let code: SessionStatus | null = null
  let message: string | (() => string) | null = null
  let requests: readonly RequestStatusEntry[] = []
  /** Completed entries the client has faded; the host still holds them (#53). */
  const faded = new Set<string>()
  const fadeTimers = new Map<string, ReturnType<typeof setTimeout>>()
  /** Failures this panel has already shown, so a seen one no longer jumps the queue. */
  const seenFailures = new Set<string>()
  let current: RequestStatusEntry | null = null
  /** The queue the current entry was chosen from, so a re-push changes nothing. */
  let lastSignature: string | null = null
  let opened = false
  let list: HTMLElement | null = null
  /** The entries the bar shows: everything the host lists, minus the faded ones. */
  const visible = (): readonly RequestStatusEntry[] => requests.filter(entry => !faded.has(entry.id))
  const clearTimer = (id: string): void => {
    const timer = fadeTimers.get(id)
    if (timer !== undefined) clearTimeout(timer)
    fadeTimers.delete(id)
  }
  /**
   * A completed entry fades ten seconds after the client sees it finish, or at
   * once when the next request starts. Both only affect what this bar shows:
   * the host's list is unchanged, so a dismissal remains the user's own act.
   */
  const fade = (id: string): void => {
    clearTimer(id)
    if (!faded.has(id)) {
      faded.add(id)
      sync()
    }
  }
  const renderList = (): void => {
    if (!opened || visible().length < 2) {
      list?.remove()
      list = null
      more.setAttribute('aria-expanded', 'false')
      return
    }
    if (!list) {
      list = doc.createElement('div')
      list.className = 'dsh-uml-viewer-request-list'
      list.setAttribute('role', 'list')
    }
    list.setAttribute('aria-label', text.t('request.queue'))
    more.setAttribute('aria-expanded', 'true')
    const items = visible().map(entry => {
      const item = doc.createElement('div')
      item.className = 'dsh-uml-viewer-request-item'
      item.setAttribute('role', 'listitem')
      item.dataset.id = entry.id
      item.dataset.state = entry.state
      const said = requestText(text, entry)
      const body = doc.createElement('span')
      body.className = 'dsh-uml-viewer-request-item-text'
      body.textContent = said
      // The reason of a refused request is long: the whole line stays readable
      // on hover even when the bar is narrow.
      body.setAttribute('title', said)
      item.appendChild(body)
      // Only a failure waits for the user; a completed one fades on its own.
      if (entry.state === 'failed' && options.onDismiss) {
        const dismiss = doc.createElement('button')
        dismiss.type = 'button'
        dismiss.className = 'dsh-uml-viewer-request-dismiss'
        dismiss.textContent = '\u00d7'
        dismiss.setAttribute('aria-label', text.t('request.dismiss', { line: said }))
        dismiss.setAttribute('title', text.t('request.dismiss', { line: said }))
        dismiss.addEventListener('click', event => {
          event.preventDefault()
          options.onDismiss?.(entry.id)
        })
        item.appendChild(dismiss)
      }
      return item
    })
    list.replaceChildren(...items)
    // Keep it below the current line even on a language relabel (idempotent).
    if (element.children[element.children.length - 1] !== list) element.appendChild(list)
  }
  const render = (): void => {
    request.textContent = current ? requestText(text, current) : ''
    request.hidden = current === null
    request.dataset.state = current?.state ?? ''
    element.dataset.request = current?.state ?? ''
    const waiting = visible().length - 1
    more.hidden = waiting < 1
    more.textContent = text.t('request.more', { count: String(Math.max(waiting, 0)) })
    more.setAttribute('aria-label', text.t('request.more.label', { count: String(Math.max(waiting, 0)) }))
    element.dataset.requestCount = String(visible().length)
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
    renderList()
  }
  /** The list as the current entry's choice reads it: ids and states in order. */
  const signatureOf = (entries: readonly RequestStatusEntry[]): string =>
    entries.map(entry => `${entry.id}:${entry.state}`).join('\n')
  /**
   * Pick the current entry again only when the queue actually changed: a
   * re-push of the same list (another event on the stream) must not demote a
   * failure the user is looking at.
   */
  const sync = (): void => {
    const entries = visible()
    const signature = signatureOf(entries)
    if (signature !== lastSignature || (current !== null && !entries.some(entry => entry.id === current!.id))) {
      lastSignature = signature
      current = currentRequest(entries, seenFailures)
      if (current?.state === 'failed') seenFailures.add(current.id)
    }
    render()
  }
  const apply = (next: readonly RequestStatusEntry[]): void => {
    const before = new Set(requests.map(entry => entry.id))
    const states = new Map(requests.map(entry => [entry.id, entry.state]))
    requests = next
    // A finished entry that is gone from the host's list is gone from the
    // timers too (a dismissal, or the host's own bound).
    for (const id of [...fadeTimers.keys()]) if (!next.some(entry => entry.id === id)) clearTimer(id)
    for (const id of [...faded]) if (!next.some(entry => entry.id === id)) faded.delete(id)
    // A request the host no longer lists is never coming back; forget that the
    // user saw it, so the set stays as small as the session's queue.
    for (const id of [...seenFailures]) if (!next.some(entry => entry.id === id)) seenFailures.delete(id)
    // The next request started: what completed before it has had its turn.
    if (next.some(entry => !before.has(entry.id) && (entry.state === 'queued' || entry.state === 'running'))) {
      for (const entry of requests) if (entry.state === 'done') fade(entry.id)
    }
    for (const entry of next) {
      if (entry.state !== 'done' || faded.has(entry.id) || fadeTimers.has(entry.id)) continue
      if (states.get(entry.id) === 'done') continue
      fadeTimers.set(entry.id, setTimeout(() => fade(entry.id), DONE_FADE_MS))
    }
  }
  const label = (): void => {
    element.setAttribute('aria-label', text.t('statusbar.label'))
    request.setAttribute('aria-label', text.t('request.label'))
  }
  more.addEventListener('click', event => {
    event.preventDefault()
    opened = !opened
    renderList()
  })
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
      apply(next)
      sync()
    },
    dispose() {
      unsubscribe()
      for (const id of [...fadeTimers.keys()]) clearTimer(id)
      list?.remove()
      list = null
      element.remove()
    },
  }
}
