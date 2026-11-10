/**
 * Panel graph primitives (dsh-uml-viewer#24): class boxes, component
 * (package) boxes, stereotypes, α / I, level numbers, dependency ports and
 * foreign-package ellipses, painted from the producer goldens and compared
 * with the native egui painter's rules (the deleted crates/viewer/src/paint.rs, #30). Expected
 * numbers are worked by hand from the golden item and docs/scene-frame.md:
 * `screen = (world - camera) * zoom`, font px `round(size * zoom)`, inside
 * strokes for rects, middle strokes for ellipses and lines.
 */
import { readdirSync, readFileSync } from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'
import { describe, expect, it } from 'vitest'
import type { SceneFrame } from '@dsh-uml-viewer/contracts'
import { paintFrame, tierOf } from '../src/paint.js'
import { readFontMetrics } from './font-metrics.js'
import { DARK_TOKENS, type DrawOp, fakeCanvas } from './fakes.js'

type WorldItem = SceneFrame['world'][number]

const framesDir = path.resolve(fileURLToPath(new URL('../../../crates/frame/tests/fixtures/frames', import.meta.url)))
const sarasa = readFontMetrics(
  path.resolve(fileURLToPath(new URL('../../../fonts/SarasaMonoSC-Regular.ttf', import.meta.url))),
  'Sarasa Mono SC',
)

function golden(name: string): SceneFrame {
  return JSON.parse(readFileSync(path.join(framesDir, name), 'utf8')) as SceneFrame
}

function find(frame: SceneFrame, predicate: (item: WorldItem) => boolean, what: string): WorldItem {
  const item = frame.world.find(predicate)
  if (!item) throw new Error(`golden has no ${what}`)
  return item
}

/** Paint one item alone on its golden frame and return the draw ops. */
function paintAlone(frame: SceneFrame, item: WorldItem, measure?: (text: string, px: number) => number): DrawOp[] {
  const canvas = fakeCanvas(measure ? { tokens: DARK_TOKENS, dark: true, measure } : { tokens: DARK_TOKENS, dark: true })
  paintFrame({ ...frame, world: [item], edges: [], chrome: [] }, canvas)
  // Drop the canvas clear.
  return canvas.recording.ops.slice(canvas.recording.ops.findIndex(op => op.op === 'fillRect') + 1)
}

interface Painted { op: DrawOp; path: DrawOp[] }

/** Every fill/stroke with the path ops built since its beginPath. */
function painted(ops: DrawOp[]): Painted[] {
  const out: Painted[] = []
  let path: DrawOp[] = []
  for (const op of ops) {
    if (op.op === 'beginPath') path = []
    else if (op.op === 'fill' || op.op === 'stroke') out.push({ op, path: [...path] })
    else if (['moveTo', 'lineTo', 'arcTo', 'arc', 'ellipse', 'rect', 'closePath'].includes(op.op)) path.push(op)
  }
  return out
}

function bbox(path: DrawOp[]): { x: number; y: number; w: number; h: number } {
  const xs: number[] = []
  const ys: number[] = []
  for (const op of path) {
    const a = op.args ?? []
    if (op.op === 'moveTo' || op.op === 'lineTo') { xs.push(a[0]!); ys.push(a[1]!) }
    if (op.op === 'arcTo') { xs.push(a[0]!, a[2]!); ys.push(a[1]!, a[3]!) }
    if (op.op === 'ellipse') { xs.push(a[0]! - a[2]!, a[0]! + a[2]!); ys.push(a[1]! - a[3]!, a[1]! + a[3]!) }
    if (op.op === 'arc') { xs.push(a[0]! - a[2]!, a[0]! + a[2]!); ys.push(a[1]! - a[2]!, a[1]! + a[2]!) }
  }
  const x = Math.min(...xs)
  const y = Math.min(...ys)
  return { x, y, w: Math.max(...xs) - x, h: Math.max(...ys) - y }
}

function radii(path: DrawOp[]): number[] {
  return path.filter(op => op.op === 'arcTo').map(op => op.args![4]!)
}

function expectBox(actual: { x: number; y: number; w: number; h: number }, expected: [number, number, number, number], label: string): void {
  expect(actual.x, `${label} x`).toBeCloseTo(expected[0], 6)
  expect(actual.y, `${label} y`).toBeCloseTo(expected[1], 6)
  expect(actual.w, `${label} w`).toBeCloseTo(expected[2], 6)
  expect(actual.h, `${label} h`).toBeCloseTo(expected[3], 6)
}

