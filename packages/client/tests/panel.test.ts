import { readFileSync } from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'
import { describe, expect, it } from 'vitest'
import type { PanelAction, PanelEvent, SceneFrame } from '@dsh-uml-viewer/contracts'
import { PanelController, type PanelRemoteApi, type PanelStatus } from '../src/panel.js'
import { screenToWorld, viewTransform, worldToScreen } from '../src/fit.js'
import { fakeCanvas, FakeMutationObserver } from './fakes.js'

const framesDir = path.resolve(fileURLToPath(new URL('../../../crates/frame/tests/fixtures/frames', import.meta.url)))
const frame = JSON.parse(readFileSync(path.join(framesDir, 'root.frame.json'), 'utf8')) as SceneFrame

interface Harness {
  controller: PanelController
  actions: { sessionId: string; action: PanelAction }[]
  statuses: PanelStatus[]
  painted: SceneFrame[]
  /** The sessions this panel reported closed, in order. */
  closes: string[]
  ended(): boolean
  emit(event: PanelEvent): void
  finish(): void
}

function harness(initial: PanelEvent[] = []): Harness {
  const actions: Harness['actions'] = []
  const statuses: PanelStatus[] = []
  const painted: SceneFrame[] = []
  const closes: string[] = []
  const queue: PanelEvent[] = [...initial]
  let waiting: ((value: IteratorResult<PanelEvent>) => void) | null = null
  let done = false
  let ended = false
  const stream = {
    [Symbol.asyncIterator](): AsyncIterator<PanelEvent> {
      return {
        next: async (): Promise<IteratorResult<PanelEvent>> => {
          const value = queue.shift()
          if (value !== undefined) return { done: false, value }
          if (done) return { done: true, value: undefined }
          return new Promise<IteratorResult<PanelEvent>>(resolve => {
            waiting = resolve
          })
        },
        return: async (): Promise<IteratorResult<PanelEvent>> => {
          done = true
          return { done: true, value: undefined }
        },
      }
    },
    end: () => { ended = true; done = true; waiting?.({ done: true, value: undefined }) },
    dispose: () => { ended = true },
  }
  const api: PanelRemoteApi = {
    watch: () => stream,
    async action(sessionId: string, action: PanelAction) {
      actions.push({ sessionId, action })
      return { ok: true, value: { accepted: true, reason: null } }
    },
    async close(sessionId: string) {
      closes.push(sessionId)
      return { closed: true }
    },
  }
  const controller = new PanelController({
    canvas: fakeCanvas({ width: 800, height: 600 }),
    api,
    paint: (value) => { painted.push(value) },
    onStatus: (status) => { statuses.push(status) },
  })
  return {
    controller,
    actions,
    statuses,
    painted,
    closes,
    ended: () => ended,
    emit(event: PanelEvent) {
      queue.push(event)
      const resolve = waiting
      waiting = null
      if (resolve) void Promise.resolve().then(() => resolve({ done: false, value: queue.shift()! }))
    },
    finish() {
      done = true
      waiting?.({ done: true, value: undefined })
    },
  }
}

async function settle(predicate: () => boolean, label: string): Promise<void> {
  for (let attempt = 0; attempt < 200; attempt += 1) {
    if (predicate()) return
    await new Promise(resolve => setTimeout(resolve, 1))
  }
  throw new Error(`timed out waiting for ${label}`)
}

