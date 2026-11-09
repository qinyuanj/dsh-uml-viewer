/**
 * Relation edges (dsh-uml-viewer#25): the six relation kinds' head shapes,
 * the routed curves, the 移除箭头 compact dependency indicators and the
 * 违规关系色, painted from the producer goldens. Expected numbers come from
 * the deleted native painter itself (#30: crates/viewer/src/paint.rs paint_head,
 * crates/frame/src/render_cache.rs decimated + lod::simplify), never from
 * this module's own arithmetic. Head vertices below are the native f32
 * computation printed by a throwaway frame-crate test; decimated point
 * indices are lod::simplify's output.
 */
import { readFileSync } from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'
import { describe, expect, it } from 'vitest'
import type { SceneFrame } from '@dsh-uml-viewer/contracts'
import { paintFrame } from '../src/paint.js'
import { DARK_TOKENS, type DrawOp, fakeCanvas } from './fakes.js'

type Edge = SceneFrame['edges'][number]

/** The dark host palette for the relation roles (theme.ts token mapping). */
const RELATION_COLORS: Record<string, string> = {
  relation: '#9db8a8',
  'relation-hover': '#ecece4',
  // Hue-fixed like the egui window (frame::theme RELATED), not a host token.
  'dependency-chain': '#3e98ec',
  violation: '#c42a24',
  'violation-hot': '#ff4030',
  indicator: '#9db8a8',
  canvas: '#161c20',
  ink: '#ecece4',
}

const framesDir = path.resolve(fileURLToPath(new URL('../../../crates/frame/tests/fixtures/frames', import.meta.url)))

function golden(name: string): SceneFrame {
  return JSON.parse(readFileSync(path.join(framesDir, `${name}.frame.json`), 'utf8')) as SceneFrame
}

/** Paint one edge alone on its golden frame and return the draw ops (no clear). */
function paintEdge(frame: SceneFrame, edge: Edge, camera?: SceneFrame['camera']): DrawOp[] {
  const canvas = fakeCanvas({ tokens: DARK_TOKENS, dark: true })
  paintFrame({ ...frame, camera: camera ?? frame.camera, world: [], edges: [edge], chrome: [] }, canvas)
  return canvas.recording.ops.slice(canvas.recording.ops.findIndex(op => op.op === 'fillRect') + 1)
}

function edgesOf(frame: SceneFrame, predicate: (edge: Edge) => boolean): Edge[] {
  const edges = frame.edges.filter(predicate)
  if (edges.length === 0) throw new Error('the golden has no such edge')
  return edges
}

function one(frame: SceneFrame, predicate: (edge: Edge) => boolean, what: string): Edge {
  const found = edgesOf(frame, predicate)
  expect(found.length, what).toBe(1)
  return found[0]!
}

/** Every moveTo/lineTo since beginPath, per stroke/fill, in paint order. */
function paths(ops: DrawOp[]): { op: DrawOp; points: [number, number][]; closed: boolean }[] {
  const out: { op: DrawOp; points: [number, number][]; closed: boolean }[] = []
  let points: [number, number][] = []
  let closed = false
  for (const op of ops) {
    if (op.op === 'beginPath') { points = []; closed = false }
    else if (op.op === 'moveTo' || op.op === 'lineTo') points.push([op.x!, op.y!])
    else if (op.op === 'closePath') closed = true
    else if (op.op === 'fill' || op.op === 'stroke') out.push({ op, points, closed })
  }
  return out
}

function points(actual: [number, number][], expected: [number, number][], label: string): void {
  expect(actual.length, `${label} point count`).toBe(expected.length)
  actual.forEach(([x, y], index) => {
    expect(x, `${label}[${index}].x`).toBeCloseTo(expected[index]![0], 3)
    expect(y, `${label}[${index}].y`).toBeCloseTo(expected[index]![1], 3)
  })
}

/**
 * Native paint_head output (f32, egui's Pos2 arithmetic) for the heads of
 * core-selected: tip, wing A, wing C, and the diamond back vertex. Printed by
 * a throwaway frame-crate test from the golden tip/behind/camera, so these are
 * the window's own numbers rather than this suite's reimplementation.
 */
