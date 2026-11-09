import { readFileSync } from 'node:fs'
import { describe, expect, it } from 'vitest'
import {
  parsePanelAction, parseResidentLine, RESIDENT_LINE_MAX_BYTES, SOURCE_LIMITS, validateCardModel,
  utf8Bytes, validateInspectorModel, validateSourceModel,
} from '../src/index.js'

/**
 * The overlay contract (#27): the validators accept exactly what
 * `crates/viewer-session/src/overlays.rs` writes, and refuse malformed lines
 * instead of letting the panel render a half-parsed model.
 */

const inspector = {
  entries: [
    { id: null, label: 'Shop', active: true },
    { id: ':p-shop', label: '2026-10-24 10:00:00', active: false },
  ],
  summary: {
    kind: 'class',
    lines: [
      { type: 'label', text: 'order', color: { role: 'ink' } },
      { type: 'label', text: '包 core', color: { role: 'muted' } },
      { type: 'segments', parts: ['Crap μ 4.0', 'max 8.0', 'σ 1.5'], color: { role: 'accent' } },
      { type: 'segments', parts: ['7 killed / 2 survived', '3 uncovered'], color: { role: 'muted' } },
      { type: 'label', text: '无数据', color: { role: 'no-data' } },
    ],
  },
  declutterLabel: '精简：无',
  detail: ':core.order',
  selected: '{:kind :class :id :core.order}',
  labels: { inspector: '检查器', proposals: '提案', pHint: 'P：返回命名空间树' },
}

const card = {
  id: ':core.order',
  title: 'order',
  contentHeight: 342,
  viewHeight: 120,
  scroll: 192,
  maxScroll: 222,
  items: [
    {
      kind: 'name', y: 0, h: 20, text: 'order', color: { role: 'ink' }, titleFont: true, x: 16,
      clipRight: null, cells: [], bars: [], stripe: false, swatch: null, clickable: false,
      module: false, opName: null, rel: null,
    },
    {
      kind: 'stats', y: 116, h: 18, text: 'order', color: { role: 'ink' }, titleFont: false, x: 30,
      clipRight: 242,
      cells: [{ id: 'crap', text: '4.0μ', left: 252, right: 308, center: false, color: { role: 'quality-stroke', grade: 10 }, tip: null }],
      bars: [{ id: 'kill', left: 414, right: 624, fraction: 0.5833333333333334, color: { role: 'quality-stroke', grade: 1 } }],
      stripe: true, swatch: { role: 'quality-stroke', grade: 5.5 }, clickable: true,
      module: false, opName: 'place', rel: null,
    },
    {
      kind: 'col-header', y: 98, h: 18, text: '成员', color: { role: 'accent' }, titleFont: false, x: 16,
      clipRight: 242,
      cells: [{ id: 'cov', text: 'Cov', left: 360, right: 404, center: false, color: { role: 'accent' }, tip: '行覆盖率' }],
      bars: [], stripe: false, swatch: null, clickable: false, module: false, opName: null, rel: null,
    },
    {
      kind: 'rel', y: 200, h: 18, text: '依赖  core', color: { role: 'ink' }, titleFont: false, x: 16,
      clipRight: null, cells: [], bars: [], stripe: false, swatch: null, clickable: true,
      module: false, opName: null, rel: ':core',
    },
  ],
}

const source = {
  file: 'D:/project/src/order.rs',
  title: 'src/order.rs:1',
  line: 1,
  lang: 'rust',
  rows: [
    {
      number: 1, text: '\tpub fn place(&mut self) -> u64 {', display: '  pub fn place(&mut self) -> u64 {',
      marked: true,
      tokens: [
        { text: '  ', kind: 'ink' }, { text: 'pub', kind: 'keyword' }, { text: ' ', kind: 'ink' },
        { text: 'fn', kind: 'keyword' }, { text: ' place(&mut self) -> u64 {', kind: 'ink' },
      ],
    },
    {
      number: 2, text: '\t\tself.id', display: '    self.id', marked: false,
      tokens: [{ text: '    ', kind: 'ink' }, { text: 'self', kind: 'keyword' }, { text: '.', kind: 'ink' }, { text: 'id', kind: 'ink' }],
    },
  ],
}

