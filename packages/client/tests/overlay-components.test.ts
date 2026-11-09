import { buildSync } from 'esbuild'
import { fileURLToPath } from 'node:url'
import vm from 'node:vm'
import { describe, expect, it, vi } from 'vitest'
import { DARK_HUES, resolveTheme } from '../src/theme.js'
import { fakeCanvas, DARK_TOKENS } from './fakes.js'

/** Host React element seam: exercise the exported components with the host's
 * createElement boundary. Actual layout/scroll/selection runs in Playwright. */
interface Element { type: string; props: Record<string, any>; children: any[] }
function elements() {
  const code = buildSync({ entryPoints: [fileURLToPath(new URL('../src/overlay-view.ts', import.meta.url))],
    bundle: true, write: false, format: 'cjs', platform: 'node', external: ['react'] }).outputFiles[0]!.text
  const module = { exports: {} as Record<string, (props: any) => Element> }
  const hooks = {
    createElement(type: any, props: any, ...children: any[]): Element {
      return typeof type === 'function' ? type(props) : { type, props: props ?? {}, children: children.flat(Infinity).filter(v => v != null) }
    },
    useRef: () => ({ current: null }), useState: (value: unknown) => [value, () => {}], useEffect: () => {},
  }
  vm.runInNewContext(code, { module, exports: module.exports, require: () => hooks })
  return module.exports
}
function all(tree: Element, predicate: (e: Element) => boolean): Element[] {
  return [tree, ...tree.children.filter(v => typeof v === 'object').flatMap(v => all(v, predicate))].filter(predicate)
}
function text(tree: Element): string {
  return tree.children.map(v => typeof v === 'object' ? text(v) : String(v)).join('')
}
const theme = () => resolveTheme(fakeCanvas({ tokens: DARK_TOKENS, dark: true }))
const labels = { inspector: '检查器', proposals: '提案', pHint: 'P：返回命名空间树' }

it('renders supplied inspector summary lines and proposal choices with host tokens', () => {
  const choose = vi.fn(), namespaceTree = vi.fn()
  const tree = elements().Inspector!({ theme: theme(), controller: { choose, namespaceTree }, state: { inspector: {
    labels, declutterLabel: '精简：无',
    entries: [{ id: null, label: 'Shop', active: false }, { id: ':plan', label: 'Plan', active: true }],
    summary: { kind: 'class', lines: [
      { type: 'segments', parts: ['Crap μ 4.0', 'max 8.0'], color: { role: 'accent' } },
      { type: 'label', text: '无数据', color: { role: 'no-data' } },
    ] },
  } } })
  expect(tree.props['aria-label']).toBe('检查器')
  expect(text(tree)).toContain('Crap μ 4.0max 8.0无数据')
  const plan = all(tree, e => e.type === 'button' && text(e) === 'Plan')[0]!
  expect(plan.props['aria-pressed']).toBe(true)
  plan.props.onClick()
  expect(choose).toHaveBeenCalledWith(':plan')
  all(tree, e => e.type === 'button' && text(e) === labels.pHint)[0]!.props.onClick()
  expect(namespaceTree).toHaveBeenCalled()
  // 无数据 is the fixed no-data red from the shared resolver, not a host error token.
  expect(all(tree, e => e.type === 'p')[0]!.props.style.color).toBe(DARK_HUES.noData)
  expect(all(tree, e => e.props.className === 'dsh-uml-summary-segments')[0]!.props.style.color).toBe('#e8c448')
})

it('offers 新建提案 below the proposals; a click asks for a timestamp-named proposal (#29)', () => {
  const newProposal = vi.fn()
  const tree = elements().Inspector!({ theme: theme(), controller: { choose: vi.fn(), namespaceTree: vi.fn(), newProposal }, state: { inspector: {
    labels, declutterLabel: '精简：无',
    entries: [{ id: null, label: 'Shop', active: true }],
    summary: { kind: 'empty', lines: [] },
  } } })
  const button = all(tree, e => e.type === 'button' && text(e) === '新建提案')
  expect(button).toHaveLength(1)
  button[0]!.props.onClick()
  expect(newProposal).toHaveBeenCalledTimes(1)
  // The name is left to the controller, which reads the local clock.
  expect(Object.prototype.toString.call(newProposal.mock.calls[0]![0])).toBe('[object Date]')
})

