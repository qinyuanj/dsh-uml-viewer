import { readFileSync, readdirSync } from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'
import { describe, expect, it } from 'vitest'
import { KNOWN_WORLD_KINDS, validateSceneFrame } from '../src/index.js'
import type { SceneFrame } from '../src/index.js'

// The Rust producer's own goldens are the contract (docs/scene-frame.md).
const frames = path.resolve(fileURLToPath(new URL('../../../crates/frame/tests/fixtures/frames', import.meta.url)))
const goldenFiles = readdirSync(frames).filter(name => name.endsWith('.frame.json')).sort()

function golden(name: string): unknown {
  return JSON.parse(readFileSync(path.join(frames, name), 'utf8'))
}

describe('scene frame validation', () => {
  it('accepts every producer golden unchanged', () => {
    expect(goldenFiles.length).toBeGreaterThanOrEqual(5)
    for (const name of goldenFiles) {
      const value = golden(name)
      // toEqual proves the validator neither redefines nor normalizes the producer's document.
      expect(validateSceneFrame(value), name).toEqual(value as SceneFrame)
    }
  })

  it('reads the documented top-level shape from the goldens', () => {
    const root = validateSceneFrame(golden('root.frame.json'))
    expect(root.schema).toBe('dsh-uml-viewer/scene-frame')
    expect(root.version).toBe(1)
    expect(root.viewport.width).toBeGreaterThan(0)
    expect(['blocks', 'frames', 'full']).toContain(root.lod)
    expect(root.waiting).toBe(false)
    expect(root.style.fonts.canvas).toBe(16)
    expect(root.style.lodZoom.full).toBe(0.4)
    expect(root.style.dimAlpha).toBe(70)
    expect(root.world.length).toBeGreaterThan(0)
    expect(root.edges.length).toBeGreaterThan(0)
    // Chrome left the frame with #46: a view has none, the hover popup alone
    // is left (the `root-hover-frames` golden carries one).
    expect(root.chrome).toEqual([])
  })

  it('carries the hover popup as the frame\'s only chrome', () => {
    const hovered = validateSceneFrame(golden('root-hover-frames.frame.json'))
    expect(hovered.chrome.map(item => item.kind)).toEqual(['popup'])
  })

  it('rejects the five chrome kinds that left the frame (#46)', () => {
    const frame = golden('root.frame.json') as Record<string, unknown>
    for (const kind of ['title', 'proposal-banner', 'back', 'status', 'hint']) {
      const item = kind === 'popup' ? {} : { kind, text: 'x', at: [12, 8], ink: { role: 'muted' } }
      expect(() => validateSceneFrame({ ...frame, chrome: [item] }), kind)
        .toThrow(/unknown chrome kind/)
    }
  })

  it('knows every world item kind the producer emits', () => {
    const kinds = new Set<string>()
    for (const name of goldenFiles) {
      for (const item of validateSceneFrame(golden(name)).world) kinds.add(item.kind)
    }
    // Every produced kind is one the renderer knows; a new kind must be added deliberately.
    for (const kind of kinds) expect(KNOWN_WORLD_KINDS.has(kind), kind).toBe(true)
    expect([...kinds].sort()).toEqual(['band', 'class', 'level', 'marker', 'no-data', 'package', 'port', 'port-link', 'quality', 'rule', 'text'])
  })

  it('carries semantic color roles and quality grades', () => {
    const frame = validateSceneFrame(golden('core-selected.frame.json'))
    const quality = frame.world.find(item => item.kind === 'quality')
    expect(quality?.kind).toBe('quality')
    if (quality?.kind !== 'quality') return
    expect(quality.fill.role).toBe('quality-fill')
    expect(typeof quality.fill.grade).toBe('number')
    expect(quality.stroke.role).toBe('quality-stroke')
    const selected = frame.world.find(item => item.kind === 'class' && item.selected)
    expect(selected?.kind).toBe('class')
    const heads = frame.edges.filter(edge => edge.kind === 'head')
    expect(new Set(heads.map(edge => edge.head))).toEqual(new Set(['open', 'triangle', 'diamond', 'diamond-fill']))
  })

  it.each([
    ['another schema', (frame: Record<string, unknown>) => { frame.schema = 'dsh-uml-viewer/other' }, /schema/],
    ['another version', (frame: Record<string, unknown>) => { frame.version = 2 }, /version/],
    ['a missing world', (frame: Record<string, unknown>) => { delete frame.world }, /world/],
    ['a world that is not an array', (frame: Record<string, unknown>) => { frame.world = {} }, /world/],
    ['an unknown lod tier', (frame: Record<string, unknown>) => { (frame.world as Record<string, unknown>[])[0]!.lod = 'huge' }, /lod/],
    ['a color without a role', (frame: Record<string, unknown>) => {
      (frame.world as Record<string, unknown>[])[0]!.stroke = { grade: 3 }
    }, /role/],
    ['a frame rect with text', (frame: Record<string, unknown>) => { frame.viewport = { width: '1', height: 2 } }, /viewport/],
    ['a camera without zoom', (frame: Record<string, unknown>) => { frame.camera = { x: 0, y: 0 } }, /camera/],
    ['a style without fonts', (frame: Record<string, unknown>) => { frame.style = {} }, /style/],
    ['a non-numeric grade', (frame: Record<string, unknown>) => {
      const item = (frame.world as Record<string, unknown>[]).find(value => value.kind === 'quality')!
      item.fill = { role: 'quality-fill', grade: 'seven' }
    }, /grade/],
    ['a head outside the documented kinds', (frame: Record<string, unknown>) => {
      const head = (frame.edges as Record<string, unknown>[]).find(value => value.kind === 'head')!
      head.head = 'arrow'
    }, /head/],
  ])('rejects %s', (_label, mutate, message) => {
    const frame = golden('core-selected.frame.json') as Record<string, unknown>
    mutate(frame)
    expect(() => validateSceneFrame(frame)).toThrow(message)
  })

  it('accepts an additive unknown world kind and an unknown role, keeping version 1', () => {
    const frame = golden('root.frame.json') as Record<string, unknown>
    const world = frame.world as Record<string, unknown>[]
    world.push({ kind: 'future-item', lod: 'full', bounds: null, dimmed: false, ink: { role: 'future-role' } })
    const parsed = validateSceneFrame(frame)
    expect(parsed.world.at(-1)?.kind).toBe('future-item')
    expect(parsed.version).toBe(1)
  })

  it('validates the waiting canvas the resident opens with', () => {
    const parsed = validateSceneFrame({
      schema: 'dsh-uml-viewer/scene-frame', version: 1,
      viewport: { width: 800, height: 600 }, camera: { x: 0, y: 0, zoom: 1 },
      lod: 'blocks', waiting: true,
      style: { fonts: { canvas: 16, level: 11, marker: 12, badge: 9, port: 10, chrome: 18 },
        widths: { band: 2, rule: 1, dot: 1, portLink: 1.3, dash: 6, dashGap: 4 },
        radius: { class: 4, package: 8, port: 3, rowWash: 2 }, dimAlpha: 70,
        lodZoom: { frames: 0.15, full: 0.4 }, minTextPx: 5, edgeDecimatePx: 8 },
      world: [], edges: [], chrome: [],
    })
    expect(parsed.waiting).toBe(true)
  })
})