function texts(ops: DrawOp[]): DrawOp[] {
  return ops.filter(op => op.op === 'fillText')
}

const SELECTED = '#ecece4' // selected -> --dsw-alias-label-primary (dark)
const MUTED = '#9db8a8' // muted -> --dsw-alias-label-tertiary
const CHAIN = '#3e98ec' // dependency-chain: hue-fixed egui blue (frame::theme RELATED)
const ACCENT = '#e8c448' // accent (package titles) -> --dsw-alias-state-business-primary

describe('class boxes (core-port-selected, zoom 0.6, camera 0,40)', () => {
  const frame = golden('core-port-selected.frame.json')

  it('fills the selected class at its screen rect and strokes it inside, like egui StrokeKind::Inside', () => {
    const item = find(frame, i => i.kind === 'class' && i.id === ':core.order', 'class :core.order')
    const [fill, stroke] = painted(paintAlone(frame, item))
    // rect (68,140,120,90) -> (40.8, 60, 72, 54); radius 4 * 0.6 = 2.4.
    expect(fill!.op.op).toBe('fill')
    expectBox(bbox(fill!.path), [40.8, 60, 72, 54], 'fill')
    expect(radii(fill!.path).every(r => Math.abs(r - 2.4) < 1e-9)).toBe(true)
    // Quality ramp for grade 2.51875: red→mid by 0.50375 = #433118.
    expect(fill!.op.fillStyle).toBe('#433118')
    // Inside stroke 3.5: the centre line is inset by 1.75, its radius by 1.75.
    expect(stroke!.op.op).toBe('stroke')
    expectBox(bbox(stroke!.path), [42.55, 61.75, 68.5, 50.5], 'stroke')
    expect(radii(stroke!.path).every(r => Math.abs(r - 0.65) < 1e-9)).toBe(true)
    expect(stroke!.op.lineWidth).toBe(3.5)
    expect(stroke!.op.strokeStyle).toBe(SELECTED)
    expect(stroke!.op.lineDash).toEqual([])
  })

  it('outlines an unmeasured class with a square dashed frame over its rounded fill', () => {
    const item = find(frame, i => i.kind === 'class' && i.id === ':core.price', 'class :core.price')
    const [fill, stroke] = painted(paintAlone(frame, item))
    // rect (228,370,120,90) -> (136.8, 198, 72, 54).
    expectBox(bbox(fill!.path), [136.8, 198, 72, 54], 'fill')
    expect(fill!.op.fillStyle).toBe('#472c18') // grade 1: red→mid by 0.2
    // egui dashed_rect: the four edges of the rect, dash 6 gap 4, no corner.
    expectBox(bbox(stroke!.path), [136.8, 198, 72, 54], 'dashed outline')
    expect(radii(stroke!.path)).toEqual([])
    expect(stroke!.op.lineDash).toEqual([6, 4])
    expect(stroke!.op.lineWidth).toBe(2.5)
    expect(stroke!.op.strokeStyle).toBe(CHAIN)
  })

  it('draws a bold centred class name as two passes 0.05 px apart at round(slot * zoom)', () => {
    const item = find(frame, i => i.kind === 'text' && i.text === 'Order', 'name Order')
    const drawn = texts(paintAlone(frame, item))
    // px = round(15 * 0.6) = 9; centre x = (79.6 + 96.8 / 2) * 0.6 = 76.8;
    // galley top = (154 - 40) * 0.6 + max(0, (9 - 9) / 2) = 68.4.
    expect(drawn.map(op => [op.x, op.y])).toEqual([[76.8, expect.closeTo(68.4, 9)], [expect.closeTo(77.25, 9), expect.closeTo(68.4, 9)]])
    for (const op of drawn) {
      expect(op.font).toMatch(/^9px /)
      expect(op.textAlign).toBe('center')
      expect(op.textBaseline).toBe('top')
      expect(op.fillStyle).toBe(SELECTED)
    }
  })

  it('draws the «abstract» stereotype, α and the level number at their native positions and sizes', () => {
    const stereo = find(frame, i => i.kind === 'text' && i.line === 'stereo' && i.text === '«abstract»', 'stereotype')
    const [s] = texts(paintAlone(frame, stereo))
    // px = round(16 * 0.6) = 10; top = (384.333 - 40) * 0.6 = 206.6, slot 9.6 < 10 so no inset.
    expect(s).toMatchObject({ text: '«abstract»', x: 144, textAlign: 'left', textBaseline: 'top', fillStyle: MUTED })
    expect(s!.y).toBeCloseTo(206.6, 9)
    expect(s!.font).toMatch(/^10px /)

    const alpha = find(frame, i => i.kind === 'marker' && i.text === 'α', 'α marker')
    const [a] = texts(paintAlone(frame, alpha))
    // at (240.5, 373) -> (144.3, 199.8); px = round(12 * 0.6) = 7.
    expect(a!.x).toBeCloseTo(144.3, 9)
    expect(a!.y).toBeCloseTo(199.8, 9)
    expect(a).toMatchObject({ textAlign: 'left', textBaseline: 'top', fillStyle: SELECTED })
    expect(a!.font).toMatch(/^7px /)

    const level = find(frame, i => i.kind === 'level' && i.owner === ':core.price', 'level')
    const [l] = texts(paintAlone(frame, level))
    // at (232, 373) -> (139.2, 199.8); px = round(11 * 0.6) = 7.
    expect(l!.x).toBeCloseTo(139.2, 9)
    expect(l!.y).toBeCloseTo(199.8, 9)
    expect(l).toMatchObject({ text: '0', textBaseline: 'top', fillStyle: MUTED })
    expect(l!.font).toMatch(/^7px /)
  })

  it('draws the I marker of a dimmed interface at the dim alpha', () => {
    const marker = find(frame, i => i.kind === 'marker' && i.text === 'I', 'I marker')
    const [m] = texts(paintAlone(frame, marker))
    // at (80.5, 373) -> (48.3, 199.8).
    expect(m!.x).toBeCloseTo(48.3, 9)
    expect(m!.y).toBeCloseTo(199.8, 9)
    expect(m!.globalAlpha).toBeCloseTo(70 / 255, 9)
  })

  it('draws the C/M letter at round(9 * zoom) px, centred on its dot', () => {
    const quality = find(frame, i => i.kind === 'quality' && i.owner === ':core.price' && i.text === 'C', 'quality C')
    if (quality.kind !== 'quality') throw new Error('not a quality item')
    const ops = paintAlone(frame, quality)
    const [disc, ring] = painted(ops)
    const cx = quality.center[0] * 0.6
    const cy = (quality.center[1] - 40) * 0.6
    expectBox(bbox(disc!.path), [cx - quality.radius * 0.6, cy - quality.radius * 0.6, quality.radius * 1.2, quality.radius * 1.2], 'dot')
    expect(ring!.op.lineWidth).toBe(1)
    const [letter] = texts(ops)
    expect(letter!.font).toMatch(/^5px /) // round(5.4) = 5
    expect(letter).toMatchObject({ textAlign: 'center', textBaseline: 'middle' })
    expect(letter!.x).toBeCloseTo(cx, 9)
    expect(letter!.y).toBeCloseTo(cy, 9)
  })
})

