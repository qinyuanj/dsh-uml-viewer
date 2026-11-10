/**
 * Validation for the Rust-owned scene frame (`docs/scene-frame.md`).
 *
 * The producer owns the format; this module only checks it. Additive fields,
 * item kinds and roles stay compatible with version 1: unknown item kinds are
 * kept (the renderer skips them) and unknown roles render as `ink`. Nothing
 * here invents a second scene model or rewrites what the producer wrote.
 */

export const SCENE_FRAME_SCHEMA = 'dsh-uml-viewer/scene-frame'
export const SCENE_FRAME_VERSION = 1

export type Lod = 'blocks' | 'frames' | 'full'

/** A color is a semantic role, optionally graded (quality) or ratio-bearing (coverage). */
export interface FrameColor {
  role: string
  grade?: number | null
  ratio?: number | null
}

export interface FrameRect { x: number; y: number; w: number; h: number }
export type FramePoint = [number, number]

export interface FrameItem {
  kind: string
  lod: Lod
  bounds: FrameRect | null
  dimmed: boolean
}

export interface PackageItem extends FrameItem {
  kind: 'package'
  id: string
  rect: FrameRect
  text: string | null
  /** The producer's quality band name (`red` when no quality data is measured). */
  band: string
  size: number
  maxW: number
  missing: boolean
  missingCount: number | null
  fill: FrameColor
  stroke: FrameColor
  strokeWidth: number
  ink: FrameColor
  grade: number | null
  selected: boolean
  related: boolean
}

export interface ClassItem extends FrameItem {
  kind: 'class'
  id: string
  owner: string | null
  rect: FrameRect
  shape: string
  band: string
  fill: FrameColor
  stroke: FrameColor
  strokeWidth: number
  grade: number | null
  missing: boolean
  selected: boolean
  hovered: boolean
  related: boolean
}

export interface QualityItem extends FrameItem {
  kind: 'quality'
  id: string
  owner: string | null
  of: string
  text: string
  band: string
  center: FramePoint
  radius: number
  fill: FrameColor
  stroke: FrameColor
  ink: FrameColor
  grade: number | null
}

export interface MarkerItem extends FrameItem {
  kind: 'marker'
  owner: string | null
  text: string
  at: FramePoint
  size: number
  ink: FrameColor
}

export interface LevelItem extends FrameItem {
  kind: 'level'
  owner: string | null
  text: string
  at: FramePoint
  size: number
  ink: FrameColor
}

export interface BandItem extends FrameItem {
  kind: 'band'
  owner: string | null
  from: FramePoint
  to: FramePoint
  stroke: FrameColor
}

export interface RuleItem extends FrameItem {
  kind: 'rule'
  owner: string | null
  from: FramePoint
  to: FramePoint
  stroke: FrameColor
}

export interface TextItem extends FrameItem {
  kind: 'text'
  owner: string | null
  line: string
  text: string | null
  x: number
  top: number
  slot: number
  size: number
  maxW: number
  italic: boolean
  bold: boolean
  center: boolean
  ink: FrameColor
}

export interface NoDataItem extends FrameItem {
  kind: 'no-data'
  owner: string | null
  text: string
  right: number
  y: number
  size: number
  ink: FrameColor
}

export interface ChildWashItem extends FrameItem {
  kind: 'child-wash'
  owner: string | null
  rect: FrameRect
  marker: { wash: FrameColor; bar: FrameColor; barWidth: number }
}

export interface PortLinkItem extends FrameItem {
  kind: 'port-link'
  owner: string | null
  behind: FramePoint
  tip: FramePoint
  stroke: FrameColor
}

export interface PortItem extends FrameItem {
  kind: 'port'
  id: string
  owner: string | null
  rect: FrameRect
  text: string
  marked: boolean
  fill: FrameColor
  stroke: FrameColor
  strokeWidth: number
  ink: FrameColor
}

/**
 * The documented version-1 items. An additively added kind is kept in the
 * document (so a re-serialization is lossless) but typed as the union member
 * whose fields the renderer ignores; the renderer's `default` branch skips it.
 */
