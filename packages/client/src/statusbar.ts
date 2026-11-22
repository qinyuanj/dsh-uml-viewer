import type { RequestStatusEntry, SessionStatus } from '@dsh-uml-viewer/contracts'
import { ENGLISH_TEXT, type PanelText } from './locale.js'

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
.dsh-uml-viewer-statusbar-live {
  position: absolute; width: 1px; height: 1px; padding: 0; margin: -1px;
  overflow: hidden; clip-path: inset(50%); white-space: nowrap; border: 0;
}
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
.dsh-uml-viewer-request-list {
  position: absolute; left: 4px; right: 4px; bottom: 100%; z-index: 50; max-height: 180px;
  overflow-x: hidden; overflow-y: auto; box-sizing: border-box; padding: 4px; margin-bottom: 2px;
  border: 1px solid var(--dsw-alias-border-l2); border-radius: 6px;
  background: var(--dsw-alias-bg-layer-2); box-shadow: var(--dsw-shadow-lv2, 0 4px 12px rgb(0 0 0 / 18%));
}
.dsh-uml-viewer-request-item {
  display: flex; align-items: center; gap: 6px; padding: 1px 2px; white-space: nowrap;
}
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
  onDismiss?: (id: string) => void
}

export interface StatusBar {
  readonly element: HTMLElement
  setStatus(code: SessionStatus | null): void
  setMessage(message: string | (() => string) | null): void
  setRequests(requests: readonly RequestStatusEntry[]): void
  dispose(): void
}

export function statusCodeText(text: PanelText, code: SessionStatus | null): string {
  if (code === null) return ''
  return code.code === 'missing-file'
    ? text.t('status.missingFile', { path: code.path })
    : text.t('status.waitingDisplay')
}

export function currentRequest(entries: readonly RequestStatusEntry[],
  seenFailures: ReadonlySet<string>): RequestStatusEntry | null {
  const unseen = entries.find(entry => entry.state === 'failed' && !seenFailures.has(entry.id))
  if (unseen) return unseen
  return entries.find(entry => entry.state === 'running')
    ?? entries.find(entry => entry.state === 'queued')
    ?? entries.at(-1)
    ?? null
}

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

let statusBarCounter = 0

function parentNode(node: EventTarget | null): Node | null {
  const element = node as (Node & { parent?: Node | null }) | null
  return element?.parentNode ?? element?.parent ?? null
}

function within(node: EventTarget | null, root: HTMLElement): boolean {
  let current = node as Node | null
  while (current !== null) {
    if (current === root) return true
    current = parentNode(current)
  }
  return false
}

