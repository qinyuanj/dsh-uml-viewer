import type { CardModel, PanelAck, PanelAction, SourceModel } from '@dsh-uml-viewer/contracts'
import { defaultProposalName } from '@dsh-uml-viewer/contracts'
import { ENGLISH_TEXT, type PanelText } from './locale.js'

export type DrawerView = 'card' | 'source'

export interface OverlaySnapshot {
  card: CardModel | null
  source: SourceModel | null
  drawer: DrawerView | null
  failure: string | null
}

export type Send = (action: PanelAction) => Promise<PanelAck | null>

export interface CardGeometry { width: number; viewHeight: number }

export class OverlayController {
  private card: CardModel | null = null
  private source: SourceModel | null = null
  private failure: string | null = null
  private generation = 0
  private cardEpoch = 0
  private cardRequest = 0
  private cardActions = 0
  private intent = 0
  private failureIntent = 0
  private sourceRequest = 0
  private closed = false
  private readonly listeners = new Set<() => void>()
  private geometry: CardGeometry = { width: 624, viewHeight: 700 }
  private nativeOffset: number | null = null
  private nativeScrollTask: Promise<void> | null = null

  public constructor(private readonly send: Send, public readonly text: PanelText = ENGLISH_TEXT) {}

  public get current(): OverlaySnapshot {
    const drawer = this.source !== null ? 'source' : this.card !== null ? 'card' : null
    return { card: this.card, source: this.source, drawer, failure: this.failure }
  }

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

  private succeed(intent: number): void {
    if (intent <= this.failureIntent) return
    this.failureIntent = intent
    this.failure = null
  }

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

  public async refresh(): Promise<void> {
    if (this.closed) return
    await this.load(++this.intent)
  }

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

  private async changeCard(action: PanelAction, close = false): Promise<void> {
    if (this.closed) return
    const intent = ++this.intent
    this.cardEpoch += 1
    this.cardActions += 1
    this.nativeOffset = null
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
    if (!this.closed && ack !== null) await this.load(intent)
  }

  public async openCardAt(at: [number, number]): Promise<void> {
    await this.changeCard({ action: 'open-card', at })
  }

  public async openCard(id: string): Promise<void> {
    await this.changeCard({ action: 'open-card', id })
  }

  public async closeCard(): Promise<void> {
    await this.changeCard({ action: 'close-card' }, true)
  }

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

  public async choose(id: string | null): Promise<void> {
    await this.changeCard({ action: 'context', proposalId: id })
  }

  public async newProposal(now: Date, name?: string): Promise<void> {
    if (this.closed) return
    const intent = ++this.intent
    const ack = await this.request({ action: 'new-proposal', name: name ?? defaultProposalName(now) }, () => true, intent)
    if (ack?.accepted) this.succeed(intent)
  }

  public async namespaceTree(): Promise<void> {
    await this.changeCard({ action: 'namespace-tree' })
  }

  public async declutter(): Promise<void> {
    await this.changeCard({ action: 'declutter' })
  }

  public async locate(id: string): Promise<void> {
    await this.changeCard({ action: 'locate', id })
  }

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

  public backToCard(): void {
    if (this.closed) return
    this.sourceRequest = ++this.intent
    if (this.source === null) return
    this.source = null
    this.publish()
  }

  public async closeDrawer(): Promise<void> {
    this.backToCard()
    if (this.card !== null) await this.closeCard()
  }
}
