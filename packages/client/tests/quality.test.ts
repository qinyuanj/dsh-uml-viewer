import { readFileSync } from 'node:fs'
import { describe, expect, it } from 'vitest'
import type { SceneFrame } from '@dsh-uml-viewer/contracts'
import { paintFrame } from '../src/paint.js'
import { fakeCanvas, DARK_TOKENS } from './fakes.js'

const golden = (name: string): SceneFrame => JSON.parse(readFileSync(
  new URL(`../../../crates/frame/tests/fixtures/frames/${name}.frame.json`, import.meta.url), 'utf8'))

/** #26 quality overlay: producer geometry and grades, native theme ramp values. */
describe('quality overlay from producer goldens', () => {
  it('paints independent C and M grades at their producer positions', () => {
    const frame = golden('core-selected')
    const dots = frame.world.filter(i => i.kind === 'quality' && i.owner === ':core.order')
    expect(dots.map(i => i.kind === 'quality' && [i.text, i.grade, i.center, i.radius])).toEqual([
      ['C', 4.0375, [182, 149], 5], ['M', 1, [182, 161], 5],
    ])
    const canvas = fakeCanvas({ tokens: DARK_TOKENS, dark: true })
    paintFrame({ ...frame, world: dots, edges: [], chrome: [] }, canvas)
    const ops = canvas.recording.ops
    // Camera -402,-167 zoom1: screen centers (584,316), (584,328).
    expect(ops.filter(o => o.op === 'arc').map(o => o.args)).toEqual([
      [584, 316, 5, 0, Math.PI * 2], [584, 328, 5, 0, Math.PI * 2],
    ])
    // Native frame::theme ramp, g=4.0375 and g=1; no TS metric aggregation.
    expect(ops.filter(o => o.op === 'fill').map(o => o.fillStyle)).toEqual(['#403718', '#472c18'])
    expect(ops.filter(o => o.op === 'stroke').map(o => [o.strokeStyle, o.lineWidth])).toEqual([
      ['#d6b357', 1], ['#de884d', 1],
    ])
    expect(ops.filter(o => o.op === 'fillText').map(o => [o.text, o.x, o.y, o.textAlign, o.globalAlpha])).toEqual([
      ['C', 584, 316, 'center', 1], ['M', 584, 328, 'center', 1],
    ])
  })

  it('keeps all missing classes red, dashed and labelled, including one on the chain', () => {
    const frame = golden('core-selected')
    const missing = frame.world.filter(i => i.kind === 'class' && i.missing)
    expect(missing.map(i => i.kind === 'class' && [i.id, i.band, i.fill])).toEqual([
      [':core', 'red', { role: 'quality-fill', grade: 1 }],
      [':core.cart', 'red', { role: 'quality-fill', grade: 1 }],
      [':core.price', 'red', { role: 'quality-fill', grade: 1 }],
    ])
    for (const box of missing) {
      if (box.kind !== 'class') throw new Error('class')
      const label = frame.world.filter(i => i.kind === 'no-data' && i.owner === box.id)
      expect(label).toHaveLength(1)
      const canvas = fakeCanvas({ tokens: DARK_TOKENS, dark: true })
      paintFrame({ ...frame, world: [box, ...label], edges: [], chrome: [] }, canvas)
      const ops = canvas.recording.ops
      expect(ops.find(o => o.op === 'fill')?.fillStyle).toBe('#472c18')
      expect(ops.find(o => o.op === 'stroke')?.lineDash).toEqual([6, 4])
      const noData = ops.find(o => o.op === 'fillText')!
      expect([noData.text, noData.fillStyle, noData.textAlign, noData.textBaseline]).toEqual(['无数据', '#c42a24', 'right', 'top'])
      expect(noData.globalAlpha).toBe(box.dimmed ? 70 / 255 : 1)
    }
  })

  it('retains the C/M discs at frames tier and drops only their letters, then dots at blocks', () => {
    const frame = golden('root-hover-frames')
    const dots = frame.world.filter(i => i.kind === 'quality')
    expect(dots).toHaveLength(12)
    const canvas = fakeCanvas({ tokens: DARK_TOKENS, dark: true })
    paintFrame({ ...frame, world: dots, edges: [], chrome: [] }, canvas)
    expect(canvas.recording.ops.filter(o => o.op === 'arc')).toHaveLength(12)
    expect(canvas.recording.ops.filter(o => o.op === 'fillText')).toHaveLength(0)
    const blocks = fakeCanvas({ tokens: DARK_TOKENS, dark: true })
    paintFrame({ ...frame, camera: { ...frame.camera, zoom: 0.12 }, world: dots, edges: [], chrome: [] }, blocks)
    expect(blocks.recording.ops.filter(o => o.op === 'arc')).toHaveLength(0)
  })
})