it('renders producer rows and sends member/module/relationship activations through the controller', () => {
  const openSource = vi.fn(), locate = vi.fn(), closeCard = vi.fn()
  const item = { kind: 'stats', y: 134, h: 18, text: '+ target()', color: { role: 'ink' }, titleFont: false,
    x: 30, clipRight: 242, cells: [{ id: 'crap', text: '4.0', left: 252, right: 308, center: false, color: { role: 'accent' }, tip: 'risk' }],
    bars: [], stripe: true, swatch: null, clickable: true, module: false, opName: 'target', rel: null }
  const tree = elements().ClassCard!({ theme: theme(), controller: { openSource, locate, closeCard }, state: { card: {
    id: ':order', title: 'Order', scroll: 0, contentHeight: 400, items: [item,
      { ...item, module: true, opName: null, text: 'order.rs' },
      { ...item, kind: 'rel', opName: null, rel: ':other', text: '依赖 other' }],
  } } })
  expect(tree.props['aria-label']).toBe('类卡片 Order')
  const member = all(tree, e => e.props['data-member'] === 'target')[0]!
  member.props.onClick()
  expect(openSource).toHaveBeenCalledWith(':order', 'target')
  all(tree, e => 'data-card-module' in e.props && e.props['data-card-module'] !== undefined)[0]!.props.onClick()
  expect(openSource).toHaveBeenCalledWith(':order', undefined)
  all(tree, e => e.props['data-rel'] === ':other')[0]!.props.onClick()
  expect(locate).toHaveBeenCalledWith(':other')
  const cell = all(tree, e => e.props.title === 'risk')[0]!
  expect(cell.props.style).toMatchObject({ left: 252, width: 56, textAlign: 'right' })
  expect(text(cell)).toBe('4.0')
})

it('renders source tokens, declaration marker and unselectable gutter without reading files', () => {
  const closeSource = vi.fn()
  const tree = elements().SourceView!({ index: 0, controller: { closeSource }, window: { serial: 7, model: {
    title: 'src/order.rs:82', line: 82, rows: [{ number: 82, marked: true,
      tokens: [{ kind: 'ink', text: '  ' }, { kind: 'keyword', text: 'pub' }, { kind: 'ink', text: ' fn target()' }] }],
  } } })
  expect(tree.props['aria-label']).toBe('源码窗口 src/order.rs:82')
  expect(all(tree, e => e.props['data-marked'] === true)[0]!.props['data-source-line']).toBe(82)
  expect(text(all(tree, e => e.type === 'code')[0]!)).toBe('  pub fn target()')
  expect(all(tree, e => e.props.className === 'dsh-uml-source-gutter')[0]!.props['aria-hidden']).toBe(true)
  all(tree, e => e.props['aria-label'] === '关闭源码窗口')[0]!.props.onClick()
  expect(closeSource).toHaveBeenCalledWith(7)
})

