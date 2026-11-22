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

export interface ElementRequest {
  readonly label: TextKey
  action(id: string): PanelAction
  readonly secondary: boolean
}

export const ELEMENT_REQUESTS: readonly ElementRequest[] = [
  ...QUALITY_REQUESTS.map((op): ElementRequest => ({
    label: LABELS[op],
    action: id => ({ action: 'quality-request', request: op, id }),
    secondary: op === 'refresh-mutate-all',
  })),
  { label: 'menu.hide', action: id => ({ action: 'omit', id }), secondary: false },
]

export function sendElementRequest(action: PanelAction, options: {
  send(action: PanelAction): Promise<PanelAck | null>
  report(reason: string | (() => string) | null): void
  text: PanelText
  live(): boolean
}): void {
  options.report(null)
  void options.send(action).then(ack => {
    if (options.live() && ack && !ack.accepted) options.report(ack.reason ?? (() => options.text.t('request.refused')))
  }, error => {
    if (options.live()) options.report(error instanceof Error ? error.message : String(error))
  })
}

export function mountQualityMenu(host: HTMLElement, canvas: HTMLCanvasElement, options: {
  frame(): SceneFrame | null
  send(action: PanelAction): Promise<PanelAck | null>
  report(reason: string | (() => string) | null): void
  text?: PanelText
}): () => void {
  const text = options.text ?? ENGLISH_TEXT
  const menu = document.createElement('div')
  menu.className = 'dsh-uml-quality-menu'
  // `role="menu"` only while open. The host's shortcut service treats any
  // `[role="menu"]` in the document as an open modal, even a `hidden` one, and
  // a modal blocks every command that does not admit it (the tab refresh is one).
  menu.hidden = true
  let target: string | null = null
  let disposed = false
  const hide = (): void => {
    menu.hidden = true
    menu.removeAttribute('role')
    target = null
  }
  const labels: (() => void)[] = [() => menu.setAttribute('aria-label', text.t('menu.label'))]
  for (const request of ELEMENT_REQUESTS) {
    const button = document.createElement('button')
    button.type = 'button'
    button.setAttribute('role', 'menuitem')
    labels.push(() => { button.textContent = text.t(request.label) })
    button.addEventListener('click', () => {
      const id = target
      hide()
      if (id === null) return
      sendElementRequest(request.action(id), { send: options.send, report: options.report, text, live: () => !disposed })
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
    const element = frame.world.find(item => (item.kind === 'class' || item.kind === 'package') && item.id === id)
    if (!element || (element.kind === 'class' && element.shape === 'oval')) return
    target = id
    menu.style.left = `${Math.max(0, Math.min(event.offsetX, bounds.width - 230))}px`
    menu.style.top = `${Math.max(0, Math.min(event.offsetY, bounds.height - 140)) + (canvas.offsetTop ?? 0)}px`
    menu.setAttribute('role', 'menu')
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
