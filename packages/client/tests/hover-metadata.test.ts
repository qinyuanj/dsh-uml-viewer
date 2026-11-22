import { readFileSync } from 'node:fs'
import { describe, expect, it } from 'vitest'
import type { SceneFrame } from '@dsh-uml-viewer/contracts'
import { validateSceneFrame } from '@dsh-uml-viewer/contracts'
import { hoverAt, withHover } from '../src/hover.js'

const golden = (name: string): SceneFrame => JSON.parse(readFileSync(
  new URL(`../../../crates/frame/tests/fixtures/frames/${name}.frame.json`, import.meta.url), 'utf8')) as SceneFrame

const METADATA = {
  complete: true,
  edges: [
    {
      from: ':api', to: ':serde',
      viaIds: [':core.order'],
      rows: [
        { text: 'api -> core.order', violating: true, ink: { role: 'violation' } },
        { text: 'core.order -> serde', violating: false, ink: { role: 'ink' } },
      ],
    },
  ],
  indicators: [{
    id: ':core', points: [[0, 0], [10, 0], [0, 10]] as [number, number][],
    rows: [{ text: 'core -> util', violating: false, ink: { role: 'ink' } }],
  }],
}

const withMetadata = (frame: SceneFrame): SceneFrame => ({ ...frame, hover: METADATA })

describe('resident hover metadata (via-ids and dependency tooltip)', () => {
  it('warms a bundled relation through a class it passes through', () => {
    const frame = golden('root')
    const hovered = withHover(withMetadata(frame), { kind: 'class', id: ':core.order' }, null)
    const warm = hovered.edges.filter(e => e.kind === 'stroke' && e.warm)
    expect(warm.map(e => e.kind === 'stroke' && [e.from, e.to, e.stroke.role])).toEqual([
      [':api', ':serde', 'relation-hover'],
    ])
    expect(withHover(frame, { kind: 'class', id: ':core.order' }, null).edges.some(e => e.kind === 'stroke' && e.warm)).toBe(false)
  })

  it('shows the producer’s tooltip rows for the relation under the pointer', () => {
    const frame = withMetadata(golden('root'))
    const hit = hoverAt(frame, [0, 0])
    expect(hit).toBeNull()
    const stroke = frame.edges.find(e => e.kind === 'stroke' && e.from === ':api' && e.to === ':serde')
    if (!stroke || stroke.kind !== 'stroke') throw new Error('api -> serde')
    const [a, b] = [stroke.points[0]!, stroke.points[1]!]
    const mid = [(a[0] + b[0]) / 2, (a[1] + b[1]) / 2] as const
    const hovered = withHover(frame, hoverAt(frame, mid), [300, 240])
    const popup = hovered.chrome.at(-1)!
    expect(popup.kind).toBe('popup')
    if (popup.kind !== 'popup') throw new Error('popup')
    expect(popup.rows).toEqual(METADATA.edges[0]!.rows)
    expect(popup.fill).toEqual({ role: 'popup' })
    expect(popup.border).toEqual({ role: 'line' })
    expect(popup.rect).toEqual({ x: 300, y: 256, w: 187, h: 56 })
  })

  it('shows no tooltip when the producer sent no metadata', () => {
    const frame = golden('root')
    const stroke = frame.edges.find(e => e.kind === 'stroke' && e.from === ':api' && e.to === ':serde')
    if (!stroke || stroke.kind !== 'stroke') throw new Error('api -> serde')
    const [a, b] = [stroke.points[0]!, stroke.points[1]!]
    const hovered = withHover(frame, hoverAt(frame, [(a[0] + b[0]) / 2, (a[1] + b[1]) / 2]), [300, 240])
    expect(hovered.chrome.some(c => c.kind === 'popup')).toBe(false)
  })

  it('hovers a dependency indicator for its tooltip without warming any relation', () => {
    const frame = withMetadata(golden('core-triangles-blocks'))
    const indicator = frame.edges.find(e => e.kind === 'indicator')!
    if (indicator.kind !== 'indicator') throw new Error('indicator')
    const center = [
      (indicator.points[0]![0] + indicator.points[1]![0] + indicator.points[2]![0]) / 3,
      (indicator.points[0]![1] + indicator.points[1]![1] + indicator.points[2]![1]) / 3,
    ] as const
    const hit = hoverAt(frame, center)
    expect(hit).toMatchObject({ kind: 'indicator', id: indicator.id })
    const hovered = withHover(frame, hit, [100, 100])
    expect(hovered.edges).toEqual(frame.edges)
    expect(hovered.chrome.some(c => c.kind === 'popup')).toBe(false)
    const named = { ...frame, hover: { ...METADATA, indicators: [{ ...METADATA.indicators[0]!, id: indicator.id }] } }
    expect(withHover(named, hit, [100, 100]).chrome.some(c => c.kind === 'popup')).toBe(false)
    const triangle = indicator.points
    const both = { ...frame, hover: { ...METADATA, indicators: [
      { ...METADATA.indicators[0]!, points: [[0, 0], [1, 0], [0, 1]] as [number, number][] },
      { ...METADATA.indicators[0]!, id: indicator.id, points: triangle },
    ] } }
    expect(withHover(both, hit, [100, 100]).chrome.at(-1)).toMatchObject({ kind: 'popup', rows: METADATA.indicators[0]!.rows })
    const points = both.hover.indicators
    expect(points[0]!.points).not.toEqual(points[1]!.points)
  })
})

