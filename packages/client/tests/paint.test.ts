import { readdirSync, readFileSync } from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'
import { describe, expect, it } from 'vitest'
import type { SceneFrame } from '@dsh-uml-viewer/contracts'
import { paintFrame } from '../src/paint.js'
import { FONT_STACK } from '../src/paint.js'
import { estimateW } from '../src/fit.js'
import { readFontMetrics } from './font-metrics.js'
import { DARK_TOKENS, fakeCanvas, LIGHT_TOKENS } from './fakes.js'

const framesDir = path.resolve(fileURLToPath(new URL('../../../crates/frame/tests/fixtures/frames', import.meta.url)))

const sarasa = readFontMetrics(
  path.resolve(fileURLToPath(new URL('../../../fonts/SarasaMonoSC-Regular.ttf', import.meta.url))),
  'Sarasa Mono SC',
)

const goldenNames = readdirSync(framesDir).filter(name => name.endsWith('.frame.json'))

function golden(name: string): SceneFrame {
  return JSON.parse(readFileSync(path.join(framesDir, name), 'utf8')) as SceneFrame
}

function fontPx(op: { font?: string }): number {
  const match = /(\d+(?:\.\d+)?)px/.exec(op.font ?? '')
  return match ? Number(match[1]) : 0
}

describe('Canvas 2D namespace tree', () => {
  it('draws every package and class box of the producer frame, in order', () => {
    const frame = golden('root.frame.json')
    const canvas = fakeCanvas({ tokens: DARK_TOKENS, dark: true })
    paintFrame(frame, canvas)
    const ops = canvas.recording.ops
    const boxes = frame.world.filter(item => item.kind === 'package' || item.kind === 'class')
    expect(boxes.length).toBeGreaterThan(0)
    // Each box is filled and outlined once.
    expect(ops.filter(op => op.op === 'fill').length).toBeGreaterThanOrEqual(boxes.length)
    expect(ops.filter(op => op.op === 'stroke').length).toBeGreaterThanOrEqual(boxes.length)
    // The canvas is cleared with the host theme's canvas token.
    const clear = ops.find(op => op.op === 'fillRect')
    expect(clear).toMatchObject({ fillStyle: '#161c20', width: 1220, height: 884 })
  })

  it('takes chrome colors from host theme tokens in light and dark', () => {
    const frame = golden('root.frame.json')
    const light = fakeCanvas({ tokens: LIGHT_TOKENS })
    paintFrame(frame, light)
    const dark = fakeCanvas({ tokens: DARK_TOKENS, dark: true })
    paintFrame(frame, dark)
    expect(light.recording.ops.find(op => op.op === 'fillRect')!.fillStyle).toBe('#ffffff')
    expect(dark.recording.ops.find(op => op.op === 'fillRect')!.fillStyle).toBe('#161c20')
    // Text exists in both and uses the host's label colors, not a hard-coded white.
    const lightText = light.recording.ops.filter(op => op.op === 'fillText')
    const darkText = dark.recording.ops.filter(op => op.op === 'fillText')
    expect(lightText.length).toBeGreaterThan(0)
    expect(Math.abs(darkText.length - lightText.length)).toBeLessThanOrEqual(8)
    expect(lightText.map(op => op.fillStyle)).toContain('#1c2324')
    expect(darkText.map(op => op.fillStyle)).toContain('#ecece4')
  })

  it('measures text with the packaged Sarasa family and never overflows its box', () => {
    const frame = golden('proposal.frame.json')
    const canvas = fakeCanvas({ tokens: DARK_TOKENS, dark: true })
    paintFrame(frame, canvas)
    const drawn = canvas.recording.ops.filter(op => op.op === 'fillText')
    expect(drawn.length).toBeGreaterThan(10)
    for (const op of drawn) {
      expect(op.font).toContain('UML Viewer Sarasa')
      expect(op.font).toContain(FONT_STACK.split(',')[0]!.replace(/"/g, '').trim())
    }
    // Every fitted label fits the maxW the producer gave that line.
    for (const item of frame.world) {
      if (item.kind !== 'text' || !item.text) continue
      const op = drawn.find(candidate => candidate.text === item.text)
      if (!op) continue
      expect(estimateW(item.text, fontPx(op)), `${item.text} at ${fontPx(op)}px`).toBeLessThanOrEqual(item.maxW + 1e-6)
    }
  })

  it('fits every label inside its box with the real Sarasa advances', () => {
    // Painting with the face the panel loads, not the layout estimate: the
    // drawn label (plus bold/italic allowance) must stay inside the box's maxW.
    const frame = golden('proposal.frame.json')
    const canvas = fakeCanvas({ tokens: DARK_TOKENS, dark: true, measure: (text, size) => sarasa.widthPx(text, size) })
    paintFrame(frame, canvas)
    const drawn = canvas.recording.ops.filter(op => op.op === 'fillText')
    let checked = 0
    for (const item of frame.world) {
      if (item.kind !== 'text' || !item.text) continue
      const op = drawn.find(candidate => candidate.text === item.text)
      if (!op) continue
      checked += 1
      const size = fontPx(op)
      const allowance = (item.bold ? size * 0.05 : 0) + (item.italic ? size * 0.2 : 0)
      expect(sarasa.widthPx(item.text, size) + allowance, `${item.text} at ${size}px`)
        .toBeLessThanOrEqual(item.maxW + 1e-6)
      expect(size).toBeLessThanOrEqual(item.size + 1e-9)
    }
    // Every golden label already fits at its nominal size, so fitting is proven
    // separately below; here the point is that the real face never overflows.
    expect(checked).toBeGreaterThan(5)
  })

  it('shrinks a long CJK label until the real Sarasa advance fits its box', () => {
    const base = golden('root.frame.json')
    const label = '命名空间树的类框宽度估算'
    const template = base.world.find(item => item.kind === 'text' && item.line === 'name')
    expect(template, 'the producer frame has a name line').toBeDefined()
    const synthetic: SceneFrame = {
      ...base,
      lod: 'full',
      world: [{ ...(template as SceneFrame['world'][number]), kind: 'text', lod: 'full', text: label, maxW: 80, size: 16 } as SceneFrame['world'][number]],
      edges: [],
      chrome: [],
    }
    const canvas = fakeCanvas({ tokens: DARK_TOKENS, dark: true, measure: (text, size) => sarasa.widthPx(text, size) })
    paintFrame(synthetic, canvas)
    const drawn = canvas.recording.ops.filter(op => op.op === 'fillText' && op.text === label)
    // The template is a bold name line: two passes at one fitted size.
    expect(drawn).toHaveLength(2)
    expect(drawn[1]!.font).toBe(drawn[0]!.font)
    const size = fontPx(drawn[0]!)
    expect(size).toBeLessThan(16)
    expect(sarasa.widthPx(label, size)).toBeLessThanOrEqual(80 + 1e-6)
  })

  it('uses the producer slot height for slotted golden text', () => {
    const frame = golden('root.frame.json')
    const item = frame.world.find(candidate => candidate.kind === 'text' && candidate.text === 'Util')
    if (!item || item.kind !== 'text') throw new Error('missing Util text')
    expect(item.size).toBe(16)
    expect(item.slot).toBe(15)
    const canvas = fakeCanvas({ tokens: DARK_TOKENS, dark: true })
    paintFrame(frame, canvas)
    const drawn = canvas.recording.ops.find(op => op.op === 'fillText' && op.text === 'Util')
    expect(drawn).toBeDefined()
    // px = round(slot * zoom) = round(15 * 106/127) = round(12.52) = 13, never the nominal 16.
    expect(fontPx(drawn!)).toBe(13)
  })

  it('dims every draw of every world item kind the producer marks dimmed', () => {
    // core-selected is the full-lod golden with the producer's dimmed items.
    // Every kind any golden carries is checked: each dimmed golden item as
    // written, and kinds with no dimmed instance (port, port-link) forced
    // dimmed. child-wash appears in no golden, so it stays synthetic.
    const frame = golden('core-selected.frame.json')
    expect(frame.lod).toBe('full')
    const dim = frame.style.dimAlpha / 255
    expect(frame.style.dimAlpha).toBe(70)
    const kinds = new Set(goldenNames.flatMap(name => golden(name).world.map(item => item.kind as string)))
    const items: SceneFrame['world'][number][] = frame.world.filter(item => item.dimmed)
    for (const kind of kinds) {
      if (items.some(item => item.kind === kind)) continue
      const template = frame.world.find(item => item.kind === kind)
      expect(template, `core-selected has a ${kind} item`).toBeDefined()
      items.push({ ...template!, dimmed: true } as SceneFrame['world'][number])
    }
    items.push({
      kind: 'child-wash', lod: 'full', bounds: null, dimmed: true, owner: ':core',
      rect: { x: 70, y: 150, w: 100, h: 16 },
      marker: { wash: { role: 'row-wash' }, bar: { role: 'row-bar' }, barWidth: 2 },
    } as unknown as SceneFrame['world'][number])
    // The golden's own dimmed package and quality items carry text.
    expect(items.filter(item => item.kind === 'package' || item.kind === 'quality').length).toBeGreaterThanOrEqual(5)
    const covered = new Set<string>()
    for (const item of items) {
      const canvas = fakeCanvas({ tokens: DARK_TOKENS, dark: true })
      paintFrame({ ...frame, world: [item], edges: [], chrome: [] }, canvas)
      // Exclude only the canvas clear; every draw op of the item is dimmed.
      const drawn = canvas.recording.ops.filter(op => ['fill', 'stroke', 'fillText', 'fillRect'].includes(op.op)
        && !(op.op === 'fillRect' && op.x === 0 && op.y === 0 && op.width === frame.viewport.width && op.height === frame.viewport.height))
      const label = `${item.kind} ${'id' in item ? String(item.id) : ''}`
      expect(drawn.length, label).toBeGreaterThan(0)
      for (const op of drawn) expect(op.globalAlpha, `${label} ${op.op} ${op.text ?? ''}`).toBeCloseTo(dim, 9)
      covered.add(item.kind)
    }
    for (const kind of kinds) expect(covered, kind).toContain(kind)
  })

  it('places a diamond back vertex 16 screen points behind the tip', () => {
    const frame = golden('core-selected.frame.json')
    const diamond = frame.edges.find(edge => edge.kind === 'head' && edge.head === 'diamond')
    if (!diamond || diamond.kind !== 'head') throw new Error('missing diamond head')
    const synthetic: SceneFrame = {
      ...frame,
      camera: { x: 0, y: 0, zoom: 1 },
      world: [],
      edges: [{ ...diamond, tip: [16, 0], behind: [0, 0], dimmed: false }],
      chrome: [],
    }
    const canvas = fakeCanvas({ tokens: DARK_TOKENS, dark: true })
    paintFrame(synthetic, canvas)
    const vertices = canvas.recording.ops.filter(op => op.op === 'lineTo')
    expect(vertices).toHaveLength(3)
    expect(vertices[1]).toMatchObject({ x: 0, y: 0 })
  })

  it('keeps the producer-supplied quality stroke on a hovered class', () => {
    const frame = golden('root-hover-frames.frame.json')
    const item = frame.world.find(candidate => candidate.kind === 'class' && candidate.hovered)
    if (!item || item.kind !== 'class') throw new Error('missing hovered class')
    const hovered = fakeCanvas({ tokens: DARK_TOKENS, dark: true })
    const ordinary = fakeCanvas({ tokens: DARK_TOKENS, dark: true })
    paintFrame({ ...frame, world: [item], edges: [], chrome: [] }, hovered)
    paintFrame({ ...frame, world: [{ ...item, hovered: false }], edges: [], chrome: [] }, ordinary)
    expect(hovered.recording.ops.find(op => op.op === 'stroke')!.strokeStyle)
      .toBe(ordinary.recording.ops.find(op => op.op === 'stroke')!.strokeStyle)
  })

  it('outlines a producer indicator with the same color as its fill', () => {
    const frame = golden('core-triangles-blocks.frame.json')
    const indicator = frame.edges.find(edge => edge.kind === 'indicator')!
    const canvas = fakeCanvas({ tokens: DARK_TOKENS, dark: true })
    paintFrame({ ...frame, lod: 'full', world: [], edges: [indicator], chrome: [] }, canvas)
    const fill = canvas.recording.ops.find(op => op.op === 'fill')
    const outline = canvas.recording.ops.find(op => op.op === 'stroke')
    expect(fill).toBeDefined()
    expect(outline).toBeDefined()
    expect(outline!.strokeStyle).toBe(fill!.fillStyle)
  })

  it('keeps a closed arrowhead opaque while its outline dims', () => {
    const frame = golden('root.frame.json')
    // Find a dimmed closed head; the producer writes one for the selected view.
    const head = frame.edges.find(edge => edge.kind === 'head' && edge.head !== 'open' && edge.dimmed)
    const candidates = head ?? frame.edges.find(edge => edge.kind === 'head' && edge.head !== 'open')
    expect(candidates, 'the producer frame has a closed head').toBeDefined()
    const synthetic: SceneFrame = {
      ...frame,
      edges: [{ ...(candidates as SceneFrame['edges'][number]), dimmed: true } as SceneFrame['edges'][number]],
      world: [],
      chrome: [],
    }
    const canvas = fakeCanvas({ tokens: DARK_TOKENS, dark: true })
    paintFrame(synthetic, canvas)
    const fills = canvas.recording.ops.filter(op => op.op === 'fill')
    const outline = canvas.recording.ops.filter(op => op.op === 'stroke')
    expect(fills.length).toBe(1)
    expect(outline.length).toBe(1)
    // The interior stays fully opaque (alpha 1); only the outline is dimmed.
    expect(fills[0]!.globalAlpha).toBe(1)
    expect(outline[0]!.globalAlpha).toBeCloseTo(70 / 255, 3)
  })

  it('skips a kind and a role it does not know instead of failing', () => {
    const frame = golden('root.frame.json')
    const synthetic: SceneFrame = {
      ...frame,
      world: [
        ...frame.world,
        { kind: 'hologram', lod: 'blocks', bounds: null, dimmed: false } as unknown as SceneFrame['world'][number],
      ],
    }
    const canvas = fakeCanvas({ tokens: DARK_TOKENS, dark: true })
    expect(() => paintFrame(synthetic, canvas)).not.toThrow()
    expect(canvas.recording.ops.length).toBeGreaterThan(0)
  })

  it('renders the waiting frame as a hint on the themed canvas', () => {
    const frame = { ...golden('root.frame.json'), waiting: true, world: [], edges: [], chrome: [] } as SceneFrame
    const canvas = fakeCanvas({ tokens: LIGHT_TOKENS })
    paintFrame(frame, canvas)
    const drawn = canvas.recording.ops.filter(op => op.op === 'fillText')
    expect(drawn).toHaveLength(1)
    expect(drawn[0]!.text).toMatch(/uml_display/)
  })
})
