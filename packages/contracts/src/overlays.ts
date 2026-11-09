/**
 * The panel overlays (#27): inspector, class card and source window payloads.
 *
 * These are *views of the Rust models*, not data the client interprets:
 * `frame::inspector` owns summary text, counts, CRAP/mutation formatting and
 * no-data semantics, `frame::detail` owns the card's rows, metric columns,
 * bars, swatches and scroll clamps, and `frame::source` owns declaration
 * lines, tab expansion and token runs. The validators below check the shape
 * the resident writes (`crates/viewer-session/src/overlays.rs`); they never
 * derive a figure.
 */
import type { FrameColor } from './scene-frame.js'

/** One inspector summary line, in display order. */
export type SummaryLine =
  | { type: 'label'; text: string; color: FrameColor }
  | { type: 'segments'; parts: string[]; color: FrameColor }

export interface InspectorEntry { id: string | null; label: string; active: boolean }

export interface InspectorSummary {
  kind: 'class' | 'package' | 'empty'
  lines: SummaryLine[]
}

export interface InspectorModel {
  entries: InspectorEntry[]
  summary: InspectorSummary
  declutterLabel: string
  /** The class whose card is open (EDN), or null. */
  detail: string | null
  /** The selected hit (EDN), or null. */
  selected: string | null
  labels: { inspector: string; proposals: string; pHint: string }
}

export interface CardCell {
  id: string
  text: string
  left: number
  right: number
  center: boolean
  color: FrameColor
  tip: string | null
}

export interface CardBar { id: string; left: number; right: number; fraction: number; color: FrameColor }

/** One card row of the native paint plan, in content points. */
export interface CardItem {
  kind: string
  y: number
  h: number
  text: string | null
  color: FrameColor
  titleFont: boolean
  x: number
  clipRight: number | null
  cells: CardCell[]
  bars: CardBar[]
  stripe: boolean
  swatch: FrameColor | null
  clickable: boolean
  module: boolean
  opName: string | null
  rel: string | null
}

export interface CardModel {
  id: string
  title: string
  contentHeight: number
  viewHeight: number
  scroll: number
  maxScroll: number
  items: CardItem[]
}

export interface SourceToken { text: string; kind: string }

export interface SourceRow {
  number: number
  /** The source line (CR trimmed). */
  text: string
  /** The painted line: tabs expanded to two spaces. */
  display: string
  /** True on the declaration line the window jumped to. */
  marked: boolean
  tokens: SourceToken[]
}

export interface SourceModel {
  file: string
  title: string
  /** 1-based declaration line, or null for a whole module. */
  line: number | null
  lang: string
  rows: SourceRow[]
}

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

function optionalString(value: unknown, label: string): string | null {
  return value === null || value === undefined ? null : str(value, label)
}

function optionalNumber(value: unknown, label: string): number | null {
  return value === null || value === undefined ? null : num(value, label)
}

function color(value: unknown, label: string): FrameColor {
  const source = record(value, label)
  const parsed: FrameColor = { role: str(source.role, `${label}.role`) }
  if (source.grade !== undefined) parsed.grade = source.grade === null ? null : num(source.grade, `${label}.grade`)
  if (source.ratio !== undefined) parsed.ratio = source.ratio === null ? null : num(source.ratio, `${label}.ratio`)
  return parsed
}

function summaryLine(value: unknown, label: string): SummaryLine {
  const source = record(value, label)
  const type = str(source.type, `${label}.type`)
  if (type === 'label') {
    return { type, text: str(source.text, `${label}.text`), color: color(source.color, `${label}.color`) }
  }
  if (type === 'segments') {
    return {
      type,
      parts: list(source.parts, `${label}.parts`).map((part, index) => str(part, `${label}.parts[${index}]`)),
      color: color(source.color, `${label}.color`),
    }
  }
  return at(`${label}.type`, `unknown summary line ${JSON.stringify(type)}`)
}

/** Validate the inspector payload of an `inspector` answer. */
export function validateInspectorModel(value: unknown, label = 'inspector'): InspectorModel {
  try {
    const source = record(value, label)
    const kind = str(record(source.summary, `${label}.summary`).kind, `${label}.summary.kind`)
    if (kind !== 'class' && kind !== 'package' && kind !== 'empty') {
      at(`${label}.summary.kind`, `unknown summary kind ${JSON.stringify(kind)}`)
    }
    const summary = record(source.summary, `${label}.summary`)
    const labels = record(source.labels, `${label}.labels`)
    return {
      entries: list(source.entries, `${label}.entries`).map((entry, index) => {
        const row = record(entry, `${label}.entries[${index}]`)
        return {
          id: optionalString(row.id, `${label}.entries[${index}].id`),
          label: str(row.label, `${label}.entries[${index}].label`),
          active: bool(row.active, `${label}.entries[${index}].active`),
        }
      }),
      summary: {
        kind,
        lines: list(summary.lines, `${label}.summary.lines`).map((line, index) => summaryLine(line, `${label}.summary.lines[${index}]`)),
      },
      declutterLabel: str(source.declutterLabel, `${label}.declutterLabel`),
      detail: optionalString(source.detail, `${label}.detail`),
      selected: optionalString(source.selected, `${label}.selected`),
      labels: {
        inspector: str(labels.inspector, `${label}.labels.inspector`),
        proposals: str(labels.proposals, `${label}.labels.proposals`),
        pHint: str(labels.pHint, `${label}.labels.pHint`),
      },
    }
  } catch (error) {
    if (error instanceof Invalid) throw new Error(error.message)
    throw error
  }
}

