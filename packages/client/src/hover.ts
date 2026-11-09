/**
 * 关系悬停高亮, client-side (ADR 0013: hover never round-trips the host).
 *
 * The resident's frame already carries the selection's 依赖链高亮 and 关系淡化
 * (`selected`, `dimmed`, the `dependency-chain` role); this module only adds the
 * temporary hover feedback on top of it, for what the pointer is over:
 *
 * - `hoverAt` is `scene::hit::at` over the frame's own geometry, reduced to what
 *   hover highlighting reads (`scene::highlight::box_id`): a class (a port
 *   stands for its class), the relation under the pointer, or nothing.
 * - `withHover` is the hover half of `frame::commands::edge_style` and
 *   `hover_rows`: an ordinary relation touching the hovered class, or the one
 *   under the pointer, turns `relation-hover`; a violating one `violation-hot`
 *   at the hot violating width. Hovered relations are never dimmed. At the
 *   overview tiers a hovered class is named in the chrome popup.
 *
 * No dependency rule is evaluated here. Whether a relation violates comes from
 * the role the producer already gave it, and selection/chain state is the
 * resident's. A closed head keeps its producer fill; only its outline changes.
 */
import type { ChromeItem, SceneFrame } from '@dsh-uml-viewer/contracts'
import { estimateW } from './fit.js'
import { tierOf } from './paint.js'

export type HoverHit =
  | { kind: 'class' | 'port'; id: string }
  | { kind: 'edge'; from: string; to: string }

/** `frame::theme::widths::EDGE_VIOLATING_HOT`: a hovered violating relation. */
export const EDGE_VIOLATING_HOT = 3.2
/** `scene::hit::at` pads, in world units: relations 8, dependency indicators 5. */
const EDGE_PAD = 8
const INDICATOR_PAD = 5
/** `frame::theme::fonts::BODY`, the popup's estimate font (`commands::chrome`). */
const POPUP_BODY = 18

type Point = readonly [number, number]

function segmentDistance(p: Point, a: Point, b: Point): number {
  const dx = b[0] - a[0]
  const dy = b[1] - a[1]
  const len2 = dx * dx + dy * dy
  const t = len2 === 0 ? 0 : Math.max(0, Math.min(1, ((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / len2))
  return Math.hypot(p[0] - (a[0] + t * dx), p[1] - (a[1] + t * dy))
}

/** `geom::near_polyline`. */
function nearPolyline(p: Point, points: readonly Point[], pad: number): boolean {
  for (let i = 1; i < points.length; i += 1) {
    if (segmentDistance(p, points[i - 1]!, points[i]!) <= pad) return true
  }
  return false
}

/** `geom::near_triangle`: inside, or within `pad` of an edge. */
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

/**
 * What hover highlighting sees at a world point, in `scene::hit::at` order:
 * dependency indicators, then (topmost first) each class's ports and its box,
 * then relations, then packages. Indicators and packages highlight nothing.
 */
export function hoverAt(frame: SceneFrame, world: Point): HoverHit | null {
  for (const edge of frame.edges) {
    if (edge.kind === 'indicator' && nearTriangle(world, edge.points, INDICATOR_PAD)) return null
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
  if (a.kind !== 'edge') return b.kind === a.kind && a.id === b.id
  return b.kind === 'edge' && a.from === b.from && a.to === b.to
}

const VIOLATING = new Set(['violation', 'violation-hot'])

/**
 * The frame as painted under the pointer. `pointer` is in viewport points and
 * places the overview popup; `null` means no popup. Returns `frame` itself
 * when nothing is hovered, and never mutates it.
 */
export function withHover(frame: SceneFrame, hit: HoverHit | null, pointer: Point | null): SceneFrame {
  if (hit === null) return frame
  const hoveredClass = hit.kind !== 'edge' ? hit.id : null
  const edges = frame.edges.map(edge => {
    if (edge.kind === 'indicator') return edge
    const hot = hit.kind === 'edge' && edge.from === hit.from && edge.to === hit.to
    const warm = !hot && hoveredClass !== null && (edge.from === hoveredClass || edge.to === hoveredClass)
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
  const popup = hit.kind === 'class' && pointer !== null && tierOf(frame) !== 'full' ? namePopup(frame, hit.id, pointer) : null
  return { ...frame, world, edges, chrome: popup ? [...frame.chrome, popup] : frame.chrome }
}

/**
 * `commands::chrome`'s popup for a hovered class at the overview tiers, whose
 * box is too narrow to show the name: one ink row, below and right of the
 * pointer, kept inside the viewport.
 */
function namePopup(frame: SceneFrame, id: string, [mx, my]: Point): ChromeItem | null {
  const name = frame.world.find(item => item.kind === 'text' && item.line === 'name' && item.owner === id)
  if (!name || name.kind !== 'text' || !name.text) return null
  const rows = [{ text: name.text, violating: false, ink: { role: 'ink' } }]
  const { width, height } = frame.viewport
  const textW = Math.max(40, ...rows.map(row => estimateW(row.text, POPUP_BODY)))
  const w = Math.min(textW + 16, Math.max(16, width - 16))
  const h = 16 + rows.length * (POPUP_BODY + 2)
  const x = Math.max(8, Math.min(mx, width - w - 8))
  let y = my + 16
  if (y + h > height) y = Math.max(8, my - h - 8)
  return { kind: 'popup', rect: { x, y, w, h }, fill: { role: 'popup' }, border: { role: 'line' }, rows }
}
