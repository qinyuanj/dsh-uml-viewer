import { describe, expect, it } from 'vitest'
import type { SceneFrame } from '@dsh-uml-viewer/contracts'
import { cssToWorld, viewTransform, worldToScreen } from '../src/fit.js'

/** #27 P2: `paintFrame` sizes the bitmap from `frame.viewport`; CSS stretches it
 * per axis to the canvas box. Double-click must invert exactly that. */
function frame(viewport: { width: number; height: number }, camera: { x: number; y: number; zoom: number }): SceneFrame {
  return { viewport, camera, style: { minTextPx: 5 } } as unknown as SceneFrame
}

/** Where a world point is painted in CSS pixels: world → frame viewport → CSS stretch. */
function paintedCss(f: SceneFrame, bounds: { width: number; height: number }, x: number, y: number): [number, number] {
  const [fx, fy] = worldToScreen(viewTransform(f, f.viewport.width, f.viewport.height), x, y)
  return [fx * bounds.width / f.viewport.width, fy * bounds.height / f.viewport.height]
}

describe('double-click CSS → world mapping', () => {
  it('maps the reviewed 1000x500 box over a 500x500 frame per axis', () => {
    const f = frame({ width: 500, height: 500 }, { x: 0, y: 0, zoom: 1 })
    const bounds = { width: 1000, height: 500 }
    // world x100 paints at CSS x200; clicking there must send world x100.
    expect(paintedCss(f, bounds, 100, 100)).toEqual([200, 100])
    expect(cssToWorld(f, bounds, 200, 100)).toEqual([100, 100])
  })

  it('inverts horizontal and vertical stretch with camera offset and zoom', () => {
    const cases = [
      { viewport: { width: 500, height: 500 }, bounds: { width: 1000, height: 500 }, camera: { x: 40, y: -30, zoom: 2 } },
      { viewport: { width: 800, height: 300 }, bounds: { width: 400, height: 900 }, camera: { x: -120, y: 75, zoom: 0.5 } },
      { viewport: { width: 640, height: 480 }, bounds: { width: 640, height: 480 }, camera: { x: 10, y: 20, zoom: 1.25 } },
    ]
    for (const { viewport, bounds, camera } of cases) {
      const f = frame(viewport, camera)
      for (const [wx, wy] of [[100, 50], [-20, 310], [camera.x, camera.y]] as const) {
        const [cx, cy] = paintedCss(f, bounds, wx, wy)
        const world = cssToWorld(f, bounds, cx, cy)!
        expect(world[0]).toBeCloseTo(wx, 9)
        expect(world[1]).toBeCloseTo(wy, 9)
      }
    }
  })

  it('does not use one uniform min scale', () => {
    const f = frame({ width: 500, height: 500 }, { x: 0, y: 0, zoom: 1 })
    // The rejected handler divided by min(2,1)=1 and sent x200.
    expect(cssToWorld(f, { width: 1000, height: 500 }, 200, 100)![0]).not.toBe(200)
  })

  it('has no mapping for a collapsed box', () => {
    const f = frame({ width: 500, height: 500 }, { x: 0, y: 0, zoom: 1 })
    expect(cssToWorld(f, { width: 0, height: 500 }, 10, 10)).toBeNull()
    expect(cssToWorld(f, { width: 500, height: 0 }, 10, 10)).toBeNull()
  })
})
