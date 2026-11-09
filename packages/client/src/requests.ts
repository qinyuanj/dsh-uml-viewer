import type { PanelAck, PanelAction, QualityRequestOp, SceneFrame } from '@dsh-uml-viewer/contracts'
import { QUALITY_REQUESTS } from '@dsh-uml-viewer/contracts'
import { cssToWorld } from './fit.js'
import { hitTest } from './paint.js'

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

const LABELS: Record<QualityRequestOp, string> = {
  'refresh-crap': 'Refresh CRAP',
  'refresh-mutate': 'Differential mutation',
  'refresh-mutate-all': 'Full mutation (--mutate-all)',
}

/**
 * Attach a quality menu to one panel. It sends only a validated element id to
 * that panel's `action` API; the host resolves the files and queues the visible
 * message. It uses no composer API and never focuses or changes a draft.
 */
export function mountQualityMenu(host: HTMLElement, canvas: HTMLCanvasElement, options: {
  frame(): SceneFrame | null
  send(action: PanelAction): Promise<PanelAck | null>
  report(reason: string | null): void
}): () => void {
  const menu = document.createElement('div')
  menu.className = 'dsh-uml-quality-menu'
  menu.setAttribute('role', 'menu')
  menu.setAttribute('aria-label', 'Quality refresh')
  menu.hidden = true
  let target: string | null = null
  let disposed = false
  const hide = (): void => { menu.hidden = true; target = null }
  for (const op of QUALITY_REQUESTS) {
    const button = document.createElement('button')
    button.type = 'button'
    button.setAttribute('role', 'menuitem')
    button.textContent = LABELS[op]
    button.addEventListener('click', () => {
      const id = target
      hide()
      if (id === null) return
      options.report(null)
      void options.send({ action: 'quality-request', request: op, id }).then(ack => {
        if (!disposed && ack && !ack.accepted) options.report(ack.reason ?? 'The request was not accepted.')
      }, error => {
        if (!disposed) options.report(error instanceof Error ? error.message : String(error))
      })
    })
    menu.appendChild(button)
  }
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
    menu.style.top = `${Math.max(0, Math.min(event.offsetY, bounds.height - 110)) + (canvas.offsetTop ?? 0)}px`
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
    canvas.removeEventListener('contextmenu', open)
    host.removeEventListener('pointerdown', dismiss)
    host.removeEventListener('keydown', key, true)
    menu.remove()
  }
}
