import { readFileSync } from 'node:fs'
import { describe, expect, it } from 'vitest'
import { DECLUTTER_MODES, type PanelAction, type PanelEvent, type SceneFrame, type SessionState } from '@dsh-uml-viewer/contracts'
import { mountMoreMenu, moreMenuEntries } from '../src/more-menu.js'
import { PanelController, type PanelStream } from '../src/panel.js'
import { ENGLISH_TEXT, translate, type PanelText } from '../src/locale.js'
import { Element, makeDocument } from './dom.js'
import { fakeCanvas } from './fakes.js'

const frame = JSON.parse(readFileSync(new URL('../../../crates/frame/tests/fixtures/frames/root.frame.json', import.meta.url), 'utf8')) as SceneFrame

function deferred<T>() {
  let resolve!: (value: T) => void
  let reject!: (reason: unknown) => void
  const promise = new Promise<T>((yes, no) => { resolve = yes; reject = no })
  return { promise, resolve, reject }
}

function stateWith(declutter: string | null): SessionState {
  return {
    schema: 'dsh-uml-viewer/session-state', version: 1, diagram: null, context: null,
    focus: [], proposalId: null, openLayer: null, declutter,
    camera: { x: 0, y: 0, zoom: 1 }, selected: null, detail: null,
  }
}

async function flush(): Promise<void> {
  await new Promise<void>(resolve => setImmediate(resolve))
}

function visibleText(element: Element): string {
  return element.descendants().map(child => child.textContent).join('')
}

function accessibleText(element: Element): string {
  return element.descendants().filter(child => child.getAttribute('aria-hidden') !== 'true')
    .map(child => child.textContent).join('')
}

const CHINESE_TEXT: PanelText = {
  language: 'zh',
  t: (key, params) => translate('zh', key, params),
  subscribe: () => () => {},
}

async function harness(options: { declutter?: string | null; rejectSave?: boolean; text?: PanelText } = {}) {
  const ack = deferred<unknown>()
  let mounted = false
  const actions: { sessionId: string; action: PanelAction }[] = []
  const doc = makeDocument()
  const host = new Element('div', doc)
  const trigger = doc.createElement('button') as unknown as Element
  host.appendChild(trigger)
  const streams: { emit(event: PanelEvent): void }[] = []
  const controller = new PanelController({
    canvas: fakeCanvas(), paint() {},
    api: {
      watch() {
        const queue: PanelEvent[] = []
        let waiting: ReturnType<typeof deferred<IteratorResult<PanelEvent>>> | null = null
        let ended = false
        streams.push({ emit(event) {
          if (waiting) { waiting.resolve({ done: false, value: event }); waiting = null } else queue.push(event)
        } })
        return {
          [Symbol.asyncIterator]() {
            return { next: async () => {
              const value = queue.shift()
              if (value) return { done: false, value }
              if (ended) return { done: true, value: undefined }
              waiting = deferred<IteratorResult<PanelEvent>>()
              return waiting.promise
            } }
          },
          end() { ended = true; waiting?.resolve({ done: true, value: undefined }) },
        } satisfies PanelStream
      },
      action(sessionId, action) {
        actions.push({ sessionId, action })
        if (action.action === 'declutter') return ack.promise
        if (mounted && action.action === 'save' && options.rejectSave) return Promise.reject(new Error('save unavailable'))
        return Promise.resolve({ accepted: true, reason: null })
      },
    },
  })
  const emit = (event: PanelEvent): void => streams.at(-1)!.emit(event)
  await controller.open('session-a')
  emit({ kind: 'frame', seq: 1, frame })
  emit({ kind: 'state', state: stateWith(options.declutter ?? null), breadcrumbs: [{ id: null, name: 'Shop' }], status: null, proposals: [] })
  await flush()
  const text = options.text ?? ENGLISH_TEXT
  const menu = mountMoreMenu(host as unknown as HTMLElement, trigger as unknown as HTMLButtonElement, {
    text,
    entries: () => moreMenuEntries(controller, text),
  })
  actions.length = 0
  mounted = true
  const menuElement = host.children.find(child => child.className.includes('dsh-uml-viewer-more-menu'))!
  const item = (id: string): Element => menuElement.descendants().find(child =>
    child.tagName === 'button' && child.dataset.entry === id)!
  return { ack, actions, controller, menu, menuElement, trigger, item, emit, host }
}