describe('overlay validators', () => {
  it('validates one exact line of each new event captured from the real resident binary', () => {
    const captured = ['inspector', 'card', 'source'].map(name => parseResidentLine(
      readFileSync(new URL(`./protocol-samples/${name}.json`, import.meta.url), 'utf8').trimEnd(),
    ))
    expect(captured.map(event => event.event)).toEqual(['inspector', 'card', 'source'])
    const [inspect, detail, opened] = captured
    if (inspect?.event !== 'inspector' || detail?.event !== 'card' || opened?.event !== 'source') {
      throw new Error('producer samples must contain the three new resident event variants')
    }
    expect(inspect.inspector.entries.map(entry => entry.id)).toEqual([null, ':p-inspector'])
    expect(detail.card.id).toBe(':catalog')
    expect(detail.card.title).toBe('Catalog')
    expect(opened.source.line).toBe(82)
    expect(opened.source.rows).toHaveLength(84)
    expect(opened.source.rows.filter(row => row.marked).map(row => row.number)).toEqual([82])
  })
  it('accepts the inspector model with its summary lines and semantic roles', () => {
    const parsed = validateInspectorModel(inspector)
    expect(parsed).toEqual(inspector)
    expect(parsed.summary.lines.map(line => line.type)).toEqual(['label', 'label', 'segments', 'segments', 'label'])
    expect(parsed.entries[0]).toEqual({ id: null, label: 'Shop', active: true })
  })

  it('accepts the card paint plan, keeping grades, fractions and null rels', () => {
    expect(validateCardModel(card)).toEqual(card)
  })

  it('accepts an opened source with marked rows and token runs', () => {
    expect(validateSourceModel(source)).toEqual(source)
    const whole = { ...source, line: null, rows: source.rows.map(row => ({ ...row, marked: false })) }
    expect(validateSourceModel(whole).line).toBeNull()
  })

  it('shares the documented source bounds with the resident', () => {
    expect(SOURCE_LIMITS).toEqual({
      maxFileBytes: 262_144, maxRows: 4000, maxLineBytes: 4096, maxDisplayBytes: 8192,
      maxRunsPerRow: 512, maxRuns: 20_000, maxEventBytes: 2_097_152, maxPathBytes: 4096, maxLangBytes: 64,
    })
    expect(RESIDENT_LINE_MAX_BYTES).toBe(16_777_216)
    expect(Object.isFrozen(SOURCE_LIMITS)).toBe(true)
  })

  it('counts UTF-8 bytes exactly as TextEncoder does, without needing it at load', () => {
    for (const text of ['', 'ascii', 'é', '中文', '😀x', 'a\uD800b', '\uDC00', 'end\uD83D', '\u07ff\u0800\uffff']) {
      expect(utf8Bytes(text), JSON.stringify(text)).toBe(new TextEncoder().encode(text).length)
    }
  })

  it('refuses source rows and target lines that are not real row positions', () => {
    for (const line of [0, -1, 1.5, 3]) {
      expect(() => validateSourceModel({ ...source, line })).toThrow(/source\.line/)
    }
    const renumbered = { ...source, rows: [source.rows[0], { ...source.rows[1], number: 3 }] }
    expect(() => validateSourceModel(renumbered)).toThrow(/rows\[1\]\.number: must be 2/)
    expect(() => validateSourceModel({ ...source, rows: [{ ...source.rows[0], number: 0.5 }] })).toThrow(/rows\[0\]\.number/)
    const twoMarks = { ...source, rows: source.rows.map(row => ({ ...row, marked: true })) }
    expect(() => validateSourceModel(twoMarks)).toThrow(/rows\[1\]\.marked/)
    expect(() => validateSourceModel({ ...source, rows: [{ ...source.rows[0], tokens: [] }, source.rows[1]] }))
      .toThrow(/rows\[0\]\.tokens: must join/)
    expect(() => validateSourceModel({
      ...source, rows: [{ ...source.rows[0], tokens: [{ text: source.rows[0]!.display, kind: 'bold' }] }, source.rows[1]],
    })).toThrow(/kind/)
  })

  it('refuses a source over each row, line, token and metadata bound', () => {
    const row = (number: number, text = 'x') => ({ number, text, display: text, marked: false, tokens: text ? [{ text, kind: 'ink' }] : [] })
    const sized = (count: number) => ({ ...source, line: null, rows: Array.from({ length: count }, (_, i) => row(i + 1, '')) })
    expect(validateSourceModel(sized(SOURCE_LIMITS.maxRows)).rows).toHaveLength(SOURCE_LIMITS.maxRows)
    expect(() => validateSourceModel(sized(SOURCE_LIMITS.maxRows + 1))).toThrow(/rows: has 4001 items; the limit is 4000/)
    const long = 'é'.repeat(SOURCE_LIMITS.maxLineBytes / 2)
    expect(validateSourceModel({ ...source, line: null, rows: [row(1, long)] }).rows[0]!.text).toBe(long)
    expect(() => validateSourceModel({ ...source, line: null, rows: [{ ...row(1, long), text: `${long}x` }] }))
      .toThrow(/rows\[0\]\.text: exceeds 4096 UTF-8 bytes/)
    const wide = ' '.repeat(SOURCE_LIMITS.maxDisplayBytes + 1)
    expect(() => validateSourceModel({ ...source, line: null, rows: [{ ...row(1), display: wide, tokens: [{ text: wide, kind: 'ink' }] }] }))
      .toThrow(/rows\[0\]\.display/)
    const punct = ';'.repeat(SOURCE_LIMITS.maxRunsPerRow + 1)
    const amplified = { ...row(1, punct), tokens: [...punct].map(text => ({ text, kind: 'ink' })) }
    expect(() => validateSourceModel({ ...source, line: null, rows: [amplified] })).toThrow(/tokens: has 513 items/)
    const busy = ';'.repeat(SOURCE_LIMITS.maxRunsPerRow)
    const busyRows = Array.from({ length: 40 }, (_, i) => ({ ...row(i + 1, busy), tokens: [...busy].map(text => ({ text, kind: 'ink' })) }))
    expect(() => validateSourceModel({ ...source, line: null, rows: busyRows })).toThrow(/exceed 20000 token runs/)
    expect(() => validateSourceModel({ ...source, file: 'f'.repeat(4097) })).toThrow(/source\.file/)
    expect(() => validateSourceModel({ ...source, title: 't'.repeat(4097) })).toThrow(/source\.title/)
    expect(() => validateSourceModel({ ...source, lang: 'l'.repeat(65) })).toThrow(/source\.lang/)
  })

  it('refuses malformed payloads instead of guessing', () => {
    expect(() => validateInspectorModel({ ...inspector, entries: [{ label: 'x', active: 'yes' }] })).toThrow(/entries\[0\]/)
    expect(() => validateInspectorModel({ ...inspector, summary: { kind: 'unknown', lines: [] } })).toThrow(/summary.kind/)
    expect(() => validateInspectorModel({
      ...inspector,
      summary: { kind: 'empty', lines: [{ type: 'label', text: 3, color: { role: 'muted' } }] },
    })).toThrow(/summary.lines\[0\].text/)
    expect(() => validateCardModel({ ...card, items: [{ ...card.items[0], y: 'top' }] })).toThrow(/items\[0\].y/)
    expect(() => validateCardModel({ ...card, maxScroll: Number.NaN })).toThrow(/maxScroll/)
    expect(() => validateSourceModel({ ...source, rows: [{ ...source.rows[0], marked: 1 }] })).toThrow(/rows\[0\].marked/)
  })

  it('carries the three new resident events at version 1', () => {
    for (const [event, key, payload] of [
      ['inspector', 'inspector', inspector],
      ['card', 'card', card],
      ['source', 'source', source],
    ] as const) {
      const parsed = parseResidentLine(JSON.stringify({ version: 1, seq: 7, event, [key]: payload }))
      expect(parsed.event).toBe(event)
      expect(parsed.seq).toBe(7)
      expect((parsed as unknown as Record<string, unknown>)[key]).toEqual(payload)
    }
    expect(() => parseResidentLine(JSON.stringify({ version: 1, seq: 7, event: 'card', card: { id: 1 } })))
      .toThrow(/card/)
  })
})