const NATIVE_HEADS: Record<string, { tip: [number, number]; a: [number, number]; c: [number, number]; back: [number, number] }> = {
  ':core.order→:core.price': { tip: [670, 537], a: [664.08026, 522.1354], c: [655.9104, 529.4182], back: [659.3533, 525.0565] },
  ':core.order→:core.cart': { tip: [514.8, 537], a: [520.27234, 521.9649], c: [509.32767, 521.9649], back: [514.8, 521] },
  ':core.cart→:core.price': { tip: [630, 593.4], a: [619.64777, 581.2004], c: [614.22797, 590.7088], back: [616.09955, 585.47675] },
  ':core.price→:core.cart': { tip: [590, 593.4], a: [605.77203, 590.7088], c: [600.35223, 581.2004], back: [603.90045, 585.47675] },
  ':core.cart→:core': { tip: [690, 339], a: [683.6315, 353.67795], c: [694.5563, 354.33755], back: [689.0357, 354.97092] },
}

describe('relation edges from the producer goldens', () => {
  const focused = golden('core-selected')

  it('draws the six relation kinds with the native head geometry and roles', () => {
    // The shop IR names these five visible relations in the core view.
    // The sixth, :store.sql→:core.order implements, folds to :store→:core at
    // root and is asserted separately below. No relation kind is synthetic.
    expect(edgesOf(focused, e => e.kind === 'head').map(e => (e.kind === 'head' ? e.head : '')).sort())
      .toEqual(['diamond', 'diamond-fill', 'open', 'open', 'triangle'])
    for (const edge of focused.edges) {
      if (edge.kind !== 'head') continue
      const ops = paintEdge(focused, edge)
      const drawn = paths(ops)
      const label = `${edge.from}→${edge.to} ${edge.head}`
      const native = NATIVE_HEADS[`${edge.from}→${edge.to}`]!
      // Native order: tip, wing A, (diamond back), wing C.
      const expected = edge.head === 'diamond' || edge.head === 'diamond-fill'
        ? [native.tip, native.a, native.back, native.c]
        : [native.tip, native.a, native.c]
      // Every role resolves exactly as the producer named it.
      const role = edge.stroke.role
      const color = RELATION_COLORS[role]!
      const alpha = edge.dimmed ? focused.style.dimAlpha / 255 : 1
      if (edge.head === 'open') {
        expect(drawn.map(d => d.op.op), label).toEqual(['stroke', 'stroke'])
        points(drawn[0]!.points, [expected[0]!, expected[1]!], `${label} wing A`)
        points(drawn[1]!.points, [expected[0]!, expected[2]!], `${label} wing C`)
        for (const d of drawn) {
          expect(d.op.strokeStyle, label).toBe(color)
          expect(d.op.lineWidth, label).toBe(edge.width)
          expect(d.op.globalAlpha, label).toBe(alpha)
          expect(d.closed, label).toBe(false)
        }
      } else {
        expect(drawn.map(d => d.op.op), label).toEqual(['fill', 'stroke'])
        points(drawn[0]!.points, expected, label)
        expect(drawn[0]!.closed, label).toBe(true)
        // Opaque interior, dimmed outline (the #16 rule), at this zoom too.
        expect(drawn[0]!.op.globalAlpha, `${label} interior`).toBe(1)
        expect(drawn[0]!.op.fillStyle, label).toBe(RELATION_COLORS[edge.fill.role]!)
        expect(drawn[1]!.op.strokeStyle, label).toBe(color)
        expect(drawn[1]!.op.lineWidth, label).toBe(edge.width)
        expect(drawn[1]!.op.globalAlpha, `${label} outline`).toBe(alpha)
      }
    }
  })

  it('paints the sixth kind, implements, as a hollow triangle at sub-unit zoom', () => {
    const root = golden('root')
    const triangle = one(root, e => e.kind === 'head' && e.head === 'triangle', 'implements triangle')
    expect(triangle).toMatchObject({ from: ':store', to: ':core', head: 'triangle', width: 1.6,
      stroke: { role: 'relation' }, fill: { role: 'canvas' } })
    const drawn = paths(paintEdge(root, triangle))
    // Native paint_head f32 output for :store→:core at zoom 0.8346456692913385.
    points(drawn[0]!.points, [
      [620.0652, 581.8583], [626.942, 570.4107], [617.9748, 568.6686],
    ], 'implements triangle')
    expect(drawn[0]!.op.fillStyle).toBe('#161c20')
    expect(drawn[0]!.op.strokeStyle).toBe('#9db8a8')
    expect(drawn[0]!.op.globalAlpha).toBe(1)
  })

  it('draws every producer stroke point of the full-tier curves, ends included', () => {
    const root = golden('root')
    // Both views together exercise every kind's producer-sampled curve;
    // neither the open heads nor the closed ones imply a straight shaft.
    for (const view of [focused, root]) for (const edge of edgesOf(view, e => e.kind === 'stroke')) {
      if (edge.kind !== 'stroke') throw new Error('expected a stroke')
      const drawn = paths(paintEdge(view, edge))
      expect(drawn.length).toBe(1)
      const expected = edge.points.map(([x, y]) =>
        [(x - view.camera.x) * view.camera.zoom, (y - view.camera.y) * view.camera.zoom] as [number, number])
      // The routed curve: :core→:util is a 41-point arc, not a straight line.
      points(drawn[0]!.points, expected, `${edge.from}→${edge.to}`)
      expect(drawn[0]!.closed).toBe(false)
      expect(drawn[0]!.op.lineWidth).toBe(edge.width)
      expect(drawn[0]!.op.lineDash).toEqual([])
      expect(drawn[0]!.op.strokeStyle).toBe(RELATION_COLORS[edge.stroke.role]!)
    }
    // A violating relation is 违规关系色 red and 2.4 pt wide, head included.
    const violating = one(root, e => e.kind === 'stroke' && e.stroke.role === 'violation', 'violating stroke')
    if (violating.kind !== 'stroke') throw new Error('expected a stroke')
    expect([violating.from, violating.to, violating.width]).toEqual([':core', ':util', 2.4])
    const head = one(root, e => e.kind === 'head' && e.from === ':core' && e.to === ':util', 'violating head')
    for (const d of paths(paintEdge(root, head))) expect(d.op.strokeStyle).toBe('#c42a24')
  })

  it('decimates frames-tier strokes exactly as the native render cache does', () => {
    const frame = golden('root-hover-frames')
    expect(frame.lod).toBe('frames')
    // Native `lod::simplify(points, 8/0.29730177875068026, 0.5/0.2973…)`
    // (quarter-octave step -7 for zoom 0.3), as printed by the frame crate.
    const kept: Record<string, number[]> = {
      ':core→:util': [0, 6, 13, 18, 24, 29, 37, 40],
      ':store→:serde': [0, 14, 21, 23],
      ':api→:serde': [0, 8, 14, 21, 23],
      ':api→:core': [0, 1],
      ':store→:core': [0, 1],
    }
    for (const edge of edgesOf(frame, e => e.kind === 'stroke')) {
      if (edge.kind !== 'stroke') throw new Error('expected a stroke')
      const indices = kept[`${edge.from}→${edge.to}`]!
      const drawn = paths(paintEdge(frame, edge))
      // One butt-ended quad per segment, like the native mesh.
      expect(drawn.length, `${edge.from}→${edge.to} segments`).toBe(Math.max(0, indices.length - 1))
      const flattened: [number, number][] = []
      drawn.forEach((d, i) => flattened.push(...(i === 0 ? d.points : d.points.slice(1))))
      for (const d of drawn) {
        expect(d.op.lineJoin, `${edge.from}→${edge.to}`).toBe('miter')
        expect(d.op.lineCap, `${edge.from}→${edge.to}`).toBe('butt')
        expect(d.op.strokeStyle).toBe(RELATION_COLORS[edge.stroke.role]!)
        expect(d.op.lineWidth).toBe(edge.width)
      }
      const expected = indices.map(i => {
        const [x, y] = edge.points[i]!
        return [(x - frame.camera.x) * frame.camera.zoom, (y - frame.camera.y) * frame.camera.zoom] as [number, number]
      })
      points(flattened, expected, `${edge.from}→${edge.to} decimated`)
    }
  })

  it('paints heads over their shafts at every zoom, including the 0.4–0.6 band', () => {
    // The native window flushes its low-zoom shaft mesh after the heads (a
    // known pre-existing bug); the panel deliberately does not copy it.
    for (const zoom of [1, 0.5, 0.4]) {
      const camera = { ...focused.camera, zoom }
      const canvas = fakeCanvas({ tokens: DARK_TOKENS, dark: true })
      paintFrame({ ...focused, camera, world: [], chrome: [] }, canvas)
      const ops = canvas.recording.ops
      // Index of each path's moveTo (the tip for a head, the start for a shaft).
      const starts = ops.map((op, index) => ({ op, index })).filter(({ op }) => op.op === 'moveTo')
      const at = (x: number, y: number): number[] => starts.filter(({ op }) => op.x === x && op.y === y).map(({ index }) => index)
      for (const edge of focused.edges) {
        if (edge.kind !== 'head') continue
        const [tx, ty] = [(edge.tip[0] - camera.x) * zoom, (edge.tip[1] - camera.y) * zoom]
        const stroke = focused.edges.find(e => e.kind === 'stroke' && e.from === edge.from && e.to === edge.to)!
        if (stroke.kind !== 'stroke') throw new Error('shaft')
        const [sx, sy] = [(stroke.points[0]![0] - camera.x) * zoom, (stroke.points[0]![1] - camera.y) * zoom]
        const heads = at(tx, ty)
        const shafts = at(sx, sy)
        const label = `${edge.from}→${edge.to} ${edge.head} @${zoom}`
        expect(heads.length, `${label} head drawn`).toBeGreaterThan(0)
        expect(shafts.length, `${label} shaft drawn`).toBeGreaterThan(0)
        for (const head of heads) for (const shaft of shafts) expect(head, `${label} head over shaft`).toBeGreaterThan(shaft)
      }
    }
  })

  it('omits heads at the frames tier and every edge at blocks', () => {
    const frames = golden('root-hover-frames')
    for (const head of edgesOf(frames, e => e.kind === 'head')) expect(paths(paintEdge(frames, head))).toEqual([])
    const blocks = golden('core-triangles-blocks')
    expect(blocks.lod).toBe('blocks')
    for (const edge of blocks.edges) expect(paths(paintEdge(blocks, edge))).toEqual([])
    for (const stroke of edgesOf(focused, e => e.kind === 'stroke')) {
      expect(paths(paintEdge(focused, stroke, { ...focused.camera, zoom: 0.1 }))).toEqual([])
    }
  })
})