describe('the top bar More menu (#46)', () => {
  it('has role="menu" only while open, so the host does not read a hidden modal', async () => {
    const h = await harness()
    expect(h.menuElement.hasAttribute('role')).toBe(false)
    expect(h.menuElement.hidden).toBe(true)
    h.menu.open()
    expect(h.menuElement.getAttribute('role')).toBe('menu')
    expect(h.menuElement.hidden).toBe(false)
    expect(h.trigger.getAttribute('aria-expanded')).toBe('true')
    h.menu.close()
    expect(h.menuElement.hasAttribute('role')).toBe(false)
    expect(h.trigger.getAttribute('aria-expanded')).toBe('false')
    h.menu.dispose()
    h.controller.close()
  })

  it('offers Fit view and all five declutter modes in one labeled radio group', async () => {
    const h = await harness()
    h.menu.open()
    const fit = h.item('fit')
    expect(visibleText(fit)).toContain('Fit view')
    expect(fit.getAttribute('aria-keyshortcuts')).toBe('Ctrl+0')
    expect(visibleText(fit)).toContain('Ctrl+0')

    const groups = h.menuElement.descendants().filter(child => child.getAttribute('role') === 'group')
    expect(groups).toHaveLength(1)
    const group = groups[0]!
    expect(group.getAttribute('aria-label')).toBe('Declutter')
    const radios = group.descendants().filter(child => child.tagName === 'button')
    expect(radios.map(radio => radio.dataset.entry)).toEqual([
      'declutter-full', 'declutter-arrows', 'declutter-triangles', 'declutter-elements', 'declutter-classes',
    ])
    expect(radios.map(radio => radio.getAttribute('role'))).toEqual(Array(5).fill('menuitemradio'))
    expect(radios.map(accessibleText)).toEqual(['Full', 'Arrows', 'Remove arrows', 'Elements', 'Classes'])
    expect(radios.map(radio => radio.getAttribute('aria-checked'))).toEqual(['true', 'false', 'false', 'false', 'false'])
    expect(h.menuElement.descendants().filter(child => child.getAttribute('role') === 'menuitemcheckbox')).toHaveLength(0)
    h.menu.dispose()
    h.controller.close()
  })

  it('fits the view from the menu, like Ctrl+0', async () => {
    const h = await harness()
    h.controller.setViewport(800, 600)
    await flush()
    h.actions.length = 0
    h.menu.open()
    h.item('fit').fire('click')
    await flush()
    const camera = h.actions.find(entry => entry.action.action === 'camera')
    expect(camera).toBeTruthy()
    h.menu.dispose()
    h.controller.close()
  })

  it('writes an accepted selection back before the next state event', async () => {
    const h = await harness()
    h.menu.open()
    h.item('declutter-triangles').fire('click')
    h.ack.resolve({ ok: true, value: { accepted: true, reason: null } })
    await flush()
    h.menu.open()
    expect(h.item('declutter-triangles').getAttribute('aria-checked')).toBe('true')
    h.emit({ kind: 'state', state: stateWith(':elements'), breadcrumbs: [{ id: null, name: 'Shop' }], status: null, proposals: [] })
    await flush()
    h.menu.close()
    h.menu.open()
    expect(h.item('declutter-triangles').getAttribute('aria-checked')).toBe('false')
    expect(h.item('declutter-elements').getAttribute('aria-checked')).toBe('true')
    h.menu.dispose()
    h.controller.close()
  })

  it('saves exactly once after Remove arrows is accepted, naming the mode', async () => {
    const h = await harness()
    h.menu.open()
    h.item('declutter-triangles').fire('click')
    expect(h.menu.isOpen()).toBe(false)
    expect(h.actions.map(entry => entry.action.action)).toEqual(['declutter'])
    expect(h.actions[0]!.action).toEqual({ action: 'declutter', mode: 'triangles' })
    h.ack.resolve({ ok: true, value: { accepted: true, reason: null } })
    await flush()
    expect(h.actions).toEqual([
      { sessionId: 'session-a', action: { action: 'declutter', mode: 'triangles' } },
      { sessionId: 'session-a', action: { action: 'save' } },
    ])
    h.menu.dispose()
    h.controller.close()
  })

  it.each([
    { accepted: false, reason: 'refused' },
    { ok: false, error: { message: 'remote failure' } },
    null,
  ])('does not save after a non-success acknowledgement (%j)', async result => {
    const h = await harness()
    h.menu.open()
    h.item('declutter-triangles').fire('click')
    h.ack.resolve(result)
    await flush()
    expect(h.actions.map(entry => entry.action.action)).toEqual(['declutter'])
    h.menu.open()
    expect(h.item('declutter-full').getAttribute('aria-checked')).toBe('true')
    expect(h.item('declutter-triangles').getAttribute('aria-checked')).toBe('false')
    h.menu.dispose()
    h.controller.close()
  })

  it('handles a rejected declutter promise without saving', async () => {
    const h = await harness()
    h.menu.open()
    h.item('declutter-triangles').fire('click')
    h.ack.reject(new Error('declutter unavailable'))
    await flush()
    expect(h.actions.map(entry => entry.action.action)).toEqual(['declutter'])
    h.menu.open()
    expect(h.item('declutter-full').getAttribute('aria-checked')).toBe('true')
    expect(h.item('declutter-triangles').getAttribute('aria-checked')).toBe('false')
    h.menu.dispose()
    h.controller.close()
  })

  it('handles a rejected follow-up save without leaving the panel stuck', async () => {
    const h = await harness({ rejectSave: true })
    h.menu.open()
    h.item('declutter-triangles').fire('click')
    h.ack.resolve({ ok: true, value: { accepted: true, reason: null } })
    await flush()
    expect(h.actions.map(entry => entry.action.action)).toEqual(['declutter', 'save'])
    h.menu.dispose()
    h.controller.close()
  })

  it('sets a non-adjacent mode directly instead of toggling from restored state', async () => {
    const restored = await harness({ declutter: ':classes' })
    restored.menu.open()
    restored.item('declutter-arrows').fire('click')
    expect(restored.actions.at(-1)!.action).toEqual({ action: 'declutter', mode: 'arrows' })
    restored.menu.dispose()
    restored.controller.close()

    const triangles = await harness({ declutter: ':triangles' })
    triangles.menu.open()
    triangles.item('declutter-full').fire('click')
    expect(triangles.actions.at(-1)!.action).toEqual({ action: 'declutter', mode: 'full' })
    triangles.menu.dispose()
    triangles.controller.close()
  })

  it.each(DECLUTTER_MODES)('marks restored %s mode as the only checked choice', async mode => {
    const h = await harness({ declutter: `:${mode}` })
    h.menu.open()
    expect(DECLUTTER_MODES.map(candidate => h.item(`declutter-${candidate}`).getAttribute('aria-checked')))
      .toEqual(DECLUTTER_MODES.map(candidate => String(candidate === mode)))
    h.menu.dispose()
    h.controller.close()
  })

  it('localizes the group label and every mode choice', async () => {
    const h = await harness({ text: CHINESE_TEXT })
    h.menu.open()
    const group = h.menuElement.descendants().find(child => child.getAttribute('role') === 'group')!
    expect(group.getAttribute('aria-label')).toBe('精简模式')
    const radios = group.descendants().filter(child => child.tagName === 'button')
    expect(radios.map(accessibleText)).toEqual(['完整', '箭头', '移除箭头', '元素', '类'])
    h.menu.dispose()
    h.controller.close()
  })

  it('rebuilds its items on every open, so no stale copy accumulates', async () => {
    const h = await harness()
    for (let i = 0; i < 3; i += 1) {
      h.menu.open()
      h.menu.close()
    }
    h.menu.open()
    const items = h.menuElement.descendants().filter(child => child.tagName === 'button')
    expect(items.map(item => item.dataset.entry)).toEqual([
      'fit', 'declutter-full', 'declutter-arrows', 'declutter-triangles', 'declutter-elements', 'declutter-classes',
    ])
    h.menu.dispose()
    h.controller.close()
  })

  it('closes when its own button is pressed again, and on a press outside', async () => {
    const h = await harness()
    h.trigger.fire('click')
    expect(h.menu.isOpen()).toBe(true)
    h.trigger.fire('pointerdown')
    h.trigger.fire('click')
    expect(h.menu.isOpen()).toBe(false)
    h.menu.open()
    expect(h.menu.isOpen()).toBe(true)
    h.host.fire('pointerdown')
    expect(h.menu.isOpen()).toBe(false)
    h.menu.dispose()
    h.controller.close()
  })

  it('closes on Escape and returns the keyboard to its button', async () => {
    const h = await harness()
    h.menu.open()
    expect(h.menu.isOpen()).toBe(true)
    const event = h.menuElement.fire('keydown', { key: 'Escape' })
    expect(event.defaultPrevented).toBe(true)
    expect(h.menu.isOpen()).toBe(false)
    expect(h.menuElement.ownerDocument.activeElement).toBe(h.trigger)
    h.menu.dispose()
    h.controller.close()
  })

  it('removes its element and listeners on dispose', async () => {
    const h = await harness()
    h.menu.dispose()
    expect(h.menuElement.parent).toBeNull()
    expect(h.menu.isOpen()).toBe(false)
    h.controller.close()
  })
})
