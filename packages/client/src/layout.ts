/**
 * The panel's wide layout (#49, ADR 0015): from 900px of *container* width the
 * class card docks on the right beside the diagram; below it, the card is a
 * bottom drawer. The panel's own width decides, never the slot it sits in, so
 * a fullscreen sidebar tab and a wide docked pane both count as wide.
 *
 * The result is the container's `data-layout` attribute (`drawer`|`docked`),
 * which CSS reads; other wide-layout features (the legend, #50) reuse it.
 */

/** The container width, in CSS px, from which the panel is wide. */
export const DOCK_WIDTH = 900

export type PanelLayout = 'drawer' | 'docked'

export function layoutFor(width: number): PanelLayout {
  return width >= DOCK_WIDTH ? 'docked' : 'drawer'
}

/**
 * Keep `element.dataset.layout` in step with the element's width; `onChange`
 * hears each new layout. Returns the disposer.
 */
export function observeLayout(element: HTMLElement, onChange?: (layout: PanelLayout) => void): () => void {
  let disposed = false
  const measure = (): void => {
    if (disposed) return
    const layout = layoutFor(element.clientWidth)
    if (element.dataset.layout === layout) return
    element.dataset.layout = layout
    onChange?.(layout)
  }
  measure()
  const Resize = element.ownerDocument?.defaultView?.ResizeObserver
  const observer = Resize ? new Resize(measure) : null
  observer?.observe(element)
  return () => {
    disposed = true
    observer?.disconnect()
  }
}
