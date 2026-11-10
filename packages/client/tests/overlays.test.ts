import { describe, expect, it } from 'vitest'
import type { CardModel, PanelAck, PanelAction, SourceModel } from '@dsh-uml-viewer/contracts'
import { OverlayController } from '../src/overlays.js'

const card: CardModel = { id: ':order', title: 'order', items: [], contentHeight: 1000, viewHeight: 700, maxScroll: 300, scroll: 0 }
const source: SourceModel = { file: 'src/order.rs', title: 'src/order.rs:82', line: 82, lang: 'rust', rows: [
  { number: 82, text: '\tpub fn target()', display: '  pub fn target()', marked: true, tokens: [{ text: 'pub', kind: 'keyword' }] },
] }

function harness() {
  const actions: PanelAction[] = []
  // The resident answers `card: null` once no card is open (#47).
  let open = true
  const controller = new OverlayController(async action => {
    actions.push(action)
    if (action.action === 'card') return { accepted: true, reason: null, card: open ? card : null }
    if (action.action === 'open-source') {
      if (action.member === 'absent') return { accepted: false, reason: 'Source not found' }
      const file = action.id === ':second' ? 'src/second.rs' : source.file
      return { accepted: true, reason: null, source: { ...source, file } }
    }
    if (action.action === 'close-card') open = false
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
const other: SourceModel = { ...source, file: 'src/other.rs', title: 'src/other.rs:1' }

/** Open a card through a full refresh so the deferred controller holds one. */
async function withCard(d: ReturnType<typeof deferred>) {
  void d.controller.refresh()
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
    await d.reply('card', ok({ card: null }))
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
    const frameCard = d.take('card')
    void d.controller.closeCard()
    frameCard.resolve(ok({ card }))
    await d.flush()
    expect(d.controller.current.card).toBeNull()
    await d.reply('close-card', ok())
    await d.reply('card', ok({ card: null }))
    expect(d.controller.current).toMatchObject({ card: null, failure: null })

    const again = deferred()
    await withCard(again)
    void again.controller.refresh()
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
    expect(d.pending.map(entry => entry.action.action)).toEqual(['close-card'])
    expect(d.controller.current.card).toBeNull()
    await d.reply('close-card', ok())
    await d.reply('card', ok({ card: null }))
    expect(d.controller.current.card).toBeNull()
  })

  it('applies the card of a later open after an older open is still settling', async () => {
    const d = deferred()
    void d.controller.openCardAt([1, 1])
    void d.controller.openCardAt([2, 2])
    await d.reply('open-card', ok())
    // The older ack refreshes, but the newer open still owns the card fetch.
    expect(d.pending.map(entry => entry.action.action)).toEqual(['open-card'])
    await d.reply('open-card', ok())
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

  it('drops a same-file source reply after its window closes, but keeps a reopen', async () => {
    const d = deferred()
    void d.controller.openSource(':order')
    await d.reply('open-source', ok({ source }), ':order')
    const serial = d.controller.current.sources[0]!.serial
    void d.controller.openSource(':order', 'target')
    const old = d.take('open-source', ':order')
    d.controller.closeSource(serial)
    old.resolve(ok({ source: { ...source, line: 7 } }))
    await d.flush()
    expect(d.controller.current.sources).toEqual([])

    // An intentional reopen after the close is live.
    void d.controller.openSource(':order', 'again')
    await d.reply('open-source', ok({ source: { ...source, line: 5 } }), ':order')
    expect(d.controller.current.sources.map(window => window.model.line)).toEqual([5])

    // A request for the same file issued before a close, landing after the
    // reopen's reply, neither reopens nor overwrites the reopened window.
    void d.controller.openSource(':alias')
    const before = d.take('open-source', ':alias')
    d.controller.closeNewestSource()
    void d.controller.openSource(':order', 'after')
    await d.reply('open-source', ok({ source: { ...source, line: 9 } }), ':order')
    before.resolve(ok({ source: { ...source, line: 1 } }))
    await d.flush()
    expect(d.controller.current.sources.map(window => window.model.line)).toEqual([9])
    expect(d.pending).toEqual([])
  })

  it('drops a stale refusal for a closed source window', async () => {
    const d = deferred()
    void d.controller.openSource(':order')
    await d.reply('open-source', ok({ source }), ':order')
    void d.controller.openSource(':order', 'absent')
    const refused = d.take('open-source', ':order')
    d.controller.closeNewestSource()
    refused.resolve({ accepted: false, reason: 'Source not found' })
    await d.flush()
    expect(d.controller.current).toMatchObject({ sources: [], failure: null })
  })

  it('keeps coexisting files and their pending requests when another file closes', async () => {
    const d = deferred()
    void d.controller.openSource(':order')
    await d.reply('open-source', ok({ source }), ':order')
    const first = d.controller.current.sources[0]!.serial
    void d.controller.openSource(':other')
    const pendingOther = d.take('open-source', ':other')
    d.controller.closeSource(first)
    pendingOther.resolve(ok({ source: other }))
    await d.flush()
    expect(d.controller.current.sources.map(window => window.model.file)).toEqual(['src/other.rs'])
    // An older reply from another id for the same file loses to a newer one.
    void d.controller.openSource(':a')
    void d.controller.openSource(':b')
    const a = d.take('open-source', ':a')
    await d.reply('open-source', ok({ source: { ...source, line: 2 } }), ':b')
    a.resolve(ok({ source: { ...source, line: 1 } }))
    await d.flush()
    expect(d.controller.current.sources.map(window => [window.model.file, window.model.line]))
      .toEqual([['src/other.rs', 82], ['src/order.rs', 2]])
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
    expect(d.controller.current).toMatchObject({ sources: [], failure: null })
    expect(d.pending).toEqual([])
  })
})

describe('panel overlays', () => {
  it('asks for the card directly and shows it without deriving quality text or scroll (#47)', async () => {
    const { controller, actions } = harness()
    await controller.refresh()
    expect(controller.current.card).toBe(card)
    expect(actions).toEqual([{ action: 'card', width: 624, viewHeight: 700 }])
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

  it('clears the card when the resident answers that none is open (#47)', async () => {
    const d = deferred()
    await withCard(d)
    void d.controller.refresh()
    await d.reply('card', ok({ card: null }))
    expect(d.controller.current).toMatchObject({ card: null, failure: null })
    expect(d.pending).toEqual([])
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
