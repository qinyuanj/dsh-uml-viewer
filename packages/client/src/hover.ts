import type { ChromeItem, FrameHoverRow, SceneFrame } from '@dsh-uml-viewer/contracts'
import { estimateW } from './fit.js'
import { tierOf } from './paint.js'

export type HoverHit =
  | { kind: 'class' | 'port'; id: string }
  | { kind: 'edge'; from: string; to: string }
  | { kind: 'indicator'; id: string; points: readonly Point[] }

export const EDGE_VIOLATING_HOT = 3.2
const EDGE_PAD = 8
const INDICATOR_PAD = 5
const POPUP_BODY = 18

type Point = readonly [number, number]

function segmentDistance(p: Point, a: Point, b: Point): number {
  const dx = b[0] - a[0]
  const dy = b[1] - a[1]
  const len2 = dx * dx + dy * dy
  const t = len2 === 0 ? 0 : Math.max(0, Math.min(1, ((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / len2))
  return Math.hypot(p[0] - (a[0] + t * dx), p[1] - (a[1] + t * dy))
}

function nearPolyline(p: Point, points: readonly Point[], pad: number): boolean {
  for (let i = 1; i < points.length; i += 1) {
    if (segmentDistance(p, points[i - 1]!, points[i]!) <= pad) return true
  }
  return false
}

function nearTriangle(p: Point, t: readonly Point[], pad: number): boolean {
  const sign = (a: Point, b: Point): number => (p[0] - b[0]) * (a[1] - b[1]) - (a[0] - b[0]) * (p[1] - b[1])
  const a = sign(t[0]!, t[1]!)
  const b = sign(t[1]!, t[2]!)
  const c = sign(t[2]!, t[0]!)
  const inside = !((a < 0 || b < 0 || c < 0) && (a > 0 || b > 0 || c > 0))
  return inside || nearPolyline(p, [t[0]!, t[1]!, t[2]!, t[0]!], pad)
}

function contains(r: { x: number; y: number; w: number; h: number }, p: Point): boolean {
  return p[0] >= r.x && p[0] <= r.x + r.w && p[1] >= r.y && p[1] <= r.y + r.h
}

export function hoverAt(frame: SceneFrame, world: Point): HoverHit | null {
  for (const edge of frame.edges) {
    if (edge.kind === 'indicator' && nearTriangle(world, edge.points, INDICATOR_PAD)) return { kind: 'indicator', id: edge.id, points: edge.points }
  }
  for (let index = frame.world.length - 1; index >= 0; index -= 1) {
    const item = frame.world[index]!
    if (item.kind === 'port' && item.owner !== null && contains(item.rect, world)) return { kind: 'port', id: item.owner }
    if (item.kind === 'class' && contains(item.rect, world)) return { kind: 'class', id: item.id }
  }
  for (let index = frame.edges.length - 1; index >= 0; index -= 1) {
    const edge = frame.edges[index]!
    if (edge.kind === 'stroke' && nearPolyline(world, edge.points, EDGE_PAD)) return { kind: 'edge', from: edge.from, to: edge.to }
  }
  return null
}

export function sameHover(a: HoverHit | null, b: HoverHit | null): boolean {
  if (a === null || b === null) return a === b
  if (a.kind === 'edge') return b.kind === 'edge' && a.from === b.from && a.to === b.to
  if (a.kind === 'indicator') return b.kind === 'indicator' && a.id === b.id && samePoints(a.points, b.points)
  return b.kind === a.kind && a.id === b.id
}

function samePoints(a: readonly Point[], b: readonly Point[]): boolean {
  return a.length === b.length && a.every((point, index) => point[0] === b[index]![0] && point[1] === b[index]![1])
}

const VIOLATING = new Set(['violation', 'violation-hot'])

export function withHover(frame: SceneFrame, hit: HoverHit | null, pointer: Point | null): SceneFrame {
  if (hit === null) return frame
  const hoveredClass = hit.kind === 'class' || hit.kind === 'port' ? hit.id : null
  const via = viaIds(frame)
  const edges = frame.edges.map(edge => {
    if (edge.kind === 'indicator') return edge
    const hot = hit.kind === 'edge' && edge.from === hit.from && edge.to === hit.to
    const warm = !hot && hoveredClass !== null
      && (edge.from === hoveredClass || edge.to === hoveredClass
        || via.get(relationKey(edge.from, edge.to))?.includes(hoveredClass) === true)
    if (!hot && !warm) return edge
    const violating = VIOLATING.has(edge.stroke.role)
    return {
      ...edge,
      stroke: { role: violating ? 'violation-hot' : 'relation-hover' },
      width: violating ? EDGE_VIOLATING_HOT : edge.width,
      hot,
      warm,
      dimmed: false,
    }
  })
  const world = hoveredClass === null
    ? frame.world
    : frame.world.map(item => (item.kind === 'class' && item.id === hoveredClass ? { ...item, hovered: true } : item))
  const popup = pointer === null ? null : hoverPopup(frame, hit, pointer)
  return { ...frame, world, edges, chrome: popup ? withPopup(frame.chrome, popup) : frame.chrome }
}

function withPopup(chrome: readonly ChromeItem[], popup: ChromeItem): ChromeItem[] {
  return [...chrome, popup]
}

function relationKey(from: string, to: string): string {
  return `${from}\u0000${to}`
}

function viaIds(frame: SceneFrame): Map<string, readonly string[]> {
  const map = new Map<string, readonly string[]>()
  if (frame.hover?.complete !== true) return map
  for (const edge of frame.hover.edges) map.set(relationKey(edge.from, edge.to), edge.viaIds)
  return map
}

function hoverRows(frame: SceneFrame, hit: HoverHit): readonly FrameHoverRow[] {
  if (frame.hover?.complete !== true) return []
  if (hit.kind === 'edge') {
    return frame.hover.edges.find(edge => edge.from === hit.from && edge.to === hit.to)?.rows ?? []
  }
  if (hit.kind === 'indicator') return frame.hover.indicators.find(item => item.id === hit.id && samePoints(item.points, hit.points))?.rows ?? []
  return []
}

export function showsPopup(frame: SceneFrame, hit: HoverHit | null): boolean {
  if (hit === null) return false
  if (hoverRows(frame, hit).length > 0) return true
  return hit.kind === 'class' && tierOf(frame) !== 'full'
    && frame.world.some(item => item.kind === 'text' && item.line === 'name' && item.owner === hit.id && item.text)
}

function hoverPopup(frame: SceneFrame, hit: HoverHit, pointer: Point): ChromeItem | null {
  const rows = hoverRows(frame, hit)
  if (rows.length > 0) return popup(frame, rows, pointer)
  if (hit.kind === 'class' && tierOf(frame) !== 'full') {
    const name = frame.world.find(item => item.kind === 'text' && item.line === 'name' && item.owner === hit.id)
    if (name && name.kind === 'text' && name.text) {
      return popup(frame, [{ text: name.text, violating: false, ink: { role: 'ink' } }], pointer)
    }
  }
  return null
}

function popup(frame: SceneFrame, rows: readonly FrameHoverRow[], [mx, my]: Point): ChromeItem {
  const { width, height } = frame.viewport
  const textW = Math.max(40, ...rows.map(row => estimateW(row.text, POPUP_BODY)))
  const w = Math.min(textW + 16, Math.max(16, width - 16))
  const h = 16 + rows.length * (POPUP_BODY + 2)
  const x = Math.max(8, Math.min(mx, width - w - 8))
  let y = my + 16
  if (y + h > height) y = Math.max(8, my - h - 8)
  return { kind: 'popup', rect: { x, y, w, h }, fill: { role: 'popup' }, border: { role: 'line' }, rows }
}
