import { readFileSync } from 'node:fs'
import { afterEach, describe, expect, it, vi } from 'vitest'
import type { PanelAction, PanelEvent, SceneFrame } from '@dsh-uml-viewer/contracts'
import { fakeCanvas } from './fakes.js'

vi.mock('react', () => ({ createElement: vi.fn() }))
vi.mock('../src/font.js', () => ({ FONT_FAMILY: 'test', loadSarasa: async () => {} }))
import { mountPanel, type ClientContext } from '../src/index.js'

const golden = JSON.parse(readFileSync(new URL('../../../crates/frame/tests/fixtures/frames/root.frame.json', import.meta.url), 'utf8')) as SceneFrame

/** Minimal DOM with bubbling and native Event cancellation, used by real mountPanel handlers. */
class Element extends EventTarget {
  children: Element[] = []
  parent: Element | null = null
  dataset: Record<string, string> = {}
  style: Record<string, string> = {}
  attributes: Record<string, string> = {}
  textContent = ''
  value = ''
  hidden = false
  disabled = false
  className = ''
  ownerDocument: ReturnType<typeof makeDocument>
  constructor(readonly tagName: string, doc: ReturnType<typeof makeDocument>) {
    super()
    this.ownerDocument = doc
    if (tagName === 'canvas') {
      const canvas = fakeCanvas({ width: 800, height: 600 })
      Object.assign(this, { getContext: canvas.getContext, getBoundingClientRect: canvas.getBoundingClientRect })
    }
  }
  append(...elements: Element[]): void { for (const el of elements) this.appendChild(el) }
  appendChild(el: Element): Element { el.parent = this; this.children.push(el); return el }
  replaceChildren(...elements: Element[]): void { this.children = []; this.append(...elements) }
  setAttribute(key: string, value: string): void { this.attributes[key] = value }
  focus(): void { this.ownerDocument.activeElement = this }
  blur(): void { this.ownerDocument.activeElement = null }
  fire(type: string, props: Record<string, unknown> = {}): Event {
    const event = new Event(type, { cancelable: true, bubbles: true })
    Object.assign(event, props)
    // Native EventTarget supplies target; bubble with that same target.
    this.dispatchEvent(event)
    const target = event.target
    Object.defineProperty(event, 'target', { value: target, configurable: true })
    if (!event.cancelBubble) this.parent?.bubble(event)
    return event
  }
  bubble(event: Event): void { this.dispatchEvent(event); if (!event.cancelBubble) this.parent?.bubble(event) }
  find(tag: string): Element { const el = this.children.flatMap(child => [child, ...child.descendants()]).find(child => child.tagName === tag); if (!el) throw new Error(`missing ${tag}`); return el }
  descendants(): Element[] { return this.children.flatMap(child => [child, ...child.descendants()]) }
}

function makeDocument() {
  return { activeElement: null as Element | null, defaultView: {}, createElement(tag: string): Element { return new Element(tag, this) } }
}

const actions: PanelAction[] = []
let pending: ((event: IteratorResult<PanelEvent>) => void) | undefined
const api = {
  watch: () => ({
    [Symbol.asyncIterator]() { return { next: () => new Promise<IteratorResult<PanelEvent>>(resolve => { pending = resolve }) } },
    end() { pending?.({ done: true, value: undefined }) },
  }),
  async action(_sessionId: string, action: PanelAction) { actions.push(action); return { accepted: true, reason: null } },
}
const ctx = { get: (name: string) => name === 'remote' ? { $mount: async () => {} } : api } as unknown as ClientContext
let dispose: (() => void) | undefined
async function emit(event: PanelEvent): Promise<void> { pending!({ done: false, value: event }); await Promise.resolve(); await Promise.resolve() }
async function mount() {
  actions.length = 0
  const doc = makeDocument()
  vi.stubGlobal('document', doc)
  const host = new Element('host', doc)
  dispose = await mountPanel(host as unknown as HTMLElement, { sessionId: 'session', ctx })
  const frame: SceneFrame = { ...golden, viewport: { width: 800, height: 600 }, camera: { x: 200, y: 200, zoom: 1 }, world: [{ ...golden.world[0]!, bounds: { x: 0, y: 0, w: 4000, h: 4000 } }] }
  await emit({ kind: 'frame', seq: 1, frame })
  actions.length = 0
  return { host, canvas: host.find('canvas'), input: host.find('input'), results: host.find('ul') }
}
afterEach(() => { dispose?.(); dispose = undefined; vi.unstubAllGlobals() })

