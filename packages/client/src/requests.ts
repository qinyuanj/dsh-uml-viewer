import type { PanelAck, PanelAction, QualityRequestOp, SceneFrame } from '@dsh-uml-viewer/contracts'
import { QUALITY_REQUESTS } from '@dsh-uml-viewer/contracts'
import { cssToWorld } from './fit.js'
import { hitTest } from './paint.js'
import { ENGLISH_TEXT, type PanelText, type TextKey } from './locale.js'

export const REQUEST_MENU_STYLE = `
.dsh-uml-quality-menu { position: absolute; z-index: 40; max-width: 100%; padding: 4px;
  border: 1px solid var(--dsw-alias-border-l2); border-radius: 6px;
  background: var(--dsw-alias-bg-overlay); color: var(--dsw-alias-label-primary);
  box-shadow: var(--dsw-shadow-lv2, 0 4px 16px rgb(0 0 0 / 20%));
  font: 12px/1.4 var(--dsw-font-family, system-ui); }
.dsh-uml-quality-menu[hidden] { display: none; }
.dsh-uml-quality-menu button { display: block; width: 100%; border: 0; border-radius: 3px;
  padding: 6px 10px; text-align: left; background: transparent; color: inherit; font: inherit; cursor: pointer; }
.dsh-uml-quality-menu button:hover, .dsh-uml-quality-menu button:focus-visible {
  background: var(--dsw-alias-interactive-bg-hover); outline: 1px solid var(--dsw-focus-ring-color); }
`

const LABELS: Record<QualityRequestOp, TextKey> = {
  'refresh-crap': 'menu.refreshCrap',
  'refresh-mutate': 'menu.refreshMutate',
  'refresh-mutate-all': 'menu.refreshMutateAll',
}

/**
 * Attach the element menu to one panel: the quality refreshes (#28) and Hide
 * (#29, `omit`). It sends only a validated element id to
 * that panel's `action` API; the host resolves the files and queues the visible
 * message. It uses no composer API and never focuses or changes a draft.
 */
export function mountQualityMenu(host: HTMLElement, canvas: HTMLCanvasElement, options: {
  frame(): SceneFrame | null
  send(action: PanelAction): Promise<PanelAck | null>
  /** A thunk is the panel's own text, re-read in the current language. */
  report(reason: string | (() => string) | null): void
  text?: PanelText
}): () => void {
  const text = options.text ?? ENGLISH_TEXT
  const menu = document.createElement('div')
  menu.className = 'dsh-uml-quality-menu'
  menu.setAttribute('role', 'menu')
  menu.hidden = true
  let target: string | null = null
  let disposed = false
  const hide = (): void => { menu.hidden = true; target = null }
  // The three quality refreshes (#28), then Hide (#29): each sends the bound id only.
  const entries: [TextKey, (id: string) => PanelAction][] = [
    ...QUALITY_REQUESTS.map((op): [TextKey, (id: string) => PanelAction] => [LABELS[op], id => ({ action: 'quality-request', request: op, id })]),
    ['menu.hide', id => ({ action: 'omit', id })],
  ]
  const labels: (() => void)[] = [() => menu.setAttribute('aria-label', text.t('menu.label'))]
  for (const [label, action] of entries) {
    const button = document.createElement('button')
    button.type = 'button'
    button.setAttribute('role', 'menuitem')
    labels.push(() => { button.textContent = text.t(label) })
    button.addEventListener('click', () => {
      const id = target
      hide()
      if (id === null) return
      options.report(null)
      void options.send(action(id)).then(ack => {
        if (!disposed && ack && !ack.accepted) options.report(ack.reason ?? (() => text.t('request.refused')))
      }, error => {
        if (!disposed) options.report(error instanceof Error ? error.message : String(error))
      })
    })
    menu.appendChild(button)
  }
  const relabel = (): void => { for (const label of labels) label() }
  relabel()
  const unsubscribe = text.subscribe(relabel)
  host.appendChild(menu)
  const open = (event: MouseEvent): void => {
    event.preventDefault()
    hide()
    const frame = options.frame()
    if (!frame || frame.waiting) return
    const bounds = canvas.getBoundingClientRect()
    const world = cssToWorld(frame, bounds, event.offsetX, event.offsetY)
    const id = world ? hitTest(frame, world) : null
    if (id === null) return
    // Dependency ports and foreign library ellipses have no element menu.
    const element = frame.world.find(item => (item.kind === 'class' || item.kind === 'package') && item.id === id)
    if (!element || (element.kind === 'class' && element.shape === 'oval')) return
    target = id
    menu.style.left = `${Math.max(0, Math.min(event.offsetX, bounds.width - 230))}px`
    menu.style.top = `${Math.max(0, Math.min(event.offsetY, bounds.height - 140)) + (canvas.offsetTop ?? 0)}px`
    menu.hidden = false
  }
  const dismiss = (event: Event): void => {
    if (event.target !== menu && !menu.contains?.(event.target as Node)) hide()
  }
  const key = (event: KeyboardEvent): void => {
    if (menu.hidden || event.isComposing) return
    if (event.key === 'Escape') {
      event.preventDefault()
      event.stopPropagation()
      hide()
    }
  }
  canvas.addEventListener('contextmenu', open)
  host.addEventListener('pointerdown', dismiss)
  host.addEventListener('keydown', key, true)
  return () => {
    disposed = true
    unsubscribe()
    canvas.removeEventListener('contextmenu', open)
    host.removeEventListener('pointerdown', dismiss)
    host.removeEventListener('keydown', key, true)
    menu.remove()
  }
}
