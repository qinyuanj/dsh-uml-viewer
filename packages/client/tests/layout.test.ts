import { describe, expect, it } from 'vitest'
import { DOCK_WIDTH, layoutFor, observeLayout } from '../src/layout.js'

/** A ResizeObserver the test drives by hand, one per observed element. */
function fakeResize() {
  const observers: { callback: () => void; disconnected: boolean }[] = []
  class FakeResizeObserver {
    private readonly entry: { callback: () => void; disconnected: boolean }
    constructor(callback: () => void) {
      this.entry = { callback, disconnected: false }
      observers.push(this.entry)
    }
    observe(): void {}
    disconnect(): void { this.entry.disconnected = true }
  }
  return { FakeResizeObserver, fire: () => { for (const entry of observers) if (!entry.disconnected) entry.callback() }, observers }
}

function panel(width: number, resize?: unknown) {
  return { clientWidth: width, dataset: {} as Record<string, string>, ownerDocument: { defaultView: { ResizeObserver: resize } } }
}

describe('panel layout by container width (#49)', () => {
  it('docks at 900px of container width and uses the drawer below it', () => {
    expect(DOCK_WIDTH).toBe(900)
    expect(layoutFor(400)).toBe('drawer')
    expect(layoutFor(899.5)).toBe('drawer')
    expect(layoutFor(900)).toBe('docked')
    expect(layoutFor(1400)).toBe('docked')
  })

  it('marks the container and follows its resizes until disposed', () => {
    const resize = fakeResize()
    const element = panel(420, resize.FakeResizeObserver)
    const seen: string[] = []
    const stop = observeLayout(element as unknown as HTMLElement, layout => seen.push(layout))
    expect(element.dataset.layout).toBe('drawer')
    element.clientWidth = 960
    resize.fire()
    expect(element.dataset.layout).toBe('docked')
    resize.fire()
    element.clientWidth = 640
    resize.fire()
    expect(element.dataset.layout).toBe('drawer')
    expect(seen).toEqual(['drawer', 'docked', 'drawer'])
    stop()
    expect(resize.observers.every(entry => entry.disconnected)).toBe(true)
    element.clientWidth = 1200
    resize.fire()
    expect(element.dataset.layout).toBe('drawer')
  })

  it('measures once where no ResizeObserver exists', () => {
    const element = panel(1200)
    const stop = observeLayout(element as unknown as HTMLElement)
    expect(element.dataset.layout).toBe('docked')
    stop()
  })
})
