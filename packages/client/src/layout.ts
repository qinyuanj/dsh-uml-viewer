export const DOCK_WIDTH = 900

export type PanelLayout = 'drawer' | 'docked'

export function layoutFor(width: number): PanelLayout {
  return width >= DOCK_WIDTH ? 'docked' : 'drawer'
}

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