function camera() { return actions.filter(action => action.action === 'camera').at(-1) }

describe('mounted panel wheel handlers', () => {
  it.each([
    [100, 0, false, 248, 200],
    [100, 200, false, 248, 200],
    [0, 100, true, 248, 200],
    [100, 200, true, 248, 200],
  ])('preserves deltaX=%s deltaY=%s shift=%s', async (deltaX, deltaY, shiftKey, x, y) => {
    const { canvas } = await mount()
    const event = canvas.fire('wheel', { offsetX: 400, offsetY: 300, deltaX, deltaY, shiftKey })
    expect(event.defaultPrevented).toBe(true)
    expect(camera()).toMatchObject({ action: 'camera', x, y, zoom: 1 })
  })

  it('keeps Ctrl wheel zoom anchored with combined axes and Shift', async () => {
    const { canvas } = await mount()
    canvas.fire('wheel', { offsetX: 400, offsetY: 300, deltaX: 300, deltaY: -100, ctrlKey: true, shiftKey: true })
    expect(camera()).toMatchObject({ action: 'camera', zoom: 1.1 })
    const result = camera()!
    if (result.action !== 'camera') throw new Error('missing camera')
    expect((600 - result.x) * result.zoom).toBeCloseTo(400)
    expect((500 - result.y) * result.zoom).toBeCloseTo(300)
  })
})

describe('mounted panel IME handlers', () => {
  it.each([{ isComposing: true }, { isComposing: false, keyCode: 229 }])('leaves composing search keys native (%j), then Enter locates', async (guard) => {
    const { host, canvas, input, results } = await mount()
    canvas.fire('keydown', { key: 'f', ctrlKey: true })
    await emit({ kind: 'search', query: 'price', results: [
      { id: ':one', name: 'price', ns: 'one', label: 'one' },
      { id: ':two', name: 'price', ns: 'two', label: 'two' },
    ] })
    input.fire('keydown', { key: 'ArrowDown' })
    expect(results.find('button').attributes['aria-selected']).toBe('true')
    actions.length = 0
    const before = results.children
    const bubbled: Event[] = []
    host.addEventListener('keydown', event => { bubbled.push(event) })
    for (const key of ['Enter', 'ArrowDown', 'ArrowUp', 'Escape']) {
      const event = input.fire('keydown', { key, ...guard })
      expect(event.defaultPrevented, key).toBe(false)
      expect(bubbled.at(-1), `${key} propagates`).toBe(event)
      expect(results.children, `${key} leaves selection and open search unchanged`).toBe(before)
      expect(input.ownerDocument.activeElement).toBe(input)
      expect(actions).toEqual([])
    }
    input.fire('keydown', { key: 'Enter', isComposing: false, keyCode: 13 })
    expect(actions).toEqual([{ action: 'locate', id: ':one' }])
  })

  it.each([{ isComposing: true }, { keyCode: 229 }])('leaves composing canvas shortcuts native (%j)', async guard => {
    const { canvas } = await mount()
    for (const key of ['Enter', 'ArrowDown', 'ArrowUp', 'Escape', 'p', 'r', 'f']) {
      expect(canvas.fire('keydown', { key, ctrlKey: key === 'f', ...guard }).defaultPrevented).toBe(false)
    }
    expect(actions).toEqual([])
  })
})
