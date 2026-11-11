import { readFileSync } from 'node:fs'
import { describe, expect, it } from 'vitest'
import type { PanelAck, PanelAction, SceneFrame } from '@dsh-uml-viewer/contracts'
import { elementSelection, mountActionBar, type ElementSelection } from '../src/action-bar.js'
import { Element, makeDocument } from './dom.js'

const golden = (name: string): SceneFrame =>
  JSON.parse(readFileSync(new URL(`../../../crates/frame/tests/fixtures/frames/${name}.frame.json`, import.meta.url), 'utf8')) as SceneFrame

/** The root of the shop sample: drillable namespaces, and `serde` as a foreign package. */
const root = golden('root')
/** Inside `core`: leaf classes. */
const core = golden('core-selected')

describe('the selection the action bar is for (#48)', () => {
  it('is nothing when nothing is selected', () => {
    expect(elementSelection(null, root)).toBeNull()
    expect(elementSelection('{:kind :class :id :core}', null)).toBeNull()
  })

  it('names a leaf class from its box and offers its class card', () => {
    expect(elementSelection('{:kind :class :id :core.order :drill? false}', core))
      .toEqual({ kind: 'class', id: ':core.order', name: 'Order' })
  })

  it('treats a drillable box as a component, whatever order the EDN keys come in', () => {
    expect(elementSelection('{:drill? true, :id :core, :kind :class}', root))
      .toEqual({ kind: 'component', id: ':core', name: 'Core' })
  })

  it('reads a foreign package ellipse as name only', () => {
    expect(elementSelection('{:kind :class :id :serde :drill? false}', root))
      .toEqual({ kind: 'foreign', id: ':serde', name: 'serde' })
  })

  it('marks a child row as a member whose id has no class card of its own (F49)', () => {
    expect(elementSelection('{:kind :child :id :core.order.price :parent :core.order}', core))
      .toEqual({ kind: 'class', id: ':core.order.price', name: 'price', member: true })
  })

  it('treats a proposal layer as a component and the view frame itself as no element', () => {
    expect(elementSelection('{:kind :package :id :proposal.domain}', golden('proposal')))
      .toEqual({ kind: 'component', id: ':proposal.domain', name: 'Domain' })
    // A click on blank canvas hits the level's own frame: there is nothing to act on.
    expect(elementSelection('{:kind :package :id :view}', root)).toBeNull()
  })

  it('has no bar for a dependency port, or for an element the frame no longer draws', () => {
    expect(elementSelection('{:kind :port :id :api}', core)).toBeNull()
    expect(elementSelection('{:kind :class :id :gone}', core)).toBeNull()
  })
})

function harness() {
  const doc = makeDocument()
  const host = new Element('div', doc)
  const actions: PanelAction[] = []
  const cards: string[] = []
  const reports: (string | null)[] = []
  let ack: PanelAck = { accepted: true, reason: null }
  const bar = mountActionBar(host as unknown as HTMLElement, {
    send: async action => { actions.push(action); return ack },
    openCard: id => { cards.push(id) },
    report: reason => { reports.push(typeof reason === 'function' ? reason() : reason) },
  })
  const element = bar.element as unknown as Element
  const buttons = (): Element[] => element.descendants().filter(child => child.tagName === 'button' && !hiddenWithin(child, element))
  const named = (label: string): Element => {
    const button = buttons().find(child => child.textContent === label || child.getAttribute('aria-label') === label)
    if (!button) throw new Error(`no visible button ${label}: ${buttons().map(b => b.textContent || b.getAttribute('aria-label')).join(', ')}`)
    return button
  }
  const menu = (): Element => element.descendants().find(child => child.className.includes('dsh-uml-viewer-action-menu'))!
  return {
    bar, element, host, actions, cards, reports, buttons, named, menu,
    refuse(reason: string | null) { ack = { accepted: false, reason } },
    show(selection: ElementSelection | null) { bar.render(selection) },
  }
}

/** A button counts as shown only when neither it nor an ancestor up to the bar is hidden. */
function hiddenWithin(child: Element, stop: Element): boolean {
  for (let node: Element | null = child; node && node !== stop; node = node.parent) if (node.hidden) return true
  return stop.hidden
}

const settle = () => new Promise(resolve => setImmediate(resolve))
const labels = (h: ReturnType<typeof harness>) => h.buttons().map(button => button.textContent || button.getAttribute('aria-label'))

