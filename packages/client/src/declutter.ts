/** The resident owns the five declutter modes; the control only sends its action. */
import type { PanelController } from './panel.js'
import { ENGLISH_TEXT, type PanelText, type TextKey } from './locale.js'

export const DECLUTTER_STYLE = `
.dsh-uml-viewer-declutter {
  position: absolute; top: 8px; right: 8px; z-index: 1;
  padding: 5px 8px; border: 1px solid var(--dsw-alias-border-l2);
  border-radius: 4px; font: 12px/1.4 var(--dsw-font-family, system-ui);
  color: var(--dsw-alias-label-primary); background: var(--dsw-alias-bg-layer-2);
  cursor: pointer;
}
.dsh-uml-viewer-declutter:hover { background: var(--dsw-alias-interactive-bg-hover); }
.dsh-uml-viewer-declutter:focus-visible { outline: 2px solid var(--dsw-alias-state-business-primary); outline-offset: 2px; }
.dsh-uml-viewer-declutter:disabled { opacity: .6; cursor: default; }
`

const LABELS: Readonly<Record<string, TextKey>> = {
  full: 'declutter.full', arrows: 'declutter.arrows', triangles: 'declutter.triangles',
  elements: 'declutter.elements', classes: 'declutter.classes',
}

/** Bind a visible keyboard-accessible button to the actual resident state. */
export function mountDeclutterControl(host: HTMLElement, controller: PanelController, text: PanelText = ENGLISH_TEXT): () => void {
  const button = host.ownerDocument.createElement('button')
  button.type = 'button'
  button.className = 'dsh-uml-viewer-declutter'
  let pending = false
  let disposed = false
  const update = (): void => {
    const mode = controller.declutterMode
    const key = LABELS[mode]
    const label = text.t('declutter.label', { mode: key ? text.t(key) : mode })
    button.textContent = label
    button.setAttribute('aria-label', label)
    button.dataset.mode = mode
    button.disabled = pending || controller.current.kind !== 'live' || !controller.current.frame || controller.current.frame.waiting
  }
  const activate = async (): Promise<void> => {
    if (button.disabled) return
    pending = true
    update()
    try {
      const ack = await controller.send({ action: 'declutter' })
      if (!disposed && ack?.accepted) await controller.send({ action: 'save' })
    } finally {
      pending = false
      if (!disposed) update()
    }
  }
  const click = (): void => { void activate().catch(() => { /* controller reports remote failures */ }) }
  button.addEventListener('click', click)
  const unsubscribe = controller.subscribe(update)
  const unsubscribeText = text.subscribe(update)
  update()
  host.append(button)
  return () => {
    disposed = true
    unsubscribe()
    unsubscribeText()
    button.removeEventListener('click', click)
    button.remove()
  }
}
