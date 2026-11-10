/// <reference path="../../host/src/text-modules.d.ts" />
import { readFileSync } from 'node:fs'
import { afterEach, describe, expect, it, vi } from 'vitest'
import type { PanelAction, PanelEvent, SceneFrame } from '@dsh-uml-viewer/contracts'
import { Element, makeDocument } from './dom.js'
import { ResidentConnection, ResidentPool, ResidentSessions } from '../../host/src/resident.js'
import { ViewerService } from '../../host/src/viewer.js'
import type { Agent, HostServices, SubprocessHandle } from '../../host/src/dsh.js'
import type { SessionStates } from '../../host/src/state.js'

vi.mock('react', () => ({ createElement: vi.fn() }))
vi.mock('../src/font.js', () => ({ FONT_FAMILY: 'test', loadSarasa: async () => {} }))
import { mountPanel, type ClientContext } from '../src/index.js'

const golden = JSON.parse(readFileSync(new URL('../../../crates/frame/tests/fixtures/frames/root.frame.json', import.meta.url), 'utf8')) as SceneFrame

const actions: PanelAction[] = []
let pending: ((event: IteratorResult<PanelEvent>) => void) | undefined
let bridge: any = null
const api = {
  watch: (...args: unknown[]): any => bridge ? bridge.watch(...args) : ({
    [Symbol.asyncIterator]() { return { next: () => new Promise<IteratorResult<PanelEvent>>(resolve => { pending = resolve }) } },
    end() { pending?.({ done: true, value: undefined }) },
  }),
  async action(_sessionId: string, action: PanelAction) { if (bridge) return bridge.action(_sessionId, action); actions.push(action); return { accepted: true, reason: null } },
  async close(sessionId: string) { return bridge?.close(sessionId) },
  watchOwned(sessionId: string, owner: any) { return bridge?.watchOwned ? bridge.watchOwned(sessionId, owner) : api.watch(sessionId) },
  async closeOwned(sessionId: string, owner: any) { return bridge?.closeOwned ? bridge.closeOwned(sessionId, owner) : api.close(sessionId) },
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
afterEach(() => { dispose?.(); dispose = undefined; bridge = null; vi.unstubAllGlobals() })

function camera() { return actions.filter(action => action.action === 'camera').at(-1) }

describe('mounted resident lifetime (#32 D13)', () => {
  it.each(['close-first', 'release-first', 'overlap'] as const)('actual disposer/new mount retains the resident despite stale legacy close: %s', async order => {
    const pool = new ResidentPool()
    const connection = new ResidentConnection('session', 'fixture', {
      done: Promise.resolve({ exitCode: 0, signal: null }), terminate() {},
    } as unknown as SubprocessHandle, false)
    const requests: string[] = []
    connection.request = async (action: { action: string }) => {
      requests.push(action.action)
      return { version: 1, seq: null, event: 'error', message: 'memory fixture' }
    }
    pool.connections.set('session', connection)
    const sessions = new ResidentSessions({ sandboxPolicy: { resolve: () => ({ mode: 'danger-full-access', workspaceRoot: process.cwd() }) } } as unknown as HostServices,
      {}, { read: async () => null } as unknown as SessionStates, pool)
    const agent = { session: { id: 'session' } } as Agent
    const viewer = new ViewerService(sessions, { openPanel() {}, closePanel() {} } as any, {} as any)
    const owners: { pageId: string; holderId: string }[] = []
    const aborts: AbortController[] = []
    const closes: string[] = []
    bridge = {
      watchOwned(_sessionId: string, owner: { pageId: string; holderId: string }) {
        owners.push(owner)
        const abort = new AbortController()
        aborts.push(abort)
        const stream = viewer.watchOwned(agent, owner, abort.signal)[Symbol.asyncIterator]()
        return { [Symbol.asyncIterator]: () => stream,
          // Pinned gateway: end() only half-closes the uplink; dispose() aborts
          // the generation and returns the iterator. close-first defers that.
          end() {}, dispose() { if (order !== 'close-first') abort.abort() } }
      },
      async action() { return { accepted: true, reason: null } },
      async close() { closes.push('legacy'); return viewer.close(agent) },
      async closeOwned(_sessionId: string, owner: { pageId: string; holderId: string }) { closes.push(owner.holderId); return viewer.closeOwned(agent, owner) },
    }
    const doc = makeDocument()
    vi.stubGlobal('document', doc)
    const first = await mountPanel(new Element('host', doc) as unknown as HTMLElement, { sessionId: 'session', ctx })
    await settled()
    expect(connection.refs).toBe(1)
    let second: (() => void) | undefined
    const mountAgain = () => mountPanel(new Element('host', doc) as unknown as HTMLElement, { sessionId: 'session', ctx })
    if (order === 'overlap') { second = await mountAgain(); await settled(); expect(connection.refs).toBe(2) }
    first()
    await settled()
    expect(closes).toEqual([]) // Detach never claims user close intent.
    // An older additive Remote may still deliver its unscoped close in either
    // order. It cannot target or poison the new owned stream.
    expect(await viewer.close(agent)).toEqual({ closed: false })
    aborts[0]!.abort()
    await settled()
    expect(requests).not.toContain('quit')
    expect(pool.connections.get('session')).toBe(connection)
    if (order !== 'overlap') expect(pool.graces.size).toBe(1)
    second ??= await mountAgain()
    await settled()
    expect(owners[0]!.pageId).toBe(owners[1]!.pageId)
    expect(owners[0]!.holderId).not.toBe(owners[1]!.holderId)
    expect(pool.connections.get('session')).toBe(connection)
    expect(connection.refs).toBe(1)
    expect(pool.graces.size).toBe(0)
    expect(await viewer.closeOwned(agent, owners[0]!)).toEqual({ closed: order === 'overlap' })
    second()
    aborts[1]!.abort()
    await settled()
    expect(pool.connections.get('session')).toBe(connection)
    expect(pool.graces.size).toBe(1)
    expect(requests).not.toContain('quit')
    await sessions.dispose()
    expect(connection.refs).toBe(0)
    expect(connection.holders.size).toBe(0)
    expect(pool.graces.size).toBe(0)
  })
})

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

async function settled(): Promise<void> {
  for (let i = 0; i < 10; i += 1) await Promise.resolve()
}

describe('mounted panel #23 + #27 union', () => {
  it('double-click sends one per-axis open-card and never a second drill-in', async () => {
    const { canvas } = await mount()
    canvas.fire('dblclick', { offsetX: 400, offsetY: 300 })
    await settled()
    expect(actions.filter(action => action.action === 'drill-in')).toEqual([])
    expect(actions.filter(action => action.action === 'open-card')).toEqual([{ action: 'open-card', at: [600, 500] }])
  })

  it('P sends exactly one namespace-tree, and none while composing', async () => {
    const { canvas } = await mount()
    canvas.fire('keydown', { key: 'p', isComposing: true })
    await settled()
    expect(actions.filter(action => action.action === 'namespace-tree')).toEqual([])
    expect(canvas.fire('keydown', { key: 'p' }).defaultPrevented).toBe(true)
    await settled()
    expect(actions.filter(action => action.action === 'namespace-tree')).toEqual([{ action: 'namespace-tree' }])
  })
})

describe('mounted panel quality menu (#28)', () => {
  it('offers all three refreshes on a component; each click sends its bound element only', async () => {
    const { host, canvas } = await mount()
    const draft = new Element('textarea', canvas.ownerDocument)
    draft.value = 'my unfinished draft'
    draft.focus()
    const event = canvas.fire('contextmenu', { offsetX: 400, offsetY: 300 })
    expect(event.defaultPrevented).toBe(true)
    const menu = host.descendants().find(el => el.className.includes('dsh-uml-quality-menu'))!
    expect(menu).toBeDefined()
    expect(menu.children.map(el => el.textContent)).toEqual(['Refresh CRAP', 'Differential mutation', 'Full mutation (--mutate-all)', 'Hide'])
    const id = golden.world[0]!.kind === 'package' ? golden.world[0]!.id : ':core'
    for (let i = 0; i < 3; i += 1) {
      canvas.fire('contextmenu', { offsetX: 400, offsetY: 300 })
      menu.children[i]!.fire('click')
    }
    await settled()
    expect(actions.filter(a => a.action === 'quality-request')).toEqual([
      { action: 'quality-request', request: 'refresh-crap', id },
      { action: 'quality-request', request: 'refresh-mutate', id },
      { action: 'quality-request', request: 'refresh-mutate-all', id },
    ])
    expect(draft.value).toBe('my unfinished draft')
    expect(draft.ownerDocument.activeElement).toBe(draft)
    expect(menu.hidden).toBe(true)
  })

  it('hides the bound element with one omit request (#29), leaving the draft alone', async () => {
    const { host, canvas } = await mount()
    const draft = new Element('textarea', canvas.ownerDocument)
    draft.value = 'my unfinished draft'
    draft.focus()
    canvas.fire('contextmenu', { offsetX: 400, offsetY: 300 })
    const menu = host.descendants().find(el => el.className.includes('dsh-uml-quality-menu'))!
    const id = golden.world[0]!.kind === 'package' ? golden.world[0]!.id : ':core'
    menu.children[3]!.fire('click')
    await settled()
    expect(actions.filter(a => a.action === 'omit')).toEqual([{ action: 'omit', id }])
    expect(actions.filter(a => a.action === 'quality-request')).toEqual([])
    expect(draft.value).toBe('my unfinished draft')
    expect(draft.ownerDocument.activeElement).toBe(draft)
    expect(menu.hidden).toBe(true)
  })

  it('offers no menu on a foreign package ellipse or a dependency port', async () => {
    const { host, canvas } = await mount()
    // A flat frame whose only hit targets are a foreign 椭圆 and a port.
    const frame: SceneFrame = { ...golden, waiting: false, viewport: { width: 800, height: 600 }, camera: { x: 0, y: 0, zoom: 1 },
      world: [
        { kind: 'class', lod: 'blocks', id: ':react', owner: null, rect: { x: 10, y: 10, w: 100, h: 40 }, shape: 'oval',
          band: 'none', fill: { role: 'foreign-fill' }, stroke: { role: 'foreign-stroke' }, strokeWidth: 1, grade: null,
          missing: false, selected: false, hovered: false, related: false, bounds: { x: 10, y: 10, w: 100, h: 40 }, dimmed: false },
        { kind: 'port', lod: 'full', id: ':outside', owner: ':core', rect: { x: 200, y: 10, w: 40, h: 18 }, text: 'x',
          marked: false, fill: { role: 'port-fill' }, stroke: { role: 'port-outline' }, strokeWidth: 1, ink: { role: 'muted' },
          bounds: { x: 200, y: 10, w: 40, h: 18 }, dimmed: false },
      ] as SceneFrame['world'] }
    await emit({ kind: 'frame', seq: 2, frame })
    const menu = host.descendants().find(el => el.className.includes('dsh-uml-quality-menu'))!
    canvas.fire('contextmenu', { offsetX: 50, offsetY: 25 })
    expect(menu.hidden).toBe(true)
    canvas.fire('contextmenu', { offsetX: 210, offsetY: 15 })
    expect(menu.hidden).toBe(true)
    expect(actions.filter(a => a.action === 'quality-request')).toEqual([])
  })

  it('does not open a menu on blank canvas and closes with Escape', async () => {
    const { host, canvas } = await mount()
    canvas.fire('contextmenu', { offsetX: 400, offsetY: 300 })
    const menu = host.descendants().find(el => el.className.includes('dsh-uml-quality-menu'))!
    expect(menu.hidden).toBe(false)
    host.fire('keydown', { key: 'Escape' })
    expect(menu.hidden).toBe(true)
    canvas.fire('contextmenu', { offsetX: 6000, offsetY: 6000 })
    expect(menu.hidden).toBe(true)
    expect(actions.filter(a => a.action === 'quality-request')).toEqual([])
  })
})

describe('mounted panel language (#42)', () => {
  /** DSH's `ctx.locale` face: the active id and a change subscription. */
  function dshLocale(active: string) {
    const listeners = new Set<() => void>()
    return {
      getLocale: () => ({ active, locales: [], revision: 0 }),
      subscribe(fn: () => void) { listeners.add(fn); return () => { listeners.delete(fn) } },
      switch(next: string) { active = next; for (const fn of [...listeners]) fn() },
    }
  }

  it('labels DOM text and accessible names in the DSH language and follows a switch without remounting', async () => {
    const locale = dshLocale('zh-CN')
    const localized = { get: (name: string) => name === 'locale' ? locale : ctx.get(name) } as unknown as ClientContext
    const doc = makeDocument()
    vi.stubGlobal('document', doc)
    const host = new Element('host', doc)
    dispose = await mountPanel(host as unknown as HTMLElement, { sessionId: 'session', ctx: localized })
    await emit({ kind: 'frame', seq: 1, frame: { ...golden, viewport: { width: 800, height: 600 }, camera: { x: 200, y: 200, zoom: 1 },
      world: [{ ...golden.world[0]!, bounds: { x: 0, y: 0, w: 4000, h: 4000 } }] } })
    const menu = host.descendants().find(el => el.className.includes('dsh-uml-quality-menu'))!
    const input = host.find('input')
    const canvas = host.find('canvas')
    expect(menu.children.map(el => el.textContent)).toEqual(['刷新 CRAP', '差量 mutation', '全量 mutation（--mutate-all）', '隐藏'])
    expect(menu.attributes['aria-label']).toBe('元素请求')
    expect(input.attributes['aria-label']).toBe('搜索类')
    expect(canvas.attributes['aria-label']).toBe('UML 图画布')

    locale.switch('en')
    expect(menu.children.map(el => el.textContent)).toEqual(['Refresh CRAP', 'Differential mutation', 'Full mutation (--mutate-all)', 'Hide'])
    expect(menu.attributes['aria-label']).toBe('Element requests')
    expect(input.attributes['aria-label']).toBe('Search classes')
    expect(canvas.attributes['aria-label']).toBe('UML diagram canvas')

    locale.switch('fr')
    expect(input.attributes['aria-label']).toBe('Search classes')
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
