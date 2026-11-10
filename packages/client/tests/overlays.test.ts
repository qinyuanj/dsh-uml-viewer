import { describe, expect, it } from 'vitest'
import type { CardModel, InspectorModel, PanelAck, PanelAction, SourceModel } from '@dsh-uml-viewer/contracts'
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

/** Every send stays pending until the test settles it, in any order. */
function deferred() {
  const pending: { action: PanelAction; resolve: (ack: PanelAck | null) => void; reject: (error: Error) => void }[] = []
  const controller = new OverlayController(action => new Promise((resolve, reject) => {
    pending.push({ action, resolve, reject })
  }))
  const flush = async () => { for (let i = 0; i < 10; i += 1) await Promise.resolve() }
  /** Take the oldest pending request for `name` (and optional id). */
  const take = (name: PanelAction['action'], id?: string) => {
    const index = pending.findIndex(entry => entry.action.action === name &&
      (id === undefined || ('id' in entry.action && entry.action.id === id)))
    if (index < 0) throw new Error(`no pending ${name} ${id ?? ''}: ${pending.map(entry => entry.action.action).join(',')}`)
    return pending.splice(index, 1)[0]!
  }
  const reply = async (name: PanelAction['action'], ack: PanelAck | null, id?: string) => {
    take(name, id).resolve(ack)
    await flush()
  }
  return { controller, pending, take, reply, flush }
}

const ok = (extra: Partial<PanelAck> = {}): PanelAck => ({ accepted: true, reason: null, ...extra })
const closedInspector: InspectorModel = { ...inspector, detail: null }
const other: SourceModel = { ...source, file: 'src/other.rs', title: 'src/other.rs:1' }

/** Open a card through a full refresh so the deferred controller holds one. */
async function withCard(d: ReturnType<typeof deferred>) {
  void d.controller.refresh()
  await d.reply('inspector', ok({ inspector }))
  await d.reply('card', ok({ card }))
  expect(d.controller.current.card).toBe(card)
}