export type WorldItem =
  | PackageItem | ClassItem | QualityItem | MarkerItem | LevelItem | BandItem | RuleItem | TextItem
  | NoDataItem | ChildWashItem | PortLinkItem | PortItem

/** The kinds the producer documents for version 1; anything else is skipped. */
export const KNOWN_WORLD_KINDS: ReadonlySet<string> = new Set([
  'package', 'class', 'quality', 'marker', 'level', 'band', 'rule', 'text',
  'no-data', 'child-wash', 'port-link', 'port',
])

export interface StrokeEdge {
  kind: 'stroke'
  lod: Lod
  from: string
  to: string
  points: FramePoint[]
  stroke: FrameColor
  width: number
  hot: boolean
  warm: boolean
  dimmed: boolean
}

export type HeadKind = 'open' | 'triangle' | 'diamond' | 'diamond-fill'

export interface HeadEdge {
  kind: 'head'
  lod: Lod
  from: string
  to: string
  head: HeadKind
  tip: FramePoint
  behind: FramePoint
  stroke: FrameColor
  fill: FrameColor
  width: number
  hot: boolean
  warm: boolean
  dimmed: boolean
}

export interface IndicatorEdge {
  kind: 'indicator'
  lod: Lod
  id: string
  points: FramePoint[]
  violating: boolean
  fill: FrameColor
  dimmed: boolean
}

export type FrameEdge = StrokeEdge | HeadEdge | IndicatorEdge

/**
 * The frame's chrome, version 1 after #46: only the pointer's hover popup. The
 * title, the proposal banner, the return line, the status line and the centred
 * hint left the frame with ADR 0015 — the panel draws them as DOM (top bar,
 * status bar) — so a frame that still carries one of those kinds is a broken
 * resident, not a frame to draw.
 */
export interface ChromePopup {
  kind: 'popup'
  rect: FrameRect
  fill: FrameColor
  border: FrameColor
  rows: readonly { text: string; violating: boolean; ink: FrameColor }[]
}

export type ChromeItem = ChromePopup

/** The chrome kinds a frame may carry: the hover popup alone (#46). */
export const CHROME_KINDS: readonly ChromeItem['kind'][] = ['popup']

export interface FrameStyle {
  fonts: { canvas: number; level: number; marker: number; badge: number; port: number; chrome: number }
  widths: { band: number; rule: number; dot: number; portLink: number; dash: number; dashGap: number }
  radius: { class: number; package: number; port: number; rowWash: number }
  dimAlpha: number
  lodZoom: { frames: number; full: number }
  minTextPx: number
  edgeDecimatePx: number
}

/**
 * The optional bounded hover metadata of a resident frame (#26), additive to
 * version 1: the dependency-tooltip rows and the bundled-edge `via-ids` the
 * strokes cannot carry. `complete: false` means a producer bound was exceeded
 * and no hover metadata is available at all; the client then falls back to the
 * endpoints alone, exactly as for an older producer that omits the field.
 */
export interface FrameHoverRow {
  text: string
  violating: boolean
  ink: FrameColor
}

export interface FrameHoverEdge {
  from: string
  to: string
  /** Classes this bundled relation passes through (`scene::highlight::touches`). */
  viaIds: readonly string[]
  /** The dependency tooltip rows, violations first (`commands::dependency_rows`). */
  rows: readonly FrameHoverRow[]
}

export interface FrameHoverIndicator {
  id: string
  points: readonly FramePoint[]
  rows: readonly FrameHoverRow[]
}

export interface FrameHover {
  complete: boolean
  edges: readonly FrameHoverEdge[]
  indicators: readonly FrameHoverIndicator[]
}

/** The kinds of swatch a legend row can draw (`frame::legend::Sample`). */
export const LEGEND_SAMPLE_KINDS = ['box', 'line', 'dot', 'ramp'] as const