export function mountStatusBar(host: HTMLElement, options: StatusBarOptions = {}): StatusBar {
  const text = options.text ?? ENGLISH_TEXT
  const doc = host.ownerDocument
  const element = doc.createElement('div')
  element.className = 'dsh-uml-viewer-statusbar'
  element.setAttribute('role', 'group')
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
  const listId = `dsh-uml-viewer-request-list-${++statusBarCounter}`
  more.setAttribute('aria-controls', listId)
  element.appendChild(more)
  const live = doc.createElement('span')
  live.className = 'dsh-uml-viewer-statusbar-live'
  live.setAttribute('role', 'status')
  live.setAttribute('aria-live', 'polite')
  live.setAttribute('aria-atomic', 'true')
  element.appendChild(live)
  let code: SessionStatus | null = null
  let message: string | (() => string) | null = null
  let requests: readonly RequestStatusEntry[] = []
  const faded = new Set<string>()
  const fadeTimers = new Map<string, ReturnType<typeof setTimeout>>()
  const seenFailures = new Set<string>()
  let current: RequestStatusEntry | null = null
  let lastSignature: string | null = null
  let opened = false
  let list: HTMLElement | null = null
  const visible = (): readonly RequestStatusEntry[] => requests.filter(entry => !faded.has(entry.id))
  const clearTimer = (id: string): void => {
    const timer = fadeTimers.get(id)
    if (timer !== undefined) clearTimeout(timer)
    fadeTimers.delete(id)
  }
  const fade = (id: string): void => {
    clearTimer(id)
    if (!faded.has(id)) {
      faded.add(id)
      sync()
    }
  }
  const renderList = (): void => {
    if (visible().length < 2) opened = false
    if (!opened || visible().length < 2) {
      list?.remove()
      list = null
      more.setAttribute('aria-expanded', 'false')
      return
    }
    if (!list) {
      list = doc.createElement('div')
      list.className = 'dsh-uml-viewer-request-list'
      list.setAttribute('id', listId)
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
      const requestLine = requestText(text, entry)
      const body = doc.createElement('span')
      body.className = 'dsh-uml-viewer-request-item-text'
      body.textContent = requestLine
      body.setAttribute('title', requestLine)
      item.appendChild(body)
      if (entry.state === 'failed' && options.onDismiss) {
        const dismiss = doc.createElement('button')
        dismiss.type = 'button'
        dismiss.className = 'dsh-uml-viewer-request-dismiss'
        dismiss.textContent = '\u00d7'
        dismiss.setAttribute('aria-label', text.t('request.dismiss', { line: requestLine }))
        dismiss.setAttribute('title', text.t('request.dismiss', { line: requestLine }))
        dismiss.addEventListener('click', event => {
          event.preventDefault()
          options.onDismiss?.(entry.id)
        })
        item.appendChild(dismiss)
      }
      return item
    })
    list.replaceChildren(...items)
    if (element.children[element.children.length - 1] !== list) element.appendChild(list)
  }
  const render = (): void => {
    request.textContent = current ? requestText(text, current) : ''
    request.hidden = current === null
    request.dataset.state = current?.state ?? ''
    const waiting = visible().length - 1
    more.hidden = waiting < 1
    more.textContent = text.t('request.more', { count: String(Math.max(waiting, 0)) })
    more.setAttribute('aria-label', text.t('request.more.label', { count: String(Math.max(waiting, 0)) }))
    const statusMessage = typeof message === 'function' ? message() : message
    const reason = statusMessage !== null && current?.state === 'failed' && current.reason === statusMessage ? null : statusMessage
    const value = reason ?? statusCodeText(text, code)
    line.textContent = value
    line.hidden = value === ''
    live.textContent = [request.hidden ? '' : request.textContent, value].filter(part => part !== '').join('. ')
    element.dataset.code = code?.code ?? ''
    element.dataset.tone = reason ? 'error' : code === null ? 'none' : 'info'
    renderList()
  }
  const signatureOf = (entries: readonly RequestStatusEntry[]): string =>
    entries.map(entry => `${entry.id}:${entry.state}`).join('\n')
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
    for (const id of [...fadeTimers.keys()]) if (!next.some(entry => entry.id === id)) clearTimer(id)
    for (const id of [...faded]) if (!next.some(entry => entry.id === id)) faded.delete(id)
    for (const id of [...seenFailures]) if (!next.some(entry => entry.id === id)) seenFailures.delete(id)
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
  const onOutside = (event: Event): void => {
    if (!opened || within(event.target, element)) return
    opened = false
    renderList()
  }
  const onKey = (event: Event): void => {
    if (!opened || (event as KeyboardEvent).isComposing || (event as KeyboardEvent).key !== 'Escape') return
    event.preventDefault()
    event.stopPropagation()
    opened = false
    renderList()
    more.focus()
  }
  doc.addEventListener?.('pointerdown', onOutside, true)
  doc.addEventListener?.('keydown', onKey, true)
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
      doc.removeEventListener?.('pointerdown', onOutside, true)
      doc.removeEventListener?.('keydown', onKey, true)
      for (const id of [...fadeTimers.keys()]) clearTimer(id)
      list?.remove()
      list = null
      element.remove()
    },
  }
}