describe('panel overlays: stale replies', () => {
  it('drops a pending scroll card reply after the card closes', async () => {
    const d = deferred()
    await withCard(d)
    void d.controller.scrollCard(2)
    await d.reply('scroll-card', ok())
    const scrolled = d.take('card')
    void d.controller.closeCard()
    expect(d.controller.current.card).toBeNull()
    scrolled.resolve(ok({ card: { ...card, scroll: 96 } }))
    await d.flush()
    expect(d.controller.current.card).toBeNull()
    await d.reply('close-card', ok())
    await d.reply('inspector', ok({ inspector: closedInspector }))
    expect(d.controller.current.card).toBeNull()
    expect(d.pending).toEqual([])
  })

  it('drops a pending scroll-card ack after the card closes, so no card fetch follows', async () => {
    const d = deferred()
    await withCard(d)
    void d.controller.scrollCard(1)
    const scroll = d.take('scroll-card')
    void d.controller.closeCard()
    scroll.resolve(ok())
    await d.flush()
    expect(d.pending.map(entry => entry.action.action)).toEqual(['close-card'])
  })

  it('drops a frame refresh card reply and its refusal when the card closes meanwhile', async () => {
    const d = deferred()
    await withCard(d)
    void d.controller.refresh()
    await d.reply('inspector', ok({ inspector }))
    const frameCard = d.take('card')
    void d.controller.closeCard()
    frameCard.resolve(ok({ card }))
    await d.flush()
    expect(d.controller.current.card).toBeNull()
    await d.reply('close-card', ok())
    await d.reply('inspector', ok({ inspector: closedInspector }))
    expect(d.controller.current).toMatchObject({ card: null, failure: null })

    const again = deferred()
    await withCard(again)
    void again.controller.refresh()
    await again.reply('inspector', ok({ inspector }))
    const refused = again.take('card')
    void again.controller.closeCard()
    refused.resolve({ accepted: false, reason: 'card gone' })
    await again.flush()
    expect(again.controller.current.failure).toBeNull()
  })

  it('does not let a frame refresh fetch the card while a card action is pending', async () => {
    const d = deferred()
    await withCard(d)
    void d.controller.closeCard()
    void d.controller.refresh()
    await d.reply('inspector', ok({ inspector }))
    expect(d.pending.map(entry => entry.action.action)).toEqual(['close-card'])
    expect(d.controller.current.card).toBeNull()
    await d.reply('close-card', ok())
    await d.reply('inspector', ok({ inspector: closedInspector }))
    expect(d.controller.current.card).toBeNull()
  })

  it('applies the card of a later open after an older open is still settling', async () => {
    const d = deferred()
    void d.controller.openCardAt([1, 1])
    void d.controller.openCardAt([2, 2])
    await d.reply('open-card', ok())
    // The older ack refreshes, but the newer open still owns the card fetch.
    await d.reply('inspector', ok({ inspector }))
    expect(d.pending.map(entry => entry.action.action)).toEqual(['open-card'])
    await d.reply('open-card', ok())
    await d.reply('inspector', ok({ inspector }))
    const second = { ...card, id: ':second' }
    await d.reply('card', ok({ card: second }))
    expect(d.controller.current.card).toBe(second)
  })

  it('keeps the newest outcome when an older refusal arrives late', async () => {
    const d = deferred()
    void d.controller.openSource(':order')
    void d.controller.openSource(':second')
    await d.reply('open-source', ok({ source: other }), ':second')
    await d.reply('open-source', { accepted: false, reason: 'old refusal' }, ':order')
    expect(d.controller.current.failure).toBeNull()
    void d.controller.openSource(':order', 'absent')
    await d.reply('open-source', { accepted: false, reason: 'Source not found' }, ':order')
    expect(d.controller.current.failure).toBe('Source not found')
    // A failed request rejects; it is still a refusal of the newest request.
    void d.controller.openSource(':order', 'boom')
    d.take('open-source', ':order').reject(new Error('resident gone'))
    await d.flush()
    expect(d.controller.current.failure).toBe('resident gone')
  })

  it('drops a source reply that arrives after the drawer went back to the card, but keeps a reopen', async () => {
    const d = deferred()
    await withCard(d)
    void d.controller.openSource(':order')
    await d.reply('open-source', ok({ source }), ':order')
    expect(d.controller.current.drawer).toBe('source')
    void d.controller.openSource(':order', 'target')
    const old = d.take('open-source', ':order')
    d.controller.backToCard()
    old.resolve(ok({ source: { ...source, line: 7 } }))
    await d.flush()
    expect(d.controller.current).toMatchObject({ source: null, drawer: 'card' })

    // An intentional reopen after going back is live.
    void d.controller.openSource(':order', 'again')
    await d.reply('open-source', ok({ source: { ...source, line: 5 } }), ':order')
    expect(d.controller.current.source?.line).toBe(5)
    expect(d.pending).toEqual([])
  })

  it('shows only the newest source request, whichever reply lands first', async () => {
    const d = deferred()
    void d.controller.openSource(':a')
    void d.controller.openSource(':b')
    const a = d.take('open-source', ':a')
    await d.reply('open-source', ok({ source: other }), ':b')
    a.resolve(ok({ source }))
    await d.flush()
    expect(d.controller.current.source).toBe(other)
  })

  it('drops a stale refusal for a source the drawer no longer shows', async () => {
    const d = deferred()
    await withCard(d)
    void d.controller.openSource(':order')
    await d.reply('open-source', ok({ source }), ':order')
    void d.controller.openSource(':order', 'absent')
    const refused = d.take('open-source', ':order')
    d.controller.backToCard()
    refused.resolve({ accepted: false, reason: 'Source not found' })
    await d.flush()
    expect(d.controller.current).toMatchObject({ source: null, drawer: 'card', failure: null })
  })

  it('ignores every reply and refusal after dispose', async () => {
    const d = deferred()
    await withCard(d)
    let published = 0
    d.controller.subscribe(() => { published += 1 })
    void d.controller.refresh()
    void d.controller.scrollCard(1)
    void d.controller.openSource(':order')
    void d.controller.closeCard()
    published = 0
    d.controller.dispose()
    for (const entry of d.pending.splice(0)) {
      entry.resolve(entry.action.action === 'open-source' ? ok({ source }) : { accepted: false, reason: 'late' })
    }
    await d.flush()
    expect(published).toBe(0)
    expect(d.controller.current).toMatchObject({ source: null, failure: null })
    expect(d.pending).toEqual([])
  })
})

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

  it('synchronizes native scrolling before subsequent wheel input', async () => {
    const { controller, actions } = harness()
    await controller.refresh()
    await controller.syncNativeScroll(192)
    expect(actions).toContainEqual({ action: 'scroll-card', delta: 4, viewHeight: 700 })
    await controller.scrollCard(-1)
    expect(actions).toContainEqual({ action: 'scroll-card', delta: -1, viewHeight: 700 })
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

  it('asks for a new proposal named with the local time unless one is given (#29)', async () => {
    const { controller, actions } = harness()
    await controller.newProposal(new Date(2026, 10, 9, 14, 5, 6))
    await controller.newProposal(new Date(2026, 10, 9, 14, 5, 7), 'Split core')
    expect(actions).toEqual([
      { action: 'new-proposal', name: '2026-11-09 14:05:06' },
      { action: 'new-proposal', name: 'Split core' },
    ])
  })

  it('shows a refused new-proposal request in the failure line', async () => {
    const controller = new OverlayController(async () => ({ accepted: false, reason: 'no sessionController' }))
    await controller.newProposal(new Date(2026, 10, 9, 14, 5, 6))
    expect(controller.current.failure).toBe('no sessionController')
  })

  it('shows one drawer: a member source replaces the card, and back returns to it', async () => {
    const { controller, actions } = harness()
    expect(controller.current.drawer).toBeNull()
    await controller.refresh()
    expect(controller.current.drawer).toBe('card')
    await controller.openSource(':order', 'target')
    expect(actions.at(-1)).toEqual({ action: 'open-source', id: ':order', member: 'target' })
    expect(controller.current).toMatchObject({ drawer: 'source', card, source })
    // Another source takes the same drawer; nothing stacks.
    await controller.openSource(':second')
    expect(controller.current.source?.file).toBe('src/second.rs')
    controller.backToCard()
    expect(controller.current).toMatchObject({ drawer: 'card', source: null, card })
  })

  it('shows the new card when a card opens while the drawer shows a source', async () => {
    const d = deferred()
    await withCard(d)
    void d.controller.openSource(':order', 'target')
    await d.reply('open-source', ok({ source }), ':order')
    expect(d.controller.current.drawer).toBe('source')

    void d.controller.openCardAt([12, 34])
    await d.reply('open-card', ok())
    await d.reply('inspector', ok({ inspector }))
    const second = { ...card, id: ':second' }
    await d.reply('card', ok({ card: second }))
    // The double-click on another class owns the drawer; no stale source stays.
    expect(d.controller.current).toMatchObject({ drawer: 'card', source: null, card: second })
  })

  it('drops a source reply still in flight when another card action takes the drawer', async () => {
    const d = deferred()
    await withCard(d)
    void d.controller.openSource(':order', 'target')
    const stale = d.take('open-source', ':order')
    void d.controller.namespaceTree()
    stale.resolve(ok({ source }))
    await d.flush()
    expect(d.controller.current.source).toBeNull()
  })

  it('closes the whole drawer, the source and its card together', async () => {
    const { controller, actions } = harness()
    await controller.refresh()
    await controller.openSource(':order', 'target')
    expect(controller.drawerOpen).toBe(true)
    await controller.closeDrawer()
    expect(controller.current).toMatchObject({ drawer: null, source: null, card: null })
    expect(controller.drawerOpen).toBe(false)
    expect(actions).toContainEqual({ action: 'close-card' })
  })

  it('keeps the shown source and reports a failed source request', async () => {
    const { controller } = harness()
    await controller.openSource(':order')
    await controller.openSource(':order', 'absent')
    expect(controller.current.failure).toBe('Source not found')
    expect(controller.current.source).toEqual(source)
    controller.dispose()
    await controller.openSource(':second')
    expect(controller.current.source).toEqual(source)
  })
})