/**
 * The legend the resident typesets (#50, ADR 0015): what the diagram's boxes,
 * quality colors and relation colors mean, worded in the frame's language. The
 * panel draws it as its own DOM — a popover from 「更多」, resident from 900px
 * of panel width — and resolves a sample's roles through the same theme as the
 * canvas, so a legend color is the diagram's color.
 */
export type LegendShape = 'rect' | 'oval'

/** A class or component box: its fill, its frame and its outline's shape. */
export interface LegendBoxSample {
  kind: 'box'
  shape: LegendShape
  fill: FrameColor
  stroke: FrameColor
  dimmed: boolean
}

/** A relation line in one of its interactive states' colors. */
export interface LegendLineSample {
  kind: 'line'
  stroke: FrameColor
  dimmed: boolean
}

/** A marker dot, 无数据. */
export interface LegendDotSample {
  kind: 'dot'
  fill: FrameColor
}

/** The continuous 质量色 ramp: sample stops, worst to best. */
export interface LegendRampSample {
  kind: 'ramp'
  stops: FrameColor[]
}

/**
 * A sample the renderer draws. An additive sample kind stays version 1: the
 * validator keeps its fields and the panel draws the row's label without a
 * swatch, exactly as it skips an unknown item kind.
 */
export type LegendSample = LegendBoxSample | LegendLineSample | LegendDotSample | LegendRampSample

export interface LegendRow {
  /** Stable identity of the explained thing (`class-box`, `relation`, …). */
  id: string
  /** The resident's label, in the frame's language. */
  label: string
  sample: LegendSample
}

export interface LegendSection {
  id: string
  title: string
  rows: LegendRow[]
}

export interface Legend {
  sections: LegendSection[]
}

export interface SceneFrame {
  schema: typeof SCENE_FRAME_SCHEMA
  version: typeof SCENE_FRAME_VERSION
  viewport: { width: number; height: number }
  camera: { x: number; y: number; zoom: number }
  lod: Lod
  waiting: boolean
  style: FrameStyle
  world: WorldItem[]
  edges: FrameEdge[]
  chrome: ChromeItem[]
  /** What the drawing means, worded in the frame's language (#50). */
  legend: Legend
  /** Optional capability; absent from the goldens (`scene --frame`). */
  hover?: FrameHover
}

/** Producer bounds the validator enforces (`crates/frame/src/hover.rs`). */
export const HOVER_LIMITS = {
  items: 1024,
  viaIds: 256,
  rowsPerItem: 128,
  rows: 2048,
  bytes: 4096,
  metadataBytes: 512 * 1024,
} as const

const LODS: ReadonlySet<string> = new Set(['blocks', 'frames', 'full'])
const HEADS: ReadonlySet<string> = new Set(['open', 'triangle', 'diamond', 'diamond-fill'])

class Invalid extends Error {}

function at(label: string, message: string): never {
  throw new Invalid(`${label}: ${message}`)
}

function record(value: unknown, label: string): Record<string, unknown> {
  if (!value || typeof value !== 'object' || Array.isArray(value)) at(label, 'must be an object')
  return value as Record<string, unknown>
}

function str(value: unknown, label: string): string {
  if (typeof value !== 'string') at(label, 'must be a string')
  return value
}

function num(value: unknown, label: string): number {
  if (typeof value !== 'number' || !Number.isFinite(value)) at(label, 'must be a finite number')
  return value
}

function bool(value: unknown, label: string): boolean {
  if (typeof value !== 'boolean') at(label, 'must be a boolean')
  return value
}

function list(value: unknown, label: string): unknown[] {
  if (!Array.isArray(value)) at(label, 'must be an array')
  return value
}

function rect(value: unknown, label: string): FrameRect {
  const source = record(value, label)
  return { x: num(source.x, `${label}.x`), y: num(source.y, `${label}.y`), w: num(source.w, `${label}.w`), h: num(source.h, `${label}.h`) }
}

function optionalRect(value: unknown, label: string): FrameRect | null {
  return value === null || value === undefined ? null : rect(value, label)
}

