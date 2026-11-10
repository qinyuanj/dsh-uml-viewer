/**
 * The panel overlays (#27): the drawer (#49) that shows the class card or one
 * source window. The inspector was deleted in #47; the discussion-context
 * switcher (`context-switcher.ts`) replaced its proposal list and still acts
 * through `choose`/`newProposal`.
 *
 * This controller owns *panel* state only: which model the resident last
 * sent, and whether the drawer shows the card or a source. Every text, count,
 * quality figure, card row and source line arrives in an ack payload produced
 * by the Rust models; nothing here recomputes them, and no file is read.
 *
 * Replies arrive asynchronously, so every request carries the freshness it was
 * issued under and a reply (or refusal) that has gone stale is dropped:
 *
 * - `generation`: each card refresh supersedes the previous one.
 * - `cardEpoch`: every card-affecting user action (open, close, scroll,
 *   context, P, declutter, locate) invalidates card replies already in flight,
 *   so a pending scroll or refresh cannot repopulate a closed card.
 * - `cardRequest`: of the card fetches in flight, only the newest applies.
 * - The drawer shows at most one source: only the newest open-source request
 *   is live, and going back to the card or closing the drawer invalidates
 *   source replies issued before it. A request issued after it (an
 *   intentional reopen) stays live.
 * - `intent` orders every user/frame request; a failure is shown only when no
 *   newer request has already reported success or failure, so a late refusal
 *   cannot overwrite a fresher outcome.
 */
import type { CardModel, PanelAck, PanelAction, SourceModel } from '@dsh-uml-viewer/contracts'
import { defaultProposalName } from '@dsh-uml-viewer/contracts'
import { ENGLISH_TEXT, type PanelText } from './locale.js'

/** What the drawer shows: a source replaces the card it was opened from. */
export type DrawerView = 'card' | 'source'

export interface OverlaySnapshot {
  card: CardModel | null
  /** The one source window; while it is set the drawer shows it, not the card. */
  source: SourceModel | null
  drawer: DrawerView | null
  /** The last refused overlay action, shown in the panel's status line. */
  failure: string | null
}

export type Send = (action: PanelAction) => Promise<PanelAck | null>

/** The geometry the card is laid out for; the resident needs it for its plan. */
export interface CardGeometry { width: number; viewHeight: number }

export class OverlayController {
  private card: CardModel | null = null
  private source: SourceModel | null = null
  private failure: string | null = null
  private generation = 0
  private cardEpoch = 0
  private cardRequest = 0
  /** Card actions whose ack has not arrived; their own refresh fetches the card. */
  private cardActions = 0
  private intent = 0
  private failureIntent = 0
  /** The newest open-source intent; only its reply may fill the drawer. */
  private sourceRequest = 0
  private closed = false
  private readonly listeners = new Set<() => void>()
  private geometry: CardGeometry = { width: 624, viewHeight: 700 }
  private nativeOffset: number | null = null
  private nativeScrollTask: Promise<void> | null = null

  /** The panel's DOM text; the view re-renders on a DSH language switch. */
  public constructor(private readonly send: Send, public readonly text: PanelText = ENGLISH_TEXT) {}

  public get current(): OverlaySnapshot {
    const drawer = this.source !== null ? 'source' : this.card !== null ? 'card' : null
    return { card: this.card, source: this.source, drawer, failure: this.failure }
  }

  /** Whether the drawer shows anything: Esc closes it before going back (#49). */
  public get drawerOpen(): boolean {
    return this.source !== null || this.card !== null
  }

  public subscribe(listener: () => void): () => void {
    this.listeners.add(listener)
    return () => this.listeners.delete(listener)
  }

  private publish(): void {
    for (const listener of this.listeners) listener()
  }

  public dispose(): void {
    this.closed = true
    this.generation += 1
    this.cardEpoch += 1
    this.nativeOffset = null
    this.listeners.clear()
  }

  /** The card's content width and viewport height, measured by the view. */
  public setCardGeometry(geometry: CardGeometry): void {
    if (geometry.width === this.geometry.width && geometry.viewHeight === this.geometry.viewHeight) return
    this.geometry = geometry
  }

  private fail(intent: number, reason: string): void {
    if (intent < this.failureIntent) return
    this.failureIntent = intent
    this.failure = reason
    this.publish()
  }

  /** A success clears only failures from strictly older requests. */
  private succeed(intent: number): void {
    if (intent <= this.failureIntent) return
    this.failureIntent = intent
    this.failure = null
  }

  /** Send `action`; a reply that is no longer `live` resolves to null. */
  private async request(action: PanelAction, live: () => boolean, intent: number): Promise<PanelAck | null> {
    if (this.closed || !live()) return null
    let ack: PanelAck | null
    try {
      ack = await this.send(action)
    } catch (error) {
      ack = { accepted: false, reason: error instanceof Error ? error.message : String(error) }
    }
    if (this.closed || !live()) return null
    if (ack && !ack.accepted) this.fail(intent, ack.reason ?? this.text.t('request.refused'))
    return ack
  }

  /** Pull the open class card after a view change; none open clears it. */
  public async refresh(): Promise<void> {
    if (this.closed) return
    await this.load(++this.intent)
  }

