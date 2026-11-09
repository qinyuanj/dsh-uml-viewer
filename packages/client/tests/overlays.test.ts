import { describe, expect, it } from 'vitest'
import type { CardModel, InspectorModel, PanelAction, SourceModel } from '@dsh-uml-viewer/contracts'
import { OverlayController } from '../src/overlays.js'

const inspector: InspectorModel = {
  entries: [{ id: null, label: 'Shop', active: true }, { id: ':plan', label: 'Proposal', active: false }],
  summary: { kind: 'class', lines: [{ type: 'segments', parts: ['Crap μ 4.0', 'max 8.0', 'σ 1.5'], color: { role: 'accent' } }] },
  declutterLabel: '精简：无', detail: ':order', selected: '{:kind :class :id :order}',
  labels: { inspector: '检查器', proposals: '提案', pHint: 'P：返回命名空间树' },
}
const card: CardModel = { id: ':order', title: 'order', items: [], contentHeight: 1000, viewHeight: 700, maxScroll: 300, scroll: 0 }
const source: SourceModel = { file: 'src/order.rs', title: 'src/order.rs:82', line: 82, lang: 'rust', rows: [
  { number: 82, text: '\tpub fn target()', display: '  pub fn target()', marked: true, tokens: [{ text: 'pub', kind: 'keyword' }] },
] }

function harness() {
  const actions: PanelAction[] = []
  let model = inspector
  const controller = new OverlayController(async action => {
    actions.push(action)
    if (action.action === 'inspector') return { accepted: true, reason: null, inspector: model }
    if (action.action === 'card') return { accepted: true, reason: null, card }
    if (action.action === 'open-source') {
      if (action.member === 'absent') return { accepted: false, reason: 'Source not found' }
      const file = action.id === ':second' ? 'src/second.rs' : source.file
      return { accepted: true, reason: null, source: { ...source, file } }
    }
    if (action.action === 'close-card') model = { ...inspector, detail: null }
    return { accepted: true, reason: null }
  })
  return { controller, actions }
}

describe('panel overlays', () => {
  it('shows the supplied summaries and card without deriving quality text or scroll', async () => {
    const { controller, actions } = harness()
    await controller.refresh()
    expect(controller.current.inspector).toBe(inspector)
    expect(controller.current.card).toBe(card)
    expect(actions).toEqual([{ action: 'inspector' }, { action: 'card', width: 624, viewHeight: 700 }])
    controller.setCardGeometry({ width: 900, viewHeight: 500 })
    await controller.scrollCard(4)
    expect(actions.slice(-2)).toEqual([
      { action: 'scroll-card', delta: 4, viewHeight: 500 }, { action: 'card', width: 900, viewHeight: 500 },
    ])
    expect(controller.current.card?.scroll).toBe(0) // producer response, no local 192px guess
  })

  it('double clicks, closes, chooses proposals and returns via P through the resident', async () => {
    const { controller, actions } = harness()
    await controller.openCardAt([12, 34])
    expect(actions[0]).toEqual({ action: 'open-card', at: [12, 34] })
    await controller.choose(':plan')
    await controller.namespaceTree()
    await controller.locate(':related')
    await controller.closeCard()
    expect(controller.current.card).toBeNull()
    expect(actions).toContainEqual({ action: 'context', proposalId: ':plan' })
    expect(actions).toContainEqual({ action: 'namespace-tree' })
    expect(actions).toContainEqual({ action: 'locate', id: ':related' })
  })

  it('reuses a source file window, keeps other files and closes only the chosen one', async () => {
    const { controller, actions } = harness()
    await controller.openSource(':order', 'target')
    const first = controller.current.sources[0]!.serial
    expect(actions[0]).toEqual({ action: 'open-source', id: ':order', member: 'target' })
    expect(controller.current.sources[0]?.model).toEqual(source)
    await controller.openSource(':second')
    expect(controller.current.sources).toHaveLength(2)
    await controller.openSource(':order')
    expect(controller.current.sources).toHaveLength(2)
    expect(controller.current.sources[1]?.serial).toBe(first)
    controller.closeSource(first)
    expect(controller.current.sources.map(window => window.model.file)).toEqual(['src/second.rs'])
    controller.closeNewestSource()
    expect(controller.current.sources).toEqual([])
  })

  it('retains existing windows and reports a failed source request', async () => {
    const { controller } = harness()
    await controller.openSource(':order')
    await controller.openSource(':order', 'absent')
    expect(controller.current.failure).toBe('Source not found')
    expect(controller.current.sources).toHaveLength(1)
    controller.dispose()
    await controller.openSource(':second')
    expect(controller.current.sources).toHaveLength(1)
  })
})