function point(value: unknown, label: string): FramePoint {
  const source = list(value, label)
  if (source.length !== 2) at(label, 'must hold two world coordinates')
  return [num(source[0], `${label}[0]`), num(source[1], `${label}[1]`)]
}

function points(value: unknown, label: string): FramePoint[] {
  return list(value, label).map((entry, index) => point(entry, `${label}[${index}]`))
}

function color(value: unknown, label: string): FrameColor {
  const source = record(value, label)
  const parsed: FrameColor = { role: str(source.role, `${label}.role`) }
  if (source.grade !== undefined) parsed.grade = source.grade === null ? null : num(source.grade, `${label}.grade`)
  if (source.ratio !== undefined) parsed.ratio = source.ratio === null ? null : num(source.ratio, `${label}.ratio`)
  return parsed
}

const LEGEND_SHAPES: ReadonlySet<string> = new Set(['rect', 'oval'])

function legendShape(value: unknown, label: string): LegendShape {
  const shape = str(value, label)
  if (!LEGEND_SHAPES.has(shape)) at(label, `unknown legend shape ${JSON.stringify(shape)}`)
  return shape as LegendShape
}

/** One row's swatch: the diagram's own roles, so one theme resolves both. */
function legendSample(value: unknown, label: string): LegendSample {
  const source = record(value, label)
  const kind = str(source.kind, `${label}.kind`)
  switch (kind) {
    case 'box':
      return {
        kind,
        shape: legendShape(source.shape, `${label}.shape`),
        fill: color(source.fill, `${label}.fill`),
        stroke: color(source.stroke, `${label}.stroke`),
        dimmed: bool(source.dimmed, `${label}.dimmed`),
      }
    case 'line':
      return {
        kind,
        stroke: color(source.stroke, `${label}.stroke`),
        dimmed: bool(source.dimmed, `${label}.dimmed`),
      }
    case 'dot':
      return { kind, fill: color(source.fill, `${label}.fill`) }
    case 'ramp':
      return { kind, stops: list(source.stops, `${label}.stops`).map((stop, index) => color(stop, `${label}.stops[${index}]`)) }
    default:
      // Compatible additive kind: the fields travel through unchanged and the
      // panel keeps the row's label while drawing no swatch.
      return { ...source, kind } as LegendSample
  }
}

function legend(value: unknown, label: string): Legend {
  const source = record(value, label)
  return {
    sections: list(source.sections, `${label}.sections`).map((section, index) => {
      const item = record(section, `${label}.sections[${index}]`)
      const id = `${label}.sections[${index}]`
      return {
        id: str(item.id, `${id}.id`),
        title: str(item.title, `${id}.title`),
        rows: list(item.rows, `${id}.rows`).map((row, n) => {
          const entry = record(row, `${id}.rows[${n}]`)
          return {
            id: str(entry.id, `${id}.rows[${n}].id`),
            label: str(entry.label, `${id}.rows[${n}].label`),
            sample: legendSample(entry.sample, `${id}.rows[${n}].sample`),
          }
        }),
      }
    }),
  }
}

function lod(value: unknown, label: string): Lod {
  const tier = str(value, label)
  if (!LODS.has(tier)) at(label, `unknown level of detail ${JSON.stringify(tier)}`)
  return tier as Lod
}

function itemBase(source: Record<string, unknown>, label: string): FrameItem {
  return {
    kind: str(source.kind, `${label}.kind`),
    lod: lod(source.lod, `${label}.lod`),
    bounds: optionalRect(source.bounds, `${label}.bounds`),
    dimmed: source.dimmed === undefined ? false : bool(source.dimmed, `${label}.dimmed`),
  }
}