  /**
   * Ask the resident for the card directly (#47): it answers `null` when no
   * card is open, which clears the one shown here.
   */
  private async load(intent: number): Promise<void> {
    const generation = ++this.generation
    const epoch = this.cardEpoch
    const cardLive = () => !this.closed && generation === this.generation && epoch === this.cardEpoch
      && this.cardActions === 0
    if (!cardLive()) return
    const request = ++this.cardRequest
    const current = () => cardLive() && request === this.cardRequest
    const ack = await this.request({ action: 'card', ...this.geometry }, current, intent)
    if (!ack?.accepted || !current()) return
    this.succeed(intent)
    const card = ack.card ?? null
    if (card === null && this.card === null) return
    this.card = card
    this.publish()
  }

  /** A card-affecting action: invalidate card replies in flight, then refresh. */
  private async changeCard(action: PanelAction, close = false): Promise<void> {
    if (this.closed) return
    const intent = ++this.intent
    this.cardEpoch += 1
    this.cardActions += 1
    this.nativeOffset = null
    // The drawer shows one view (#49): a card action takes it back from the
    // source — a double-click on another class must not leave a stale source
    // in place — and a source reply still in flight must not bring one back.
    this.sourceRequest = intent
    const hadSource = this.source !== null
    this.source = null
    if (close && this.card !== null) {
      this.card = null
      this.publish()
    } else if (hadSource) {
      this.publish()
    }
    let ack: PanelAck | null
    try {
      ack = await this.request(action, () => true, intent)
    } finally {
      this.cardActions -= 1
    }
    // The same intent keeps this action's refusal visible through its refresh.
    if (!this.closed && ack !== null) await this.load(intent)
  }

  /** Double-click on the canvas: a namespace drills, a class opens its card. */
  public async openCardAt(at: [number, number]): Promise<void> {
    await this.changeCard({ action: 'open-card', at })
  }

  public async closeCard(): Promise<void> {
    await this.changeCard({ action: 'close-card' }, true)
  }

  /** Wheel notches over the card; the resident owns the step and the clamps. */
  public async scrollCard(delta: number): Promise<void> {
    if (delta === 0 || this.closed) return
    const intent = ++this.intent
    const epoch = ++this.cardEpoch
    const live = () => epoch === this.cardEpoch
    const ack = await this.request({ action: 'scroll-card', delta, viewHeight: this.geometry.viewHeight }, live, intent)
    if (!ack?.accepted) return
    const request = ++this.cardRequest
    const current = () => live() && request === this.cardRequest
    const result = await this.request({ action: 'card', ...this.geometry }, current, intent)
    if (!result?.accepted || !result.card || !current()) return
    this.card = result.card
    this.succeed(intent)
    this.publish()
  }

  /** Translate a native scrollbar/keyboard pixel position into the protocol's
   * 48-point notch units. Rust still owns the step, plan and clamping. Coalesce
   * drag events so each relative input starts from the last acknowledged plan. */
  public syncNativeScroll(offset: number): Promise<void> {
    this.nativeOffset = offset
    this.nativeScrollTask ??= this.drainNativeScroll().finally(() => { this.nativeScrollTask = null })
    return this.nativeScrollTask
  }

  private async drainNativeScroll(): Promise<void> {
    while (this.nativeOffset !== null && !this.closed) {
      const target = this.nativeOffset
      this.nativeOffset = null
      if (this.card && Math.abs(target - this.card.scroll) > 0.5) {
        await this.scrollCard((target - this.card.scroll) / 48)
      }
    }
  }

  /** Choose the source diagram (null) or a proposal as the discussion context. */
  public async choose(id: string | null): Promise<void> {
    await this.changeCard({ action: 'context', proposalId: id })
  }

  /**
   * 新建提案 (#29): ask the companion for a proposal named `name`, or with
   * the local time to the second. The proposal list changes only when the
   * companion's regenerated IR is displayed, so nothing here changes now.
   */
  public async newProposal(now: Date, name?: string): Promise<void> {
    if (this.closed) return
    const intent = ++this.intent
    const ack = await this.request({ action: 'new-proposal', name: name ?? defaultProposalName(now) }, () => true, intent)
    if (ack?.accepted) this.succeed(intent)
  }

  /** **P**: back to the source diagram's namespace tree. */
  public async namespaceTree(): Promise<void> {
    await this.changeCard({ action: 'namespace-tree' })
  }

  public async declutter(): Promise<void> {
    await this.changeCard({ action: 'declutter' })
  }

  /** A card's relationship row: locate that class in the view. */
  public async locate(id: string): Promise<void> {
    await this.changeCard({ action: 'locate', id })
  }

  /**
   * Show a module's (`member` undefined) or a member's source in the drawer,
   * in place of the card; the drawer keeps one source, so a newer request
   * replaces it rather than stacking a window.
   */
  public async openSource(id: string, member?: string): Promise<void> {
    if (this.closed) return
    const intent = ++this.intent
    this.sourceRequest = intent
    const live = () => this.sourceRequest === intent
    const ack = await this.request({ action: 'open-source', id, ...(member === undefined ? {} : { member }) }, live, intent)
    const model = ack?.accepted ? ack.source : undefined
    if (!model) return
    this.source = model
    this.succeed(intent)
    this.publish()
  }

  /** The source's back button: the drawer shows the card again. */
  public backToCard(): void {
    if (this.closed) return
    // A source reply still in flight would bring the source back.
    this.sourceRequest = ++this.intent
    if (this.source === null) return
    this.source = null
    this.publish()
  }

  /** Close the drawer: the source and the card it came from go together. */
  public async closeDrawer(): Promise<void> {
    this.backToCard()
    if (this.card !== null) await this.closeCard()
  }
}