it('colours card cells, bars and swatches through the shared role resolver with producer grade/ratio', () => {
  const resolved = theme()
  const base = { kind: 'stats', y: 0, h: 18, text: 'x', color: { role: 'ink' }, titleFont: false, x: 30, clipRight: null,
    bars: [], stripe: false, swatch: null, clickable: false, module: false, opName: null, rel: null }
  const cells = [
    { id: 'cov-hi', text: '90%', left: 0, right: 40, center: false, color: { role: 'coverage', ratio: 0.9 }, tip: null },
    { id: 'cov-mid', text: '60%', left: 40, right: 80, center: false, color: { role: 'coverage', ratio: 0.6 }, tip: null },
    { id: 'cov-lo', text: '20%', left: 80, right: 120, center: false, color: { role: 'coverage', ratio: 0.2 }, tip: null },
    { id: 'q6', text: '6', left: 120, right: 160, center: false, color: { role: 'quality-stroke', grade: 6 }, tip: null },
  ]
  const tree = elements().ClassCard!({ theme: resolved, controller: {}, state: { card: {
    id: ':x', title: 'X', scroll: 0, contentHeight: 40, items: [{ ...base, cells,
      swatch: { role: 'quality-fill', grade: 7.5 },
      bars: [{ id: 'b', left: 0, right: 100, fraction: 0.5, color: { role: 'violation' } }] }],
  } } })
  const colour = (label: string) => all(tree, e => e.props.className === 'dsh-uml-card-cell' && text(e) === label)[0]!.props.style.color
  expect(colour('90%')).toBe(resolved.frame('coverage', null, 0.9))
  expect(colour('60%')).toBe(resolved.frame('coverage', null, 0.6))
  expect(colour('20%')).toBe(resolved.frame('coverage', null, 0.2))
  expect(new Set([colour('90%'), colour('60%'), colour('20%')]).size).toBe(3)
  expect(colour('6')).toBe(resolved.frame('quality-stroke', 6))
  expect(colour('6')).not.toMatch(/var\(/)
  const swatch = all(tree, e => e.props.style?.width === 10)[0]!
  expect(swatch.props.style.background).toBe(resolved.frame('quality-fill', 7.5))
  const bar = all(tree, e => e.props.style?.width === '50%')[0]!
  expect(bar.props.style.background).toBe(DARK_HUES.violation)
})

describe('bounded source DOM', () => {
  const big = (count: number, line: number | null) => ({ title: 'big.ts', line, rows: Array.from({ length: count }, (_, i) => ({
    number: i + 1, marked: line === i + 1, tokens: [{ kind: 'ink', text: `line ${i + 1}` }],
  })) })

  it('renders only a bounded row window of a long file, around the declaration line', () => {
    const view = elements() as unknown as Record<string, any>
    const tree = view.SourceView({ index: 0, controller: {}, window: { serial: 1, model: big(4000, 3000) } })
    const rows = all(tree, e => e.props['data-source-line'] !== undefined)
    expect(rows.length).toBeGreaterThan(0)
    expect(rows.length).toBeLessThanOrEqual(view.SOURCE_MAX_RENDERED_ROWS)
    expect(rows.some(e => e.props['data-source-line'] === 3000 && e.props['data-marked'] === true)).toBe(true)
    const scroll = all(tree, e => e.props.className === 'dsh-uml-source-scroll')[0]!
    expect(scroll.props['data-source-rows']).toBe(4000)
    // Spacers keep the full scroll height: rendered + spacer rows = file rows.
    const spacers = scroll.children.filter((e: any) => e.props['aria-hidden'] === true && e.props.style?.height !== undefined)
    const spacerRows = spacers.reduce((sum: number, e: any) => sum + e.props.style.height, 0) / view.SOURCE_ROW_HEIGHT
    expect(spacerRows + rows.length).toBe(4000)
  })

  it('computes the visible window per scroll offset with fixed bounds', () => {
    const { sourceRowWindow, SOURCE_OVERSCAN_ROWS, SOURCE_MAX_RENDERED_ROWS } = elements() as unknown as Record<string, any>
    expect(sourceRowWindow(84, 0, 600)).toEqual({ start: 0, end: 84 })
    expect(sourceRowWindow(4000, 0, 600)).toEqual({ start: 0, end: 31 + 2 * SOURCE_OVERSCAN_ROWS })
    expect(sourceRowWindow(4000, 2000, 400)).toEqual({ start: 100 - SOURCE_OVERSCAN_ROWS, end: 100 - SOURCE_OVERSCAN_ROWS + 21 + 2 * SOURCE_OVERSCAN_ROWS })
    expect(sourceRowWindow(4000, 79990, 600).end).toBe(4000)
    const huge = sourceRowWindow(4000, 0, 1e6)
    expect(huge.end - huge.start).toBe(SOURCE_MAX_RENDERED_ROWS)
    expect(sourceRowWindow(10, Number.NaN, Number.NaN)).toEqual({ start: 0, end: 10 })
  })
})