describe('移除箭头 compact dependency indicators', () => {
  const frame = golden('root-triangles-selected')

  it('draws exact producer triangles, red when violating, and dims unrelated indicators', () => {
    expect(frame.edges.length).toBeGreaterThan(0)
    expect(frame.edges.every(e => e.kind === 'indicator')).toBe(true)
    const roles = new Set(frame.edges.flatMap(e => e.kind === 'indicator' ? [e.fill.role] : []))
    expect([...roles].sort()).toEqual(['indicator', 'violation'])
    expect(frame.edges.filter(e => e.kind === 'indicator' && e.violating).length).toBe(2)
    expect(frame.edges.some(e => e.kind === 'indicator' && e.dimmed)).toBe(true)

    for (const edge of frame.edges) {
      if (edge.kind !== 'indicator') continue
      const drawn = paths(paintEdge(frame, edge))
      const label = `${edge.id} ${edge.violating ? 'violating' : 'plain'} ${edge.dimmed ? 'dimmed' : ''}`
      expect(drawn.map(d => d.op.op), label).toEqual(['fill', 'stroke'])
      const expected = edge.points.map(([x, y]) =>
        [(x - frame.camera.x) * frame.camera.zoom, (y - frame.camera.y) * frame.camera.zoom] as [number, number])
      points(drawn[0]!.points, expected, label)
      expect(drawn[0]!.closed, label).toBe(true)
      const color = RELATION_COLORS[edge.fill.role]!
      const alpha = edge.dimmed ? frame.style.dimAlpha / 255 : 1
      expect(drawn[0]!.op.fillStyle, label).toBe(color)
      expect(drawn[0]!.op.globalAlpha, label).toBe(alpha)
      // The outline is the same color at 1 pt, as the native convex_polygon.
      expect(drawn[1]!.op.strokeStyle, label).toBe(color)
      expect(drawn[1]!.op.lineWidth, label).toBe(1)
      expect(drawn[1]!.op.globalAlpha, label).toBe(alpha)
    }
  })

  it('points up for incoming dependencies and down for outgoing ones, 12 world points wide', () => {
    // The incoming :util indicator above its box and the outgoing :core one
    // below are the two violating ones.
    const incoming = frame.edges.find(e => e.kind === 'indicator' && e.id === ':util' && e.violating)!
    const outgoing = frame.edges.find(e => e.kind === 'indicator' && e.id === ':core' && e.violating)!
    if (incoming.kind !== 'indicator' || outgoing.kind !== 'indicator') throw new Error('expected indicators')
    expect(incoming.points).toEqual([
      [207.25925925925927, 89.60769515458674], [201.25925925925927, 100], [213.25925925925927, 100],
    ])
    expect(outgoing.points).toEqual([
      [201.25925925925927, 696], [213.25925925925927, 696], [207.25925925925927, 706.3923048454133],
    ])
    // Exactly the producer's triangle, apex first; nothing is recomputed here.
    const drawn = paths(paintEdge(frame, incoming))
    expect(drawn[0]!.op.fillStyle).toBe('#c42a24')
    expect(drawn[0]!.points[0]).toEqual([
      (207.25925925925927 - frame.camera.x) * frame.camera.zoom,
      (89.60769515458674 - frame.camera.y) * frame.camera.zoom,
    ])
  })
})