describe('component (package) boxes', () => {
  it('fills a package translucently, dashes its unmeasured frame and places the italic title at +12,+8', () => {
    const frame = golden('core-port-selected.frame.json')
    const item = find(frame, i => i.kind === 'package', 'package')
    const ops = paintAlone(frame, item)
    const [fill, stroke] = painted(ops)
    // rect (56,56,304,474) -> (33.6, 9.6, 182.4, 284.4); radius 8 * 0.6 = 4.8.
    expectBox(bbox(fill!.path), [33.6, 9.6, 182.4, 284.4], 'fill')
    expect(radii(fill!.path).every(r => Math.abs(r - 4.8) < 1e-9)).toBe(true)
    // package-fill is the fill ramp at alpha 80, package-stroke the stroke ramp at 170.
    expect(fill!.op.fillStyle).toBe('rgba(71, 44, 24, 0.3137)')
    expectBox(bbox(stroke!.path), [33.6, 9.6, 182.4, 284.4], 'dashed outline')
    expect(stroke!.op.lineDash).toEqual([6, 4])
    expect(stroke!.op.strokeStyle).toBe('rgba(222, 136, 77, 0.6667)')
    expect(stroke!.op.lineWidth).toBe(2)
    const [title] = texts(ops)
    // (56 + 12, 56 + 8) -> (40.8, 14.4); px = round(16 * 0.6) = 10, italic, accent.
    expect(title).toMatchObject({ text: 'core', textAlign: 'left', textBaseline: 'top', fillStyle: ACCENT })
    expect(title!.x).toBeCloseTo(40.8, 9)
    expect(title!.y).toBeCloseTo(14.4, 9)
    expect(title!.font).toMatch(/^italic 10px /)
    expect(title!.globalAlpha).toBeCloseTo(70 / 255, 9)
  })
})

