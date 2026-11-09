import { buildSync } from 'esbuild'
import { fileURLToPath } from 'node:url'
import vm from 'node:vm'
import { describe, expect, it, vi } from 'vitest'

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
const labels = { inspector: '检查器', proposals: '提案', pHint: 'P：返回命名空间树' }

it('renders supplied inspector summary lines and proposal choices with host tokens', () => {
  const choose = vi.fn(), namespaceTree = vi.fn()
  const tree = elements().Inspector!({ controller: { choose, namespaceTree }, state: { inspector: {
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
  expect(all(tree, e => e.type === 'p')[0]!.props.style.color).toBe('var(--dsw-alias-state-error-primary)')
})

it('renders producer rows and sends member/module/relationship activations through the controller', () => {
  const openSource = vi.fn(), locate = vi.fn(), closeCard = vi.fn()
  const item = { kind: 'stats', y: 134, h: 18, text: '+ target()', color: { role: 'ink' }, titleFont: false,
    x: 30, clipRight: 242, cells: [{ id: 'crap', text: '4.0', left: 252, right: 308, center: false, color: { role: 'accent' }, tip: 'risk' }],
    bars: [], stripe: true, swatch: null, clickable: true, module: false, opName: 'target', rel: null }
  const tree = elements().ClassCard!({ controller: { openSource, locate, closeCard }, state: { card: {
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