describe('session-bound panel stream', () => {
  it('falls back to the legacy watch and close when an older host refuses the owned stream', async () => {
    const calls: string[] = []
    let finish: (() => void) | null = null
    const legacy = {
      async *[Symbol.asyncIterator]() {
        yield { kind: 'ready', actions: [] } as PanelEvent
        await new Promise<void>(resolve => { finish = resolve })
      },
      end: () => finish?.(),
    }
    const api: PanelRemoteApi = {
      watch: sessionId => { calls.push(`watch ${sessionId}`); return legacy },
      watchOwned: () => {
        calls.push('watchOwned')
        return { [Symbol.asyncIterator]: () => ({ next: async () => { throw new Error('gateway/method-not-found') } }) }
      },
      async closeOwned() { calls.push('closeOwned') },
      async close(sessionId) { calls.push(`close ${sessionId}`) },
      async action() { return { accepted: true, reason: null } },
    }
    const controller = new PanelController({ canvas: fakeCanvas({ width: 800, height: 600 }), api, paint: () => {},
      owner: () => ({ pageId: 'page', holderId: 'holder' }) })
    await controller.open('session-a')
    await settle(() => controller.current.kind === 'live', 'the legacy ready')
    expect(calls).toEqual(['watchOwned', 'watch session-a'])
    await controller.close()
    // The owner was never accepted, so the legacy close is the one that can work.
    expect(calls).toEqual(['watchOwned', 'watch session-a', 'close session-a'])
  })

  it('waits for its watch to acquire ownership before sending viewport actions', async () => {
    const h = harness()
    await h.controller.open('session-a')
    expect(h.actions).toEqual([])
    h.emit({ kind: 'ready', actions: [] })
    await settle(() => h.actions.length === 1, 'the owned watch ready')
    expect(h.actions[0]).toMatchObject({ sessionId: 'session-a', action: { action: 'viewport' } })
    h.controller.close()
  })

  it('paints each frame the session stream sends and reports live', async () => {
    const h = harness()
    await h.controller.open('session-a')
    h.emit({ kind: 'ready', actions: ['display', 'reload'] })
    h.emit({ kind: 'frame', seq: 1, frame })
    await settle(() => h.painted.length === 1, 'the first frame')
    expect(h.painted[0]).toBe(frame)
    expect(h.controller.current.kind).toBe('live')
    expect(h.controller.current.actions).toEqual(['display', 'reload'])
    expect(h.controller.current.frame).toBe(frame)
    // The panel names its viewport for this session before it can be laid out.
    await settle(() => h.actions.some(entry => entry.action.action === 'viewport'), 'the viewport action')
    expect(h.actions[0]).toMatchObject({ sessionId: 'session-a', action: { action: 'viewport', viewport: { width: 800, height: 600 } } })
    h.controller.close()
  })

  it('reports a close, but never a rebind of the same session, as closed', async () => {
    const h = harness([{ kind: 'ready', actions: [] }])
    await h.controller.open('session-a')
    await settle(() => h.controller.current.kind === 'live', 'the ready event')
    // The sidebar/main reparent rebinds the same session: nothing was closed,
    // so the resident the new stream re-attaches to must not be told to stop.
    await h.controller.open('session-a')
    expect(h.closes).toEqual([])
    // A different session is left behind: that one is closed.
    await h.controller.open('session-b')
    expect(h.closes).toEqual(['session-a'])
    h.controller.close()
    expect(h.closes).toEqual(['session-a', 'session-b'])
    // Closing an already closed panel says nothing twice.
    h.controller.close()
    expect(h.closes).toEqual(['session-a', 'session-b'])
  })

  it('shows the host’s refusal reason instead of an empty canvas', async () => {
    const h = harness([{ kind: 'refused', reason: 'read-only sandbox: uml_ir cannot run' }])
    await h.controller.open('session-a')
    await settle(() => h.controller.current.kind === 'refused', 'the refusal')
    expect(h.controller.current.reason).toMatch(/read-only/)
    expect(h.painted).toEqual([])
    h.controller.close()
  })

  it('reports a failed stream and stops painting after close', async () => {
    const h = harness([{ kind: 'failed', reason: 'resident exited' }])
    await h.controller.open('session-a')
    await settle(() => h.controller.current.kind === 'failed', 'the failure')
    expect(h.controller.current.reason).toBe('resident exited')
    h.controller.close()
    expect(h.ended()).toBe(true)
  })

  it('sends pan and select gestures to the host, never laying out locally', async () => {
    const h = harness([{ kind: 'ready', actions: [] }, { kind: 'frame', seq: 1, frame }])
    await h.controller.open('session-a')
    await settle(() => h.painted.length === 1, 'the frame')
    h.controller.pointerDown(400, 300)
    h.controller.pointerMove(380, 290)
    h.controller.pointerUp(380, 290)
    await settle(() => h.actions.some(entry => entry.action.action === 'camera'), 'the camera action')
    const camera = h.actions.find(entry => entry.action.action === 'camera')!.action
    expect(camera).toMatchObject({ action: 'camera', zoom: frame.camera.zoom })
    if (camera.action === 'camera') {
      // 20 screen px left at the frame's zoom moves the camera 20/zoom world units right.
      expect(camera.x).toBeCloseTo(frame.camera.x + 20 / frame.camera.zoom, 6)
      expect(camera.y).toBeCloseTo(frame.camera.y + 10 / frame.camera.zoom, 6)
    }
    // A press without motion is a select, not a pan.
    h.controller.pointerDown(410, 310)
    h.controller.pointerUp(410, 310)
    await settle(() => h.actions.some(entry => entry.action.action === 'select'), 'the select action')
    expect(h.actions.at(-1)).toMatchObject({ sessionId: 'session-a', action: { action: 'select' } })
    h.controller.close()
  })

  it('round-trips local pan and zoom through resident camera actions', async () => {
    const initial: SceneFrame = { ...frame, viewport: { width: 800, height: 600 } }
    const h = harness([{ kind: 'ready', actions: [] }, { kind: 'frame', seq: 1, frame: initial }])
    await h.controller.open('session-a')
    await settle(() => h.painted.length === 1, 'the frame')

    const world: [number, number] = [200, 180]
    const beforePan = worldToScreen(viewTransform(initial, 800, 600), ...world)
    h.controller.pointerDown(400, 300)
    h.controller.pointerMove(380, 290)
    h.controller.pointerUp(380, 290)
    await settle(() => h.actions.filter(entry => entry.action.action === 'camera').length === 1, 'the pan action')
    const pan = h.actions.filter(entry => entry.action.action === 'camera')[0]!.action
    if (pan.action !== 'camera') throw new Error('expected a camera action')
    const panned = h.controller.current.frame!
    expect(panned.camera).toMatchObject({ x: pan.x, y: pan.y, zoom: pan.zoom })
    const afterPan = worldToScreen(viewTransform(panned, 800, 600), ...world)
    expect(afterPan[0]).toBeCloseTo(beforePan[0] - 20, 10)
    expect(afterPan[1]).toBeCloseTo(beforePan[1] - 10, 10)

    const anchor = screenToWorld(viewTransform(panned, 800, 600), 380, 290)
    h.controller.wheel(380, 290, -1, { control: true })
    await settle(() => h.actions.filter(entry => entry.action.action === 'camera').length === 2, 'the zoom action')
    const zoom = h.actions.filter(entry => entry.action.action === 'camera')[1]!.action
    if (zoom.action !== 'camera') throw new Error('expected a camera action')
    const zoomed = h.controller.current.frame!
    expect(zoomed.camera).toMatchObject({ x: zoom.x, y: zoom.y, zoom: zoom.zoom })
    const afterZoom = worldToScreen(viewTransform(zoomed, 800, 600), ...anchor)
    expect(afterZoom[0]).toBeCloseTo(380, 10)
    expect(afterZoom[1]).toBeCloseTo(290, 10)
    // The resident acknowledges a camera action without sending another frame;
    // both local camera changes have therefore already been painted.
    expect(h.painted).toHaveLength(3)
    h.controller.close()
  })

  it('pans on a plain wheel and zooms only with control, like the egui window', async () => {
    const initial: SceneFrame = { ...frame, viewport: { width: 800, height: 600 },
      camera: { x: 100, y: 100, zoom: frame.camera.zoom } }
    const h = harness([{ kind: 'ready', actions: [] }, { kind: 'frame', seq: 1, frame: initial }])
    await h.controller.open('session-a')
    await settle(() => h.painted.length === 1, 'the frame')
    const y = initial.camera.y
    h.controller.wheel(400, 300, -1)
    await settle(() => h.actions.some(entry => entry.action.action === 'camera'), 'the wheel pan')
    expect(h.controller.current.frame!.camera.zoom).toBe(initial.camera.zoom)
    expect(h.controller.current.frame!.camera.y).toBeCloseTo(y + 48 / initial.camera.zoom, 6)
    h.controller.wheel(400, 300, 1, { control: true })
    await settle(() => h.controller.current.frame!.camera.zoom > initial.camera.zoom, 'the ctrl zoom')
    expect(h.controller.current.frame!.camera.zoom).toBeCloseTo(initial.camera.zoom * 1.1, 6)
    h.controller.close()
  })

  it('Ctrl+0 restores the resident fit including scene margins after local zoom and pan', async () => {
    // CI7 Deep: a 647×917 viewport, outer package (56,56,824,180), and
    // the resident's fresh-load camera. The scene size is 936×292 after
    // compose::bounds' 16-unit inflation and compose::size's 40-unit margin.
    const outer = frame.world.find(item => item.kind === 'package')!
    const rect = { x: 56, y: 56, w: 824, h: 180 }
    const fitted = { x: 0, y: -543.3415765069552, zoom: 0.6912393162393162 }
    const initial: SceneFrame = { ...frame, viewport: { width: 647, height: 917 },
      camera: fitted, world: [{ ...outer, bounds: rect, rect }], edges: [] }
    const h = harness([{ kind: 'frame', seq: 1, frame: initial }])
    await h.controller.open('session-a')
    await settle(() => h.painted.length === 1, 'the frame')
    h.controller.setViewport(647, 917)
    h.controller.wheel(194, 367, 1, { control: true })
    h.controller.pointerDown(130, 887)
    h.controller.pointerMove(30, 837)
    h.controller.pointerUp(30, 837)
    h.controller.wheel(323.5, 458.5, -1)
    expect(h.controller.current.frame!.camera).not.toEqual(fitted)
    expect(h.controller.key('0', { control: true })).toBe(true)
    expect(h.controller.current.frame!.camera).toEqual(fitted)
    expect(h.actions.at(-1)!.action).toEqual({ action: 'camera', ...fitted })
    h.controller.close()
  })

  it('sends double-click drill-in, Escape back and P namespace-tree to the host', async () => {
    const h = harness([{ kind: 'ready', actions: [] }, { kind: 'frame', seq: 1, frame }])
    await h.controller.open('session-a')
    await settle(() => h.painted.length === 1, 'the frame')
    const box = frame.world.find(item => item.kind === 'class' && item.id === ':core')!
    const rect = 'rect' in box ? box.rect : box.bounds!
    const at = worldToScreen(viewTransform(frame, 800, 600), rect.x + rect.w / 2, rect.y + rect.h / 2)
    h.controller.doubleClick(...at)
    await settle(() => h.actions.some(entry => entry.action.action === 'drill-in'), 'the drill-in')
    const drill = h.actions.find(entry => entry.action.action === 'drill-in')!
    if (drill.action.action !== 'drill-in') throw new Error('expected drill-in')
    expect(drill.action.at![0]).toBeCloseTo(rect.x + rect.w / 2, 6)
    expect(drill.action.at![1]).toBeCloseTo(rect.y + rect.h / 2, 6)

    // A drilled frame has the back chrome; Escape goes back one level.
    const drilled: SceneFrame = { ...frame, chrome: [...frame.chrome, { kind: 'back', text: '< back', at: [12, 8], ink: { role: 'accent' }, plate: { role: 'chrome-plate' } }] }
    h.emit({ kind: 'frame', seq: 2, frame: drilled })
    await settle(() => h.controller.current.frame === drilled, 'the drilled frame')
    expect(h.controller.key('Escape')).toBe(true)
    await settle(() => h.actions.some(entry => entry.action.action === 'back'), 'escape goes back')

    expect(h.controller.key('p')).toBe(true)
    await settle(() => h.actions.some(entry => entry.action.action === 'namespace-tree'), 'P returns to the tree')
    h.controller.close()
  })

  it('searches through the host, captures canvas keys and locates the chosen result', async () => {
    const h = harness([{ kind: 'ready', actions: [] }, { kind: 'frame', seq: 1, frame }])
    await h.controller.open('session-a')
    await settle(() => h.painted.length === 1, 'the frame')
    expect(h.controller.key('f', { control: true })).toBe(true)
    expect(h.controller.current.search.active).toBe(true)
    h.controller.setSearchQuery('price')
    await settle(() => h.actions.some(entry => entry.action.action === 'search'), 'the search action')
    h.emit({ kind: 'search', query: 'price', results: [
      { id: ':alpha.deep.price', name: 'price', ns: 'alpha.deep.price', label: 'price · alpha.deep' },
      { id: ':beta.price', name: 'price', ns: 'beta.price', label: 'price · beta' },
    ] })
    await settle(() => h.controller.current.search.results.length === 2, 'the results')
    // An open search box captures P, zoom and pointer gestures.
    expect(h.controller.key('p')).toBe(true)
    h.controller.wheel(400, 300, 1, { control: true })
    h.controller.pointerDown(400, 300)
    h.controller.pointerUp(400, 300)
    expect(h.actions.some(entry => entry.action.action === 'namespace-tree')).toBe(false)
    expect(h.actions.some(entry => entry.action.action === 'camera')).toBe(false)
    expect(h.actions.some(entry => entry.action.action === 'select')).toBe(false)

    h.controller.key('ArrowDown')
    expect(h.controller.current.search.selection).toBe(0)
    h.controller.key('ArrowDown')
    expect(h.controller.current.search.selection).toBe(1)
    h.controller.key('Enter')
    await settle(() => h.actions.some(entry => entry.action.action === 'locate'), 'locate the result')
    expect(h.actions.at(-1)!.action).toEqual({ action: 'locate', id: ':beta.price' })
    // Enter locates without closing; Escape closes the box, not the view.
    expect(h.controller.current.search.active).toBe(true)
    h.controller.key('Escape')
    expect(h.controller.current.search.active).toBe(false)
    expect(h.actions.some(entry => entry.action.action === 'back')).toBe(false)
    h.controller.close()
  })

  it('keeps the stream’s discussion context on the panel status', async () => {
    const h = harness([{ kind: 'ready', actions: [] }])
    await h.controller.open('session-a')
    h.emit({ kind: 'context', context: { kind: 'proposal', proposalId: ':p-shop', name: 'Shop proposal', edn: '{:context :proposal}' } })
    h.emit({ kind: 'frame', seq: 1, frame })
    await settle(() => h.painted.length === 1, 'the frame')
    expect(h.controller.current.context).toEqual({
      kind: 'proposal', proposalId: ':p-shop', name: 'Shop proposal', edn: '{:context :proposal}',
    })
    h.controller.close()
  })

  it('repaints the last frame when the host theme flips', async () => {
    const before = FakeMutationObserver.instances.length
    const h = harness([{ kind: 'frame', seq: 1, frame }])
    await h.controller.open('session-a')
    await settle(() => h.painted.length === 1, 'the frame')
    // The open panel is the one that started watching the theme attribute.
    expect(FakeMutationObserver.instances.length).toBe(before + 1)
    FakeMutationObserver.instances.at(-1)!.trigger()
    expect(h.painted.length).toBe(2)
    h.controller.close()
  })
})

