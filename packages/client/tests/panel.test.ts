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
  ended(): boolean
  emit(event: PanelEvent): void
  finish(): void
}

function harness(initial: PanelEvent[] = []): Harness {
  const actions: Harness['actions'] = []
  const statuses: PanelStatus[] = []
  const painted: SceneFrame[] = []
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
    h.controller.wheel(380, 290, -1)
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