function cardCell(value: unknown, label: string): CardCell {
  const source = record(value, label)
  return {
    id: str(source.id, `${label}.id`),
    text: str(source.text, `${label}.text`),
    left: num(source.left, `${label}.left`),
    right: num(source.right, `${label}.right`),
    center: bool(source.center, `${label}.center`),
    color: color(source.color, `${label}.color`),
    tip: optionalString(source.tip, `${label}.tip`),
  }
}

function cardBar(value: unknown, label: string): CardBar {
  const source = record(value, label)
  return {
    id: str(source.id, `${label}.id`),
    left: num(source.left, `${label}.left`),
    right: num(source.right, `${label}.right`),
    fraction: num(source.fraction, `${label}.fraction`),
    color: color(source.color, `${label}.color`),
  }
}

function cardItem(value: unknown, label: string): CardItem {
  const source = record(value, label)
  return {
    kind: str(source.kind, `${label}.kind`),
    y: num(source.y, `${label}.y`),
    h: num(source.h, `${label}.h`),
    text: optionalString(source.text, `${label}.text`),
    color: color(source.color, `${label}.color`),
    titleFont: bool(source.titleFont, `${label}.titleFont`),
    x: num(source.x, `${label}.x`),
    clipRight: optionalNumber(source.clipRight, `${label}.clipRight`),
    cells: list(source.cells, `${label}.cells`).map((cell, index) => cardCell(cell, `${label}.cells[${index}]`)),
    bars: list(source.bars, `${label}.bars`).map((bar, index) => cardBar(bar, `${label}.bars[${index}]`)),
    stripe: bool(source.stripe, `${label}.stripe`),
    swatch: source.swatch === null || source.swatch === undefined ? null : color(source.swatch, `${label}.swatch`),
    clickable: bool(source.clickable, `${label}.clickable`),
    module: bool(source.module, `${label}.module`),
    opName: optionalString(source.opName, `${label}.opName`),
    rel: optionalString(source.rel, `${label}.rel`),
  }
}

/** Validate the class card paint plan of a `card` answer. */
export function validateCardModel(value: unknown, label = 'card'): CardModel {
  try {
    const source = record(value, label)
    return {
      id: str(source.id, `${label}.id`),
      title: str(source.title, `${label}.title`),
      contentHeight: num(source.contentHeight, `${label}.contentHeight`),
      viewHeight: num(source.viewHeight, `${label}.viewHeight`),
      scroll: num(source.scroll, `${label}.scroll`),
      maxScroll: num(source.maxScroll, `${label}.maxScroll`),
      items: list(source.items, `${label}.items`).map((item, index) => cardItem(item, `${label}.items[${index}]`)),
    }
  } catch (error) {
    if (error instanceof Invalid) throw new Error(error.message)
    throw error
  }
}

/** Validate the opened source of an `open-source` answer. */
export function validateSourceModel(value: unknown, label = 'source'): SourceModel {
  try {
    const source = record(value, label)
    return {
      file: str(source.file, `${label}.file`),
      title: str(source.title, `${label}.title`),
      line: optionalNumber(source.line, `${label}.line`),
      lang: str(source.lang, `${label}.lang`),
      rows: list(source.rows, `${label}.rows`).map((row, index) => {
        const at = `${label}.rows[${index}]`
        const item = record(row, at)
        return {
          number: num(item.number, `${at}.number`),
          text: str(item.text, `${at}.text`),
          display: str(item.display, `${at}.display`),
          marked: bool(item.marked, `${at}.marked`),
          tokens: list(item.tokens, `${at}.tokens`).map((token, tokenIndex) => {
            const run = record(token, `${at}.tokens[${tokenIndex}]`)
            return {
              text: str(run.text, `${at}.tokens[${tokenIndex}].text`),
              kind: str(run.kind, `${at}.tokens[${tokenIndex}].kind`),
            }
          }),
        }
      }),
    }
  } catch (error) {
    if (error instanceof Invalid) throw new Error(error.message)
    throw error
  }
}