describe('scene-frame hover contract', () => {
  const base = (): Record<string, unknown> => JSON.parse(JSON.stringify(golden('root'))) as Record<string, unknown>

  it('keeps the golden frame valid without the optional field', () => {
    expect(validateSceneFrame(base()).hover).toBeUndefined()
  })

  it('accepts the bounded resident capability and keeps its rows', () => {
    const frame = validateSceneFrame({ ...base(), hover: METADATA })
    expect(frame.hover).toEqual(METADATA)
  })

  it('refuses a partial capability and every producer bound', () => {
    const items = (count: number) => Array.from({ length: count }, () => ({ from: ':a', to: ':b', viaIds: [], rows: [] }))
    const rows = (count: number) => Array.from({ length: count }, () => ({ text: 'a -> b', violating: false, ink: { role: 'ink' } }))
    const bad = [
      { complete: false, edges: METADATA.edges, indicators: [] },
      { complete: true, edges: items(1025), indicators: [] },
      { complete: true, edges: [{ from: ':a', to: ':b', viaIds: Array.from({ length: 257 }, (_, i) => `:${i}`), rows: [] }], indicators: [] },
      { complete: true, edges: [{ from: ':a', to: ':b', viaIds: [], rows: rows(129) }], indicators: [] },
      { complete: true, edges: Array.from({ length: 17 }, () => ({ from: ':a', to: ':b', viaIds: [], rows: rows(128) })), indicators: [] },
      { complete: true, edges: [{ from: ':a', to: ':b', viaIds: [], rows: [{ text: 'x'.repeat(4097), violating: false, ink: { role: 'ink' } }] }], indicators: [] },
      { complete: true, edges: Array.from({ length: 16 }, () => ({
        from: ':a', to: ':b', viaIds: [],
        rows: rows(128).map(row => ({ ...row, text: 'y'.repeat(4000) })),
      })), indicators: [] },
      { complete: true, edges: [{ from: 'x'.repeat(4097), to: ':b', viaIds: [], rows: [] }], indicators: [] },
      { complete: true, edges: [], indicators: [{ id: ':a', points: [[0, 0], [1, 1]], rows: [] }] },
    ]
    for (const hover of bad) expect(() => validateSceneFrame({ ...base(), hover }), JSON.stringify(hover).slice(0, 60)).toThrow()
    expect(() => validateSceneFrame({ ...base(), hover: {
      complete: true, edges: [], indicators: [{ id: 'x'.repeat(4097), points: [[0, 0], [1, 1], [2, 2]], rows: [] }],
    } })).toThrow(/UTF-8 bytes/)
  })
})