describe('overlay panel actions', () => {
  it('parses each action the overlays send', () => {
    expect(parsePanelAction({ action: 'inspector' })).toEqual({ action: 'inspector' })
    expect(parsePanelAction({ action: 'close-card' })).toEqual({ action: 'close-card' })
    expect(parsePanelAction({ action: 'card', viewHeight: 120 })).toEqual({ action: 'card', viewHeight: 120 })
    expect(parsePanelAction({ action: 'card' })).toEqual({ action: 'card' })
    expect(parsePanelAction({ action: 'open-card', at: [10, 20] })).toEqual({ action: 'open-card', at: [10, 20] })
    expect(parsePanelAction({ action: 'open-card', id: ':core.order' })).toEqual({ action: 'open-card', id: ':core.order' })
    expect(parsePanelAction({ action: 'scroll-card', delta: 4, viewHeight: 120 }))
      .toEqual({ action: 'scroll-card', delta: 4, viewHeight: 120 })
    expect(parsePanelAction({ action: 'open-source', id: ':core.order', member: 'place' }))
      .toEqual({ action: 'open-source', id: ':core.order', member: 'place' })
    expect(parsePanelAction({ action: 'open-source', id: ':core.order' })).toEqual({ action: 'open-source', id: ':core.order' })
  })

  it('refuses an incomplete or malformed overlay action', () => {
    expect(() => parsePanelAction({ action: 'open-card' })).toThrow(/open-card needs an id or a point/)
    expect(() => parsePanelAction({ action: 'open-source', member: 'place' })).toThrow(/id/)
    expect(() => parsePanelAction({ action: 'scroll-card', delta: 'four', viewHeight: 120 })).toThrow(/delta/)
    expect(() => parsePanelAction({ action: 'card', width: 'wide' })).toThrow(/width/)
  })
})
