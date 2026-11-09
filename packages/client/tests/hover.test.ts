import { readFileSync } from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'
import { describe, expect, it } from 'vitest'
import type { SceneFrame } from '@dsh-uml-viewer/contracts'
import { hoverAt, withHover } from '../src/hover.js'
import { paintFrame } from '../src/paint.js'
import { fakeCanvas, DARK_TOKENS } from './fakes.js'

/**
 * 关系悬停高亮 is client-side (ADR 0013): the panel restyles the resident's
 * frame for what is under the pointer. The oracle is the producer itself:
 * `root` and `root-hover-frames` are the same Shop scene (identical edge
 * geometry), the second one rendered by Rust with `--hover {:kind :class :id
 * :core} --pointer 300,240`. Hovering `:core` on top of `root` must give
 * exactly the producer's hovered edges.
 */
const framesDir = path.resolve(fileURLToPath(new URL('../../../crates/frame/tests/fixtures/frames', import.meta.url)))
const golden = (name: string): SceneFrame =>
  JSON.parse(readFileSync(path.join(framesDir, `${name}.frame.json`), 'utf8')) as SceneFrame

/** The hover-dependent fields of an edge, as the producer writes them. */
function hoverFields(frame: SceneFrame): unknown[] {
  return frame.edges.map(edge => edge.kind === 'indicator'
    ? { kind: edge.kind, id: edge.id, fill: edge.fill, dimmed: edge.dimmed }
    : { kind: edge.kind, from: edge.from, to: edge.to, stroke: edge.stroke, width: edge.width,
        hot: edge.hot, warm: edge.warm, dimmed: edge.dimmed })
}

describe('client hover (关系悬停高亮)', () => {
  it('warms the edges touching a hovered class exactly like the producer', () => {
    const base = golden('root')
    const expected = golden('root-hover-frames')
    const hovered = withHover(base, { kind: 'class', id: ':core' }, null)
    expect(hoverFields(hovered)).toEqual(hoverFields(expected))
    // Three edges touch :core; one of them (:core -> :util) violates and turns
    // the brighter red at the producer's hot violating width.
    const warm = hovered.edges.filter(e => e.kind === 'stroke' && e.warm)
    expect(warm.map(e => e.kind === 'stroke' && [e.from, e.to, e.stroke.role, e.width])).toEqual([
      [':api', ':core', 'relation-hover', 1.6],
      [':store', ':core', 'relation-hover', 1.6],
      [':core', ':util', 'violation-hot', 3.2],
    ])
    // The hovered box is flagged like the producer's, and nothing else is.
    expect(hovered.world.filter(i => i.kind === 'class' && i.hovered).map(i => i.kind === 'class' && i.id)).toEqual([':core'])
    expect(hovered.world.filter(i => i.kind === 'class' && i.hovered).length)
      .toBe(expected.world.filter(i => i.kind === 'class' && i.hovered).length)
  })

  it('never mutates the resident frame and restores it when the hover ends', () => {
    const base = golden('root')
    const before = JSON.stringify(base)
    withHover(base, { kind: 'class', id: ':core' }, [300, 240])
    expect(JSON.stringify(base)).toBe(before)
    expect(withHover(base, null, [300, 240])).toEqual(base)
  })

  it('names a hovered class in a popup at the overview tiers, placed like the producer', () => {
    const expected = golden('root-hover-frames')
    // Same view as the golden, without the producer's hover: camera -200,-100 at 0.3.
    const base: SceneFrame = {
      ...expected,
      chrome: expected.chrome.filter(c => c.kind !== 'popup'),
      world: expected.world.map(i => (i.kind === 'class' ? { ...i, hovered: false } : i)),
      edges: golden('root').edges,
    }
    const hovered = withHover(base, { kind: 'class', id: ':core' }, [300, 240])
    expect(hovered.chrome).toEqual(expected.chrome)
    expect(hovered).toEqual(expected)
  })

  it('paints the overview popup text at the native top edge and chrome font size', () => {
    const frame = golden('root-hover-frames')
    const canvas = fakeCanvas({ tokens: DARK_TOKENS, dark: true })
    paintFrame({ ...frame, world: [], edges: [], chrome: frame.chrome.filter(c => c.kind === 'popup') }, canvas)
    // Producer popup rect (300,256,56,36); native paint.rs uses +8,+8,
    // CHROME_PX=18, draw_text top anchoring, and an inside 1px outline.
    expect(canvas.recording.ops.find(o => o.op === 'fillText')).toMatchObject({
      text: 'Core', x: 308, y: 264, textBaseline: 'top', fillStyle: '#ecece4',
    })
    expect(canvas.recording.ops.find(o => o.op === 'fillText')!.font).toMatch(/^18px /)
    expect(canvas.recording.ops.filter(o => o.op === 'moveTo').map(o => o.args)).toEqual([[304, 256], [304, 256.5]])
  })

  it('shows no class popup at the full tier, where the box shows its name', () => {
    const base = golden('root')
    expect(base.lod).toBe('full')
    const hovered = withHover(base, { kind: 'class', id: ':core' }, [300, 240])
    expect(hovered.chrome).toEqual(base.chrome)
  })

  it('uses the local camera tier for the overview name popup before another resident frame arrives', () => {
    const base = golden('root')
    const zoomed = { ...base, camera: { ...base.camera, zoom: 0.3 } }
    expect(withHover(zoomed, { kind: 'class', id: ':core' }, [300, 240]).chrome.at(-1)?.kind).toBe('popup')
    const overview = golden('root-hover-frames')
    const localFull = { ...overview, chrome: overview.chrome.filter(c => c.kind !== 'popup'), camera: { ...overview.camera, zoom: 1 } }
    expect(withHover(localFull, { kind: 'class', id: ':core' }, [300, 240]).chrome.some(c => c.kind === 'popup')).toBe(false)
  })

  it('lights only the edge under the pointer, undimmed, over a selection that dims it', () => {
    // core-selected: :core.order is selected; four relations are dimmed.
    const base = golden('core-selected')
    const dimmed = base.edges.find(e => e.kind === 'stroke' && e.dimmed)
    if (!dimmed || dimmed.kind !== 'stroke') throw new Error('core-selected has a dimmed relation')
    const hovered = withHover(base, { kind: 'edge', from: dimmed.from, to: dimmed.to }, null)
    const same = (e: SceneFrame['edges'][number]): boolean => e.kind !== 'indicator' && e.from === dimmed.from && e.to === dimmed.to
    for (const edge of hovered.edges) {
      if (edge.kind === 'indicator') continue
      if (same(edge)) {
        // GLOSSARY 关系悬停高亮: an ordinary relation turns white (relation-hover)
        // and is no longer dimmed while hovered; its width stays 1.6.
        expect([edge.stroke, edge.width, edge.hot, edge.warm, edge.dimmed]).toEqual([{ role: 'relation-hover' }, 1.6, true, false, false])
      } else {
        const original = base.edges[hovered.edges.indexOf(edge)]
        expect(edge).toEqual(original)
      }
    }
    // A hovered closed head keeps its opaque canvas fill (only the outline is restyled).
    const head = hovered.edges.find(e => e.kind === 'head' && same(e))
    const baseHead = base.edges.find(e => e.kind === 'head' && same(e))
    if (head?.kind === 'head' && baseHead?.kind === 'head') expect(head.fill).toEqual(baseHead.fill)
  })
})

