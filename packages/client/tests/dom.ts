import { fakeCanvas } from './fakes.js'

/** Minimal DOM for real mountPanel handlers; no browser or renderer substitutes. */
export class Element extends EventTarget {
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
  remove(): void { if (this.parent) this.parent.children = this.parent.children.filter(child => child !== this); this.parent = null }
  setAttribute(key: string, value: string): void { this.attributes[key] = value }
  focus(): void { this.ownerDocument.activeElement = this }
  blur(): void { this.ownerDocument.activeElement = null }
  fire(type: string, props: Record<string, unknown> = {}): Event {
    const event = new Event(type, { cancelable: true, bubbles: true })
    Object.assign(event, props)
    this.dispatchEvent(event)
    const target = event.target
    Object.defineProperty(event, 'target', { value: target, configurable: true })
    if (!event.cancelBubble) this.parent?.bubble(event)
    return event
  }
  bubble(event: Event): void { this.dispatchEvent(event); if (!event.cancelBubble) this.parent?.bubble(event) }
  find(tag: string): Element { const el = this.descendants().find(child => child.tagName === tag); if (!el) throw new Error(`missing ${tag}`); return el }
  descendants(): Element[] { return this.children.flatMap(child => [child, ...child.descendants()]) }
}
export function makeDocument() {
  return { activeElement: null as Element | null, defaultView: {}, createElement(tag: string): Element { return new Element(tag, this) } }
}