function worldItem(value: unknown, label: string): WorldItem {
  const source = record(value, label)
  const base = itemBase(source, label)
  const ink = () => color(source.ink, `${label}.ink`)
  switch (base.kind) {
    case 'package':
      return { ...base, kind: 'package', id: str(source.id, `${label}.id`), rect: rect(source.rect, `${label}.rect`),
        text: source.text === null ? null : str(source.text, `${label}.text`),
        band: str(source.band, `${label}.band`), size: num(source.size, `${label}.size`),
        maxW: num(source.maxW, `${label}.maxW`), missing: bool(source.missing, `${label}.missing`),
        missingCount: source.missingCount === null ? null : num(source.missingCount, `${label}.missingCount`),
        fill: color(source.fill, `${label}.fill`), stroke: color(source.stroke, `${label}.stroke`),
        strokeWidth: num(source.strokeWidth, `${label}.strokeWidth`), ink: ink(),
        grade: source.grade === null ? null : num(source.grade, `${label}.grade`),
        selected: bool(source.selected, `${label}.selected`), related: bool(source.related, `${label}.related`) }
    case 'class':
      return { ...base, kind: 'class', id: str(source.id, `${label}.id`),
        owner: source.owner === null ? null : str(source.owner, `${label}.owner`),
        rect: rect(source.rect, `${label}.rect`), shape: str(source.shape, `${label}.shape`),
        band: str(source.band, `${label}.band`), fill: color(source.fill, `${label}.fill`),
        stroke: color(source.stroke, `${label}.stroke`), strokeWidth: num(source.strokeWidth, `${label}.strokeWidth`),
        grade: source.grade === null ? null : num(source.grade, `${label}.grade`),
        missing: bool(source.missing, `${label}.missing`), selected: bool(source.selected, `${label}.selected`),
        hovered: bool(source.hovered, `${label}.hovered`), related: bool(source.related, `${label}.related`) }
    case 'quality':
      return { ...base, kind: 'quality', id: str(source.id, `${label}.id`),
        owner: source.owner === null ? null : str(source.owner, `${label}.owner`),
        of: str(source.of, `${label}.of`), text: str(source.text, `${label}.text`),
        band: str(source.band, `${label}.band`), center: point(source.center, `${label}.center`),
        radius: num(source.radius, `${label}.radius`), fill: color(source.fill, `${label}.fill`),
        stroke: color(source.stroke, `${label}.stroke`), ink: ink(),
        grade: source.grade === null ? null : num(source.grade, `${label}.grade`) }
    case 'marker':
    case 'level':
      return { ...base, kind: base.kind as 'marker' | 'level',
        owner: source.owner === null ? null : str(source.owner, `${label}.owner`),
        text: str(source.text, `${label}.text`), at: point(source.at, `${label}.at`),
        size: num(source.size, `${label}.size`), ink: ink() } as WorldItem
    case 'band':
    case 'rule':
      return { ...base, kind: base.kind as 'band' | 'rule',
        owner: source.owner === null ? null : str(source.owner, `${label}.owner`),
        from: point(source.from, `${label}.from`), to: point(source.to, `${label}.to`),
        stroke: color(source.stroke, `${label}.stroke`) } as WorldItem
    case 'text':
      return { ...base, kind: 'text', owner: source.owner === null ? null : str(source.owner, `${label}.owner`),
        line: str(source.line, `${label}.line`), text: source.text === null ? null : str(source.text, `${label}.text`),
        x: num(source.x, `${label}.x`), top: num(source.top, `${label}.top`), slot: num(source.slot, `${label}.slot`),
        size: num(source.size, `${label}.size`), maxW: num(source.maxW, `${label}.maxW`),
        italic: bool(source.italic, `${label}.italic`), bold: bool(source.bold, `${label}.bold`),
        center: bool(source.center, `${label}.center`), ink: ink() }
    case 'no-data':
      return { ...base, kind: 'no-data', owner: source.owner === null ? null : str(source.owner, `${label}.owner`),
        text: str(source.text, `${label}.text`), right: num(source.right, `${label}.right`),
        y: num(source.y, `${label}.y`), size: num(source.size, `${label}.size`), ink: ink() }
    case 'child-wash': {
      const marker = record(source.marker, `${label}.marker`)
      return { ...base, kind: 'child-wash', owner: source.owner === null ? null : str(source.owner, `${label}.owner`),
        rect: rect(source.rect, `${label}.rect`),
        marker: { wash: color(marker.wash, `${label}.marker.wash`),
          bar: color(marker.bar, `${label}.marker.bar`), barWidth: num(marker.barWidth, `${label}.marker.barWidth`) } }
    }
    case 'port-link':
      return { ...base, kind: 'port-link', owner: source.owner === null ? null : str(source.owner, `${label}.owner`),
        behind: point(source.behind, `${label}.behind`), tip: point(source.tip, `${label}.tip`),
        stroke: color(source.stroke, `${label}.stroke`) }
    case 'port':
      return { ...base, kind: 'port', id: str(source.id, `${label}.id`),
        owner: source.owner === null ? null : str(source.owner, `${label}.owner`),
        rect: rect(source.rect, `${label}.rect`), text: str(source.text, `${label}.text`),
        marked: bool(source.marked, `${label}.marked`), fill: color(source.fill, `${label}.fill`),
        stroke: color(source.stroke, `${label}.stroke`), strokeWidth: num(source.strokeWidth, `${label}.strokeWidth`), ink: ink() }
    default:
      // Compatible additive kind: the renderer skips it, so it borrows the base
      // shape while every producer field travels through unchanged.
      return { ...base, ...source, kind: base.kind, lod: base.lod, bounds: base.bounds, dimmed: base.dimmed } as WorldItem
  }
}

