/**
 * The panel overlays (#27): inspector, class card and source windows.
 *
 * This controller owns *panel* state only: which model the resident last
 * sent, which source windows are open and in which order. Every text, count,
 * quality figure, card row and source line arrives in an ack payload produced
 * by the Rust models; nothing here recomputes them, and no file is read.
 *
 * Replies arrive asynchronously, so every request carries the freshness it was
 * issued under and a reply (or refusal) that has gone stale is dropped:
 *
 * - `generation`: each inspector refresh supersedes the previous one.
 * - `cardEpoch`: every card-affecting user action (open, close, scroll,
 *   context, P, declutter, locate) invalidates card replies already in flight,
 *   so a pending scroll or refresh cannot repopulate a closed card.
 * - `cardRequest`: of the card fetches in flight, only the newest applies.
 * - Source windows are keyed by file: each class id's newest request is the
 *   only live one for that id, and closing a window invalidates replies for
 *   that file issued before the close. Other files' requests are unaffected,
 *   and a request issued after the close (an intentional reopen) stays live.
 * - `intent` orders every user/frame request; a failure is shown only when no
 *   newer request has already reported success or failure, so a late refusal
 *   cannot overwrite a fresher outcome.
 */
import type { CardModel, InspectorModel, PanelAck, PanelAction, SourceModel } from '@dsh-uml-viewer/contracts'
import { defaultProposalName } from '@dsh-uml-viewer/contracts'
import { ENGLISH_TEXT, type PanelText } from './locale.js'

export interface SourceWindow {
  serial: number
  model: SourceModel
}

export interface OverlaySnapshot {
  inspector: InspectorModel | null
  card: CardModel | null
  sources: readonly SourceWindow[]
  /** The last refused overlay action, shown in the panel's status line. */
  failure: string | null
}

export type Send = (action: PanelAction) => Promise<PanelAck | null>

/** The geometry the card is laid out for; the resident needs it for its plan. */
export interface CardGeometry { width: number; viewHeight: number }

export class OverlayController {
  private inspector: InspectorModel | null = null
  private card: CardModel | null = null
  private sources: SourceWindow[] = []
  private failure: string | null = null
  private serial = 0
  private generation = 0
  private cardEpoch = 0
  private cardRequest = 0
  /** Card actions whose ack has not arrived; their own refresh fetches the card. */
  private cardActions = 0
  private intent = 0
  private failureIntent = 0
  /** Newest open-source intent per class id. */
  private readonly sourceRequests = new Map<string, number>()
  /** The file each class id last opened, learnt from a fresh reply. */
  private readonly sourceFiles = new Map<string, string>()
  /** Per file, the intent of its last applied open or close. */
  private readonly fileIntents = new Map<string, number>()
  private closed = false
  private readonly listeners = new Set<() => void>()
  private geometry: CardGeometry = { width: 624, viewHeight: 700 }
  private nativeOffset: number | null = null
  private nativeScrollTask: Promise<void> | null = null

  /** The panel's DOM text; the view re-renders on a DSH language switch. */
  public constructor(private readonly send: Send, public readonly text: PanelText = ENGLISH_TEXT) {}

  public get current(): OverlaySnapshot {
    return { inspector: this.inspector, card: this.card, sources: this.sources, failure: this.failure }
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

  /** Pull the inspector, and the card when one is open, after a view change. */
  public async refresh(): Promise<void> {
    if (this.closed) return
    await this.load(++this.intent)
  }

  private async load(intent: number): Promise<void> {
    const generation = ++this.generation
    const epoch = this.cardEpoch
    const live = () => !this.closed && generation === this.generation
    const ack = await this.request({ action: 'inspector' }, live, intent)
    if (!ack?.accepted || !ack.inspector) return
    this.inspector = ack.inspector
    this.succeed(intent)
    this.publish()
    const cardLive = () => live() && epoch === this.cardEpoch && this.cardActions === 0
    if (!cardLive()) return
    if (ack.inspector.detail === null) {
      if (this.card === null) return
      this.card = null
      this.publish()
      return
    }
    const request = ++this.cardRequest
    const current = () => cardLive() && request === this.cardRequest
    const card = await this.request({ action: 'card', ...this.geometry }, current, intent)
    if (!card?.accepted || !current()) return
    this.card = card.card ?? null
    this.publish()
  }

  /** A card-affecting action: invalidate card replies in flight, then refresh. */
  private async changeCard(action: PanelAction, close = false): Promise<void> {
    if (this.closed) return
    const intent = ++this.intent
    this.cardEpoch += 1
    this.cardActions += 1
    this.nativeOffset = null
    if (close && this.card !== null) {
      this.card = null
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

  /** The action bar's Open class card (#48): that class's card; the view stays. */
  public async openCard(id: string): Promise<void> {
    await this.changeCard({ action: 'open-card', id })
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
   * Open a source window on a module (`member` undefined) or a member. The
   * same file keeps one window, which moves to the new member (egui parity).
   * Windows on other files stay open, including ones whose requests are still
   * in flight.
   */
  public async openSource(id: string, member?: string): Promise<void> {
    if (this.closed) return
    const intent = ++this.intent
    this.sourceRequests.set(id, intent)
    const fresh = (file: string | undefined) => file === undefined || intent > (this.fileIntents.get(file) ?? 0)
    const live = () => this.sourceRequests.get(id) === intent && fresh(this.sourceFiles.get(id))
    const ack = await this.request({ action: 'open-source', id, ...(member === undefined ? {} : { member }) }, live, intent)
    const model = ack?.accepted ? ack.source : undefined
    // The reply names its file: a close of that file, or a newer reply for it
    // from another id, issued after this request makes this reply stale.
    if (!model || !fresh(model.file)) return
    this.sourceFiles.set(id, model.file)
    this.fileIntents.set(model.file, intent)
    const existing = this.sources.findIndex(window => window.model.file === model.file)
    const serial = existing >= 0 ? this.sources[existing]!.serial : ++this.serial
    if (existing >= 0) this.sources.splice(existing, 1)
    this.sources.push({ serial, model })
    this.succeed(intent)
    this.publish()
  }

  public closeSource(serial: number): void {
    if (this.closed) return
    const window = this.sources.find(window => window.serial === serial)
    if (!window) return
    this.fileIntents.set(window.model.file, ++this.intent)
    this.sources = this.sources.filter(window => window.serial !== serial)
    this.publish()
  }

  /** Close the newest source window; the older ones stay. */
  public closeNewestSource(): void {
    const newest = this.sources[this.sources.length - 1]
    if (newest) this.closeSource(newest.serial)
  }
}