describe('the selection action bar (#48)', () => {
  it('is hidden until something is selected, and hides again when the selection clears', () => {
    const h = harness()
    expect(h.host.children).toContain(h.element)
    expect(h.element.hidden).toBe(true)
    h.show({ kind: 'class', id: ':core.order', name: 'Order' })
    expect(h.element.hidden).toBe(false)
    expect(h.element.getAttribute('aria-label')).toBe('Selection actions')
    h.show(null)
    expect(h.element.hidden).toBe(true)
    h.bar.dispose()
    expect(h.host.children).not.toContain(h.element)
  })

  it('offers a class its card, the two refreshes and Hide, and full mutation only behind the secondary menu', () => {
    const h = harness()
    h.show({ kind: 'class', id: ':core.order', name: 'Order' })
    expect(h.element.descendants().some(child => child.textContent === 'Order')).toBe(true)
    expect(labels(h)).toEqual(['Open class card', 'Refresh CRAP', 'Differential mutation', 'Hide', 'More requests'])
    h.named('Open class card').fire('click')
    expect(h.cards).toEqual([':core.order'])
    h.bar.dispose()
  })

  it('never offers an unresolvable class card for a member row (F49)', () => {
    const h = harness()
    h.show({ kind: 'class', id: ':core.order.price', name: 'price', member: true })
    expect(labels(h)).toEqual(['Refresh CRAP', 'Differential mutation', 'Hide', 'More requests'])
    expect(h.cards).toEqual([])
    h.bar.dispose()
  })

  it('offers a component drill-in instead of a card', async () => {
    const h = harness()
    h.show({ kind: 'component', id: ':core', name: 'Core' })
    expect(labels(h)).toEqual(['Drill in', 'Refresh CRAP', 'Differential mutation', 'Hide', 'More requests'])
    h.named('Drill in').fire('click')
    await settle()
    expect(h.actions).toEqual([{ action: 'drill-in', id: ':core' }])
    h.bar.dispose()
  })

  it('shows a foreign package by name alone: no refresh, no Hide', () => {
    const h = harness()
    h.show({ kind: 'foreign', id: ':serde', name: 'serde' })
    expect(h.element.hidden).toBe(false)
    expect(h.element.dataset.kind).toBe('foreign')
    expect(h.element.descendants().some(child => child.textContent === 'serde')).toBe(true)
    expect(labels(h)).toEqual([])
    h.bar.dispose()
  })

  it('sends the same panel requests as the element menu, for the selected id', async () => {
    const h = harness()
    h.show({ kind: 'component', id: ':core', name: 'Core' })
    h.named('Refresh CRAP').fire('click')
    h.named('Differential mutation').fire('click')
    h.named('Hide').fire('click')
    await settle()
    expect(h.actions).toEqual([
      { action: 'quality-request', request: 'refresh-crap', id: ':core' },
      { action: 'quality-request', request: 'refresh-mutate', id: ':core' },
      { action: 'omit', id: ':core' },
    ])
    h.bar.dispose()
  })

  it('keeps full mutation in a secondary menu that is a menu only while open', async () => {
    const h = harness()
    h.show({ kind: 'class', id: ':core.order', name: 'Order' })
    // The host reads any [role="menu"] in the document as an open modal.
    expect(h.menu().hasAttribute('role')).toBe(false)
    expect(h.menu().hidden).toBe(true)
    const more = h.named('More requests')
    expect(more.getAttribute('aria-expanded')).toBe('false')
    more.fire('click')
    expect(h.menu().getAttribute('role')).toBe('menu')
    expect(more.getAttribute('aria-expanded')).toBe('true')
    h.named('Full mutation (--mutate-all)').fire('click')
    await settle()
    expect(h.actions).toEqual([{ action: 'quality-request', request: 'refresh-mutate-all', id: ':core.order' }])
    expect(h.menu().hasAttribute('role')).toBe(false)
    expect(h.menu().hidden).toBe(true)
    // Escape and a new selection close it too.
    more.fire('click')
    h.element.fire('keydown', { key: 'Escape', isComposing: false })
    expect(h.menu().hasAttribute('role')).toBe(false)
    more.fire('click')
    h.show({ kind: 'class', id: ':core.price', name: 'Price' })
    expect(h.menu().hasAttribute('role')).toBe(false)
    h.bar.dispose()
  })

  it('reports a refused request on the status bar like the element menu does', async () => {
    const h = harness()
    h.show({ kind: 'class', id: ':core.order', name: 'Order' })
    h.refuse(null)
    h.named('Refresh CRAP').fire('click')
    await settle()
    expect(h.reports).toEqual([null, 'The request was not accepted.'])
    h.bar.dispose()
  })
})