function edge(value: unknown, label: string): FrameEdge {
  const source = record(value, label)
  const kind = str(source.kind, `${label}.kind`)
  const tier = lod(source.lod, `${label}.lod`)
  const dimmed = source.dimmed === undefined ? false : bool(source.dimmed, `${label}.dimmed`)
  switch (kind) {
    case 'stroke':
      return { kind, lod: tier, from: str(source.from, `${label}.from`), to: str(source.to, `${label}.to`),
        points: points(source.points, `${label}.points`), stroke: color(source.stroke, `${label}.stroke`),
        width: num(source.width, `${label}.width`), hot: bool(source.hot, `${label}.hot`),
        warm: bool(source.warm, `${label}.warm`), dimmed }
    case 'head': {
      const head = str(source.head, `${label}.head`)
      if (!HEADS.has(head)) at(`${label}.head`, `unknown arrowhead ${JSON.stringify(head)}`)
      return { kind, lod: tier, from: str(source.from, `${label}.from`), to: str(source.to, `${label}.to`),
        head: head as HeadKind, tip: point(source.tip, `${label}.tip`),
        behind: point(source.behind, `${label}.behind`), stroke: color(source.stroke, `${label}.stroke`),
        fill: color(source.fill, `${label}.fill`), width: num(source.width, `${label}.width`),
        hot: bool(source.hot, `${label}.hot`), warm: bool(source.warm, `${label}.warm`), dimmed }
    }
    case 'indicator':
      return { kind, lod: tier, id: str(source.id, `${label}.id`),
        points: points(source.points, `${label}.points`), violating: bool(source.violating, `${label}.violating`),
        fill: color(source.fill, `${label}.fill`), dimmed }
    default:
      return { ...source, kind, lod: tier, dimmed } as FrameEdge
  }
}

function chrome(value: unknown, label: string): ChromeItem {
  const source = record(value, label)
  const kind = str(source.kind, `${label}.kind`)
  switch (kind) {
    case 'popup':
      return { kind, rect: rect(source.rect, `${label}.rect`), fill: color(source.fill, `${label}.fill`),
        border: color(source.border, `${label}.border`),
        rows: list(source.rows, `${label}.rows`).map((row, index) => {
          const entry = record(row, `${label}.rows[${index}]`)
          return { text: str(entry.text, `${label}.rows[${index}].text`),
            violating: bool(entry.violating, `${label}.rows[${index}].violating`),
            ink: color(entry.ink, `${label}.rows[${index}].ink`) }
        }) }
    default:
      // The five canvas kinds left the frame with #46; a frame that carries
      // one is a resident the panel cannot draw, so say so instead of drawing it.
      return at(`${label}.kind`, `unknown chrome kind ${JSON.stringify(kind)}; the hover popup is the only one`)
  }
}