describe('local hover (关系悬停高亮, ADR 0013)', () => {
  const coreCenter = (view: SceneFrame): [number, number] => {
    const core = view.world.find(i => i.kind === 'class' && i.id === ':core')
    if (!core || core.kind !== 'class') throw new Error(':core')
    const [fx, fy] = worldToScreen(viewTransform(view, 800, 600), core.rect.x + core.rect.w / 2, core.rect.y + core.rect.h / 2)
    return [fx * 800 / view.viewport.width, fy * 600 / view.viewport.height]
  }
  const warm = (painted: SceneFrame): string[] =>
    painted.edges.flatMap(e => (e.kind === 'stroke' && e.warm ? [`${e.from}->${e.to}:${e.stroke.role}`] : []))
  const expectedWarm = [':api->:core:relation-hover', ':store->:core:relation-hover', ':core->:util:violation-hot']

  it('paints the hovered class’s relations without a host round trip or a new resident frame', async () => {
    const h = harness([{ kind: 'ready', actions: [] }, { kind: 'frame', seq: 1, frame }])
    await h.controller.open('session-a')
    await settle(() => h.painted.length === 1, 'the frame')
    const sent = h.actions.length
    h.controller.hover(...coreCenter(frame))
    expect(warm(h.painted.at(-1)!)).toEqual(expectedWarm)
    // Hover is feedback only: no action, and the resident's frame is unchanged.
    expect(h.actions.length).toBe(sent)
    expect(h.controller.current.frame).toBe(frame)
    // Moving within the same box does not repaint; leaving the canvas restores the frame.
    const paints = h.painted.length
    h.controller.hover(coreCenter(frame)[0] + 2, coreCenter(frame)[1])
    expect(h.painted.length).toBe(paints)
    h.controller.hoverEnd()
    expect(h.painted.at(-1)).toBe(frame)
    h.controller.close()
  })

  it('moves the dependency tooltip with the pointer while staying on the same relation', async () => {
    const value: SceneFrame = { ...frame, hover: { complete: true, indicators: [], edges: [{
      from: ':api', to: ':serde', viaIds: [],
      rows: [{ text: 'api -> serde', violating: false, ink: { role: 'ink' } }],
    }] } }
    const h = harness([{ kind: 'frame', seq: 1, frame: value }])
    await h.controller.open('session-a')
    await settle(() => h.painted.length === 1, 'the frame')
    const edge = frame.edges.find(e => e.kind === 'stroke' && e.from === ':api' && e.to === ':serde')
    if (!edge || edge.kind !== 'stroke') throw new Error('edge')
    const [a, b] = [edge.points[0]!, edge.points[1]!]
    const [sx, sy] = worldToScreen(viewTransform(frame, 800, 600), (a[0] + b[0]) / 2, (a[1] + b[1]) / 2)
    const x = sx * 800 / frame.viewport.width, y = sy * 600 / frame.viewport.height
    h.controller.hover(x, y)
    const first = h.painted.at(-1)!.chrome.at(-1)
    expect(first?.kind).toBe('popup')
    const before = h.painted.length
    h.controller.hover(x + 0.25, y)
    expect(h.painted.length).toBe(before + 1)
    const second = h.painted.at(-1)!.chrome.at(-1)
    if (first?.kind !== 'popup' || second?.kind !== 'popup') throw new Error('popup')
    expect(second.rect.x).toBeGreaterThan(first.rect.x)
    h.controller.close()
  })

  it('keeps the hover across a local pan and over the next resident frame', async () => {
    const h = harness([{ kind: 'ready', actions: [] }, { kind: 'frame', seq: 1, frame }])
    await h.controller.open('session-a')
    await settle(() => h.painted.length === 1, 'the frame')
    h.controller.hover(...coreCenter(frame))
    // A wheel pan repaints the hovered frame with the new camera.
    h.controller.wheel(10, 10, -1)
    const panned = h.painted.at(-1)!
    expect(panned.camera).not.toEqual(frame.camera)
    expect(warm(panned)).toEqual(expectedWarm)
    // A select answer from the resident (say, a reload) is painted with the hover on top.
    const reloaded: SceneFrame = { ...frame }
    h.emit({ kind: 'frame', seq: 2, frame: reloaded })
    await settle(() => h.controller.current.frame === reloaded, 'the next frame')
    expect(warm(h.painted.at(-1)!)).toEqual(expectedWarm)
    expect(h.controller.current.frame).toBe(reloaded)
    h.controller.close()
  })
})