describe('dependency ports', () => {
  const frame = golden('core-port-selected.frame.json')

  it('marks the selected port: marked fill, selected inside stroke, square under 2 px radius, ink label', () => {
    const item = find(frame, i => i.kind === 'port' && i.id === ':api', 'port :api')
    const ops = paintAlone(frame, item)
    const [fill, stroke] = painted(ops)
    // rect (78,100,44,18) -> (46.8, 36, 26.4, 10.8); radius 3 * 0.6 = 1.8 < 2: square.
    expectBox(bbox(fill!.path), [46.8, 36, 26.4, 10.8], 'fill')
    expect(radii(fill!.path).every(r => r === 0)).toBe(true)
    expect(fill!.op.fillStyle).toBe('#4a604e') // port-fill-marked -> done-secondary
    expectBox(bbox(stroke!.path), [47.9, 37.1, 24.2, 8.6], 'inside stroke 2.2')
    expect(stroke!.op).toMatchObject({ lineWidth: 2.2, strokeStyle: SELECTED })
    const [label] = texts(ops)
    // centre x 60; top = centre y 41.4 - 5 * 0.6 = 38.4; px = round(10 * 0.6) = 6.
    expect(label).toMatchObject({ text: 'Api', textAlign: 'center', textBaseline: 'top', fillStyle: SELECTED })
    expect(label!.x).toBeCloseTo(60, 9)
    expect(label!.y).toBeCloseTo(38.4, 9)
    expect(label!.font).toMatch(/^6px /)
  })

  it('draws an unselected port with port roles', () => {
    const item = find(frame, i => i.kind === 'port' && i.id === ':store', 'port :store')
    const ops = paintAlone(frame, item)
    const [fill, stroke] = painted(ops)
    expect(fill!.op.fillStyle).toBe('#1a2428') // port-fill -> bg-layer-2
    expect(stroke!.op).toMatchObject({ lineWidth: 1, strokeStyle: MUTED })
    expect(texts(ops)[0]).toMatchObject({ text: 'Store', fillStyle: MUTED })
  })

  it('arrows a port into its class with a 10 screen-point head at ±20°, whatever the zoom', () => {
    const item = find(frame, i => i.kind === 'port-link' && i.owner === ':core.order', 'port-link')
    const ops = paintAlone(frame, item)
    const strokes = painted(ops).filter(p => p.op.op === 'stroke')
    expect(strokes).toHaveLength(3)
    for (const s of strokes) expect(s.op).toMatchObject({ lineWidth: 1.3, strokeStyle: MUTED })
    // behind (100,118) -> (60, 46.8); tip (100,140) -> (60, 60).
    expect(strokes[0]!.path.map(op => [op.x, op.y])).toEqual([[60, expect.closeTo(46.8, 9)], [60, 60]])
    const wings = strokes.slice(1).map(s => s.path[1]!).map(op => [op.x!, op.y!]).sort((p, q) => p[0]! - q[0]!)
    // tip - 10 * (sin 20°, cos 20°) on each side.
    expect(wings[0]![0]).toBeCloseTo(56.5798, 4)
    expect(wings[0]![1]).toBeCloseTo(50.6031, 4)
    expect(wings[1]![0]).toBeCloseTo(63.4202, 4)
    expect(wings[1]![1]).toBeCloseTo(50.6031, 4)
  })
})

describe('foreign-package ellipses', () => {
  it('draws an unmeasured foreign package as a solid ellipse with a middle stroke (root, zoom 106/127)', () => {
    const frame = golden('root.frame.json')
    const item = find(frame, i => i.kind === 'class' && i.shape === 'oval', 'oval')
    const [fill, stroke] = painted(paintAlone(frame, item))
    // rect (168,916,80,44) at the golden's own fit (#46: zoom 106/122,
    // camera -483.0859728506787,0): centre (601.2992, 816.1339), radii (34.8031, 19.1417).
    for (const p of [fill!, stroke!]) {
      const e = p.path.find(op => op.op === 'ellipse')!
      expect(e.args![0]).toBeCloseTo(601.2992125984251, 9)
      expect(e.args![1]).toBeCloseTo(816.1338582677165, 9)
      expect(e.args![2]).toBeCloseTo(34.803149606299215, 9)
      expect(e.args![3]).toBeCloseTo(19.141732283464567, 9)
    }
    expect(fill!.op.fillStyle).toBe('#472c18')
    // egui draws the oval branch before the missing check: never dashed.
    expect(stroke!.op).toMatchObject({ lineDash: [], lineWidth: 2.5, strokeStyle: '#de884d' })
  })
})