function utf8Bytes(text: string): number {
  let bytes = 0
  for (const character of text) {
    const code = character.codePointAt(0)!
    bytes += code < 0x80 ? 1 : code < 0x800 ? 2 : code < 0x10000 ? 3 : 4
  }
  return bytes
}

function boundedText(value: unknown, label: string): string {
  const text = str(value, label)
  if (utf8Bytes(text) > HOVER_LIMITS.bytes) at(label, `must be under ${HOVER_LIMITS.bytes} UTF-8 bytes`)
  return text
}

function hoverRows(value: unknown, label: string): FrameHoverRow[] {
  const rows = list(value, label)
  if (rows.length > HOVER_LIMITS.rowsPerItem) at(label, `must hold at most ${HOVER_LIMITS.rowsPerItem} rows`)
  return rows.map((row, index) => {
    const entry = record(row, `${label}[${index}]`)
    return { text: boundedText(entry.text, `${label}[${index}].text`),
      violating: bool(entry.violating, `${label}[${index}].violating`),
      ink: color(entry.ink, `${label}[${index}].ink`) }
  })
}

function hover(value: unknown, label: string): FrameHover {
  const source = record(value, label)
  const complete = bool(source.complete, `${label}.complete`)
  const rawEdges = list(source.edges, `${label}.edges`)
  const rawIndicators = list(source.indicators, `${label}.indicators`)
  if (rawEdges.length + rawIndicators.length > HOVER_LIMITS.items) at(label, `must hold at most ${HOVER_LIMITS.items} items`)
  const edges = rawEdges.map((entry, index) => {
    const item = record(entry, `${label}.edges[${index}]`)
    const via = list(item.viaIds, `${label}.edges[${index}].viaIds`)
    if (via.length > HOVER_LIMITS.viaIds) at(`${label}.edges[${index}].viaIds`, `must hold at most ${HOVER_LIMITS.viaIds} ids`)
    return { from: boundedText(item.from, `${label}.edges[${index}].from`),
      to: boundedText(item.to, `${label}.edges[${index}].to`),
      viaIds: via.map((id, position) => boundedText(id, `${label}.edges[${index}].viaIds[${position}]`)),
      rows: hoverRows(item.rows, `${label}.edges[${index}].rows`) }
  })
  const indicators = rawIndicators.map((entry, index) => {
    const item = record(entry, `${label}.indicators[${index}]`)
    const triangle = points(item.points, `${label}.indicators[${index}].points`)
    if (triangle.length !== 3) at(`${label}.indicators[${index}].points`, 'must hold three points')
    return { id: boundedText(item.id, `${label}.indicators[${index}].id`),
      points: triangle, rows: hoverRows(item.rows, `${label}.indicators[${index}].rows`) }
  })
  if (edges.length + indicators.length > HOVER_LIMITS.items) {
    at(label, `must hold at most ${HOVER_LIMITS.items} edges and indicators`)
  }
  // `complete: false` carries no data at all: the capability is unavailable
  // rather than silently partial (crates/frame/src/hover.rs).
  if (!complete && (edges.length > 0 || indicators.length > 0)) at(label, 'must carry no items when incomplete')
  const rowCount = [...edges, ...indicators].reduce((total, item) => total + item.rows.length, 0)
  if (rowCount > HOVER_LIMITS.rows) at(label, `must hold at most ${HOVER_LIMITS.rows} rows`)
  const parsed = { complete, edges, indicators }
  if (utf8Bytes(JSON.stringify(parsed)) > HOVER_LIMITS.metadataBytes) at(label, 'hover metadata exceeds 512 KiB')
  return parsed
}