describe('hover hit test (scene::hit::at order)', () => {
  const frame = golden('root')
  const center = (id: string): [number, number] => {
    const item = frame.world.find(i => i.kind === 'class' && i.id === id)
    if (!item || item.kind !== 'class') throw new Error(id)
    return [item.rect.x + item.rect.w / 2, item.rect.y + item.rect.h / 2]
  }

  it('hits a class box before anything else', () => {
    expect(hoverAt(frame, center(':core'))).toEqual({ kind: 'class', id: ':core' })
  })

  it('hits an edge within 8 world units of its stroke, and not further away', () => {
    const stroke = frame.edges.find(e => e.kind === 'stroke' && e.from === ':store' && e.to === ':serde')
    if (!stroke || stroke.kind !== 'stroke') throw new Error('store -> serde')
    // The midpoint of the stroke's longest segment, offset perpendicular to it.
    let best = 0
    for (let i = 1; i < stroke.points.length; i += 1) {
      const [a, b] = [stroke.points[i - 1]!, stroke.points[i]!]
      if (Math.hypot(b[0] - a[0], b[1] - a[1]) > Math.hypot(stroke.points[best + 1]![0] - stroke.points[best]![0], stroke.points[best + 1]![1] - stroke.points[best]![1])) best = i - 1
    }
    const [a, b] = [stroke.points[best]!, stroke.points[best + 1]!]
    const len = Math.hypot(b[0] - a[0], b[1] - a[1])
    const normal = [-(b[1] - a[1]) / len, (b[0] - a[0]) / len]
    const mid = [(a[0] + b[0]) / 2, (a[1] + b[1]) / 2]
    const at = (d: number): [number, number] => [mid[0]! + normal[0]! * d, mid[1]! + normal[1]! * d]
    expect(hoverAt(frame, at(7.9))).toEqual({ kind: 'edge', from: ':store', to: ':serde' })
    expect(hoverAt(frame, at(-7.9))).toEqual({ kind: 'edge', from: ':store', to: ':serde' })
    expect(hoverAt(frame, at(8.2))).not.toEqual({ kind: 'edge', from: ':store', to: ':serde' })
    expect(hoverAt(frame, at(-8.2))).not.toEqual({ kind: 'edge', from: ':store', to: ':serde' })
  })

  it('a package or empty canvas highlights nothing', () => {
    // Inside the :view package, away from boxes and edges (its title strip).
    expect(hoverAt(frame, [70, 60])).toBeNull()
    expect(hoverAt(frame, [-500, -500])).toBeNull()
  })

  it('a dependency port stands for its class (scene::highlight::box_id)', () => {
    const drilled = golden('core-port-selected')
    const port = drilled.world.find(i => i.kind === 'port')
    if (!port || port.kind !== 'port') throw new Error('port')
    expect(hoverAt(drilled, [port.rect.x + port.rect.w / 2, port.rect.y + port.rect.h / 2])).toEqual({ kind: 'port', id: port.owner })
    const hit = hoverAt(drilled, [port.rect.x + port.rect.w / 2, port.rect.y + port.rect.h / 2])
    const overview = { ...drilled, camera: { ...drilled.camera, zoom: 0.3 } }
    const hovered = withHover(overview, hit, [100, 100])
    expect(hovered.chrome.some(c => c.kind === 'popup')).toBe(false)
    expect(hovered.edges.filter(e => e.kind === 'stroke' && e.warm).length).toBeGreaterThan(0)
  })
})