describe('overview tiers', () => {
  it('draws square class boxes and paints every name after every box (root-hover-frames, zoom 0.3)', () => {
    const frame = golden('root-hover-frames.frame.json')
    expect(frame.lod).toBe('frames')
    const canvas = fakeCanvas({ tokens: DARK_TOKENS, dark: true })
    paintFrame({ ...frame, edges: [], chrome: [] }, canvas)
    const ops = canvas.recording.ops
    const boxes = painted(ops).filter(p => p.path.some(op => op.op === 'arcTo'))
    // radius 4 * 0.3 = 1.2 < 2 and 8 * 0.3 = 2.4 >= 2: only package corners stay round.
    for (const box of boxes) for (const r of radii(box.path)) expect(r === 0 || Math.abs(r - 2.4) < 1e-9 || Math.abs(r - 1.4) < 1e-9).toBe(true)
    const lastBox = ops.map(op => op.op).lastIndexOf('fill')
    const names = new Set(frame.world.filter(i => i.kind === 'text' && i.line === 'name').map(i => (i as { text: string }).text))
    const firstName = ops.findIndex(op => op.op === 'fillText' && names.has(op.text ?? ''))
    expect(firstName).toBeGreaterThan(lastBox)
  })
})

describe('tier from the camera zoom', () => {
  it('paints at the tier of the zoom the panel shows, not the tier the frame was produced at', () => {
    // A local wheel zoom keeps the producer frame (every item, tagged with its
    // coarsest tier) and replaces only the camera; lodZoom decides the tier.
    const frame = golden('core-port-selected.frame.json')
    expect(frame.lod).toBe('full')
    const levels = frame.world.filter(i => i.kind === 'level').length
    expect(levels).toBe(4)
    const count = (zoom: number): { level: number; name: number; quality: number } => {
      const canvas = fakeCanvas({ tokens: DARK_TOKENS, dark: true })
      paintFrame({ ...frame, camera: { x: 0, y: 0, zoom }, edges: [], chrome: [] }, canvas)
      const drawn = texts(canvas.recording.ops).map(op => op.text)
      return {
        level: drawn.filter(t => t === '0').length,
        name: drawn.filter(t => t === 'Order').length,
        quality: painted(canvas.recording.ops).filter(p => p.path.some(op => op.op === 'arc')).length,
      }
    }
    // full (zoom >= 0.4): levels drawn; frames (0.15 <= zoom < 0.4): names and dots but no
    // levels; blocks: neither.
    expect(count(0.6).level).toBe(4)
    expect(count(0.3)).toEqual({ level: 0, name: 2, quality: 20 })
    expect(count(0.12)).toEqual({ level: 0, name: 0, quality: 0 })
  })

  it('agrees with the producer tier on every golden', () => {
    const names = readdirSync(framesDir).filter(name => name.endsWith('.frame.json'))
    // #44: each of the seven views has a Chinese and an English golden.
    expect(names).toHaveLength(14)
    for (const name of names) {
      const frame = golden(name)
      expect(tierOf(frame), name).toBe(frame.lod)
    }
  })
})

describe('CJK labels with the packaged Sarasa face', () => {
  it('shrinks a CJK class name to whole pixels until its Sarasa advance fits', () => {
    const base = golden('core-selected.frame.json') // zoom 1
    const template = find(base, i => i.kind === 'text' && i.line === 'name', 'name line')
    const label = '命名空间树的类框宽度估算' // 12 full-width glyphs, 1 em each in Sarasa Mono
    const item = { ...template, text: label, maxW: 80, slot: 16, size: 16, bold: false, italic: false, center: false, dimmed: false } as WorldItem
    const [drawn] = texts(paintAlone(base, item, (text, px) => sarasa.widthPx(text, px)))
    // nominal 16 -> natural 192 > 80; floor(16 * 80 / 192) = 6, and 12 * 6 = 72 fits.
    expect(drawn!.font).toMatch(/^6px "UML Viewer Sarasa"/)
    expect(sarasa.widthPx(label, 6)).toBeCloseTo(72, 6)
  })
})