function style(value: unknown, label: string): FrameStyle {
  const source = record(value, label)
  const fonts = record(source.fonts, `${label}.fonts`)
  const widths = record(source.widths, `${label}.widths`)
  const radius = record(source.radius, `${label}.radius`)
  const lodZoom = record(source.lodZoom, `${label}.lodZoom`)
  return {
    fonts: { canvas: num(fonts.canvas, `${label}.fonts.canvas`), level: num(fonts.level, `${label}.fonts.level`),
      marker: num(fonts.marker, `${label}.fonts.marker`), badge: num(fonts.badge, `${label}.fonts.badge`),
      port: num(fonts.port, `${label}.fonts.port`), chrome: num(fonts.chrome, `${label}.fonts.chrome`) },
    widths: { band: num(widths.band, `${label}.widths.band`), rule: num(widths.rule, `${label}.widths.rule`),
      dot: num(widths.dot, `${label}.widths.dot`), portLink: num(widths.portLink, `${label}.widths.portLink`),
      dash: num(widths.dash, `${label}.widths.dash`), dashGap: num(widths.dashGap, `${label}.widths.dashGap`) },
    radius: { class: num(radius.class, `${label}.radius.class`), package: num(radius.package, `${label}.radius.package`),
      port: num(radius.port, `${label}.radius.port`), rowWash: num(radius.rowWash, `${label}.radius.rowWash`) },
    dimAlpha: num(source.dimAlpha, `${label}.dimAlpha`),
    lodZoom: { frames: num(lodZoom.frames, `${label}.lodZoom.frames`), full: num(lodZoom.full, `${label}.lodZoom.full`) },
    minTextPx: num(source.minTextPx, `${label}.minTextPx`),
    edgeDecimatePx: num(source.edgeDecimatePx, `${label}.edgeDecimatePx`),
  }
}

/**
 * Validate one scene frame.
 * @param value the wire value the producer wrote.
 * @param label the document name used in failure messages.
 * @returns the same document, narrowed to the documented shape.
 * @throws {Error} when a documented field is absent or mistyped.
 */
export function validateSceneFrame(value: unknown, label = 'scene frame'): SceneFrame {
  try {
    const source = record(value, label)
    if (source.schema !== SCENE_FRAME_SCHEMA) {
      at(`${label}.schema`, `must be ${JSON.stringify(SCENE_FRAME_SCHEMA)}`)
    }
    if (source.version !== SCENE_FRAME_VERSION) {
      at(`${label}.version`, `must be ${SCENE_FRAME_VERSION}`)
    }
    const viewport = record(source.viewport, `${label}.viewport`)
    const camera = record(source.camera, `${label}.camera`)
    return {
      schema: SCENE_FRAME_SCHEMA,
      version: SCENE_FRAME_VERSION,
      viewport: { width: num(viewport.width, `${label}.viewport.width`), height: num(viewport.height, `${label}.viewport.height`) },
      camera: { x: num(camera.x, `${label}.camera.x`), y: num(camera.y, `${label}.camera.y`), zoom: num(camera.zoom, `${label}.camera.zoom`) },
      lod: lod(source.lod, `${label}.lod`),
      waiting: bool(source.waiting, `${label}.waiting`),
      style: style(source.style, `${label}.style`),
      world: list(source.world, `${label}.world`).map((item, index) => worldItem(item, `${label}.world[${index}]`)),
      edges: list(source.edges, `${label}.edges`).map((item, index) => edge(item, `${label}.edges[${index}]`)),
      chrome: list(source.chrome, `${label}.chrome`).map((item, index) => chrome(item, `${label}.chrome[${index}]`)),
      legend: legend(source.legend, `${label}.legend`),
      ...(source.hover === undefined || source.hover === null
        ? {}
        : { hover: hover(source.hover, `${label}.hover`) }),
    }
  } catch (error) {
    if (error instanceof Invalid) throw new Error(error.message)
    throw error
  }
}

/** True when this world item kind is one the documented renderer draws. */
export function isDrawnWorldKind(kind: string): boolean {
  return KNOWN_WORLD_KINDS.has(kind)
}
