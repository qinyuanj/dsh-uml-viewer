/**
 * The panel's bottom status bar (#46, ADR 0015): the third section of the
 * skeleton, below the canvas. It shows what used to be painted into the canvas
 * (the waiting / missing-diagram hint, a refusal) as localized DOM text, and it
 * is the region the request status of #51 grows in.
 *
 * The text comes from the locale files, so a DSH language switch relabels the
 * bar in place (#42). The host's UI primitives are never loaded (ADR 0013).
 */
import type { SessionStatus } from '@dsh-uml-viewer/contracts'
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
  dispose(): void
}

/** The localized text of a canvas status code (#45). */
export function statusCodeText(text: PanelText, code: SessionStatus | null): string {
  if (code === null) return ''
  return code.code === 'missing-file'
    ? text.t('status.missingFile', { path: code.path })
    : text.t('status.waitingDisplay')
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
  let code: SessionStatus | null = null
  let message: string | (() => string) | null = null
  const render = (): void => {
    const reason = typeof message === 'function' ? message() : message
    const value = reason ?? statusCodeText(text, code)
    line.textContent = value
    // The bar is the panel's third section and always there (#46); only its
    // text comes and goes.
    line.hidden = value === ''
    element.dataset.code = code?.code ?? ''
    element.dataset.tone = reason ? 'error' : code === null ? 'none' : 'info'
  }
  element.setAttribute('aria-label', text.t('statusbar.label'))
  render()
  const unsubscribe = text.subscribe(() => {
    element.setAttribute('aria-label', text.t('statusbar.label'))
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
    dispose() {
      unsubscribe()
      element.remove()
    },
  }
}
