import { readFileSync } from 'node:fs'
import { describe, expect, it } from 'vitest'
import type { PanelAction, SceneFrame } from '@dsh-uml-viewer/contracts'
import { cssToWorld } from '../src/fit.js'
import { hitTest } from '../src/paint.js'
import { mountQualityMenu } from '../src/requests.js'

const frame = JSON.parse(readFileSync(new URL('../../../crates/frame/tests/fixtures/frames/root.frame.json', import.meta.url), 'utf8')) as SceneFrame

/** The first canvas point that hits an element with a menu, in canvas pixels. */
const at = (() => {
  const bounds = { width: frame.viewport.width, height: frame.viewport.height }
  for (let y = 20; y < bounds.height; y += 20) {
    for (let x = 20; x < bounds.width; x += 20) {
      const world = cssToWorld(frame, bounds, x, y)
      if (!world) continue
      const id = hitTest(frame, world)
      const element = frame.world.find(item => (item.kind === 'class' || item.kind === 'package') && item.id === id)
      if (element && !(element.kind === 'class' && element.shape === 'oval')) return { x, y }
    }
  }
  throw new Error('the fixture has no element with a menu')
})()

function fakeMenuElement() {
  const attributes = new Map<string, string>()
  const listeners = new Map<string, Set<(event: never) => void>>()
  const element = {
    className: '', textContent: '', type: '', hidden: false,
    style: {} as Record<string, string>, children: [] as unknown[], removed: false,
    appendChild(child: unknown) { element.children.push(child) },
    setAttribute(name: string, value: string) { attributes.set(name, value) },
    getAttribute(name: string) { return attributes.get(name) ?? null },
    removeAttribute(name: string) { attributes.delete(name) },
    hasAttribute(name: string) { return attributes.has(name) },
    addEventListener(name: string, listener: (event: never) => void) {
      const set = listeners.get(name) ?? new Set()
      set.add(listener)
      listeners.set(name, set)
    },
    removeEventListener(name: string, listener: (event: never) => void) { listeners.get(name)?.delete(listener) },
    contains() { return false },
    remove() { element.removed = true },
    fire(name: string, event: unknown) { for (const listener of listeners.get(name) ?? []) listener(event as never) },
  }
  return element
}

function harness() {
  // `mountQualityMenu` builds its markup through `document.createElement`.
  const created: ReturnType<typeof fakeMenuElement>[] = []
  ;(globalThis as { document?: { createElement(name: string): unknown } }).document = {
    createElement: () => {
      const element = fakeMenuElement()
      created.push(element)
      return element
    },
  }
  const canvasListeners = new Map<string, Set<(event: never) => void>>()
  const hostListeners = new Map<string, Set<(event: never) => void>>()
  const record = (map: Map<string, Set<(event: never) => void>>) => ({
    addEventListener(name: string, listener: (event: never) => void) {
      const set = map.get(name) ?? new Set()
      set.add(listener)
      map.set(name, set)
    },
    removeEventListener(name: string, listener: (event: never) => void) { map.get(name)?.delete(listener) },
    fire(name: string, event: unknown) { for (const listener of map.get(name) ?? []) listener(event as never) },
  })
  const canvas = {
    ...record(canvasListeners), offsetTop: 0,
    getBoundingClientRect: () => ({ left: 0, top: 0, width: 1220, height: 884 }),
  }
  const host = { ...record(hostListeners), appendChild() {} }
  const actions: PanelAction[] = []
  const dispose = mountQualityMenu(host as unknown as HTMLElement, canvas as unknown as HTMLCanvasElement, {
    frame: () => frame,
    send: async action => { actions.push(action); return { accepted: true, reason: null } },
    report() {},
  })
  const menu = created[0]!
  const contextMenu = () => canvas.fire('contextmenu', {
    preventDefault() {}, offsetX: at.x, offsetY: at.y,
  })
  return { actions, contextMenu, dispose, escape: () => host.fire('keydown', {
    key: 'Escape', isComposing: false, preventDefault() {}, stopPropagation() {},
  }), menu, pointerdown: () => host.fire('pointerdown', { target: null }) }
}

/**
 * The host's shortcut service reads every `[role="menu"]` in the document as an
 * open modal, whether or not it is visible, and a modal blocks commands that do
 * not admit it — the tab refresh among them (#43). So the menu may only carry
 * that role while it is open.
 */
describe('element menu role', () => {
  it('exposes role="menu" only while open', () => {
    const h = harness()
    expect(h.menu.hasAttribute('role')).toBe(false)
    h.contextMenu()
    expect(h.menu.getAttribute('role')).toBe('menu')
    expect(h.menu.hidden).toBe(false)
    h.escape()
    expect(h.menu.hasAttribute('role')).toBe(false)
    expect(h.menu.hidden).toBe(true)
    h.dispose()
  })

  it('drops the role on every close path', async () => {
    const h = harness()
    h.contextMenu()
    h.pointerdown()
    expect(h.menu.hasAttribute('role')).toBe(false)
    h.contextMenu()
    const first = h.menu.children[0] as ReturnType<typeof fakeMenuElement>
    first.fire('click', {})
    await new Promise(resolve => setImmediate(resolve))
    expect(h.actions.map(action => action.action)).toContain('quality-request')
    expect(h.menu.hasAttribute('role')).toBe(false)
    h.dispose()
  })
})
