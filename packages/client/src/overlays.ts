/**
 * The panel overlays (#27): inspector, class card and source windows.
 *
 * This controller owns *panel* state only: which model the resident last
 * sent, which source windows are open and in which order. Every text, count,
 * quality figure, card row and source line arrives in an ack payload produced
 * by the Rust models; nothing here recomputes them, and no file is read.
 */
import type { CardModel, InspectorModel, PanelAck, PanelAction, SourceModel } from '@dsh-uml-viewer/contracts'

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
  private closed = false
  private readonly listeners = new Set<() => void>()
  private geometry: CardGeometry = { width: 624, viewHeight: 700 }
  private nativeOffset: number | null = null
  private nativeScrollTask: Promise<void> | null = null

  public constructor(private readonly send: Send) {}

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
    this.listeners.clear()
  }

  /** The card's content width and viewport height, measured by the view. */
  public setCardGeometry(geometry: CardGeometry): void {
    if (geometry.width === this.geometry.width && geometry.viewHeight === this.geometry.viewHeight) return
    this.geometry = geometry
  }

  private async request(action: PanelAction): Promise<PanelAck | null> {
    if (this.closed) return null
    let ack: PanelAck | null
    try {
      ack = await this.send(action)
    } catch (error) {
      ack = { accepted: false, reason: error instanceof Error ? error.message : String(error) }
    }
    if (!this.closed && ack && !ack.accepted) {
      this.failure = ack.reason ?? 'panel action refused'
      this.publish()
    }
    return ack
  }

  /** Pull the inspector, and the card when one is open, after a view change. */
  public async refresh(): Promise<void> {
    const generation = ++this.generation
    const ack = await this.request({ action: 'inspector' })
    if (this.closed || generation !== this.generation) return
    if (ack?.inspector) {
      this.inspector = ack.inspector
      this.failure = null
      if (ack.inspector.detail !== null) {
        const card = await this.request({ action: 'card', ...this.geometry })
        if (this.closed || generation !== this.generation) return
        this.card = card?.card ?? null
      } else {
        this.card = null
      }
      this.publish()
    }
  }

  /** Double-click on the canvas: a namespace drills, a class opens its card. */
  public async openCardAt(at: [number, number]): Promise<void> {
    await this.request({ action: 'open-card', at })
    await this.refresh()
  }

  public async closeCard(): Promise<void> {
    await this.request({ action: 'close-card' })
    this.card = null
    await this.refresh()
  }

  /** Wheel notches over the card; the resident owns the step and the clamps. */
  public async scrollCard(delta: number): Promise<void> {
    if (delta === 0) return
    await this.request({ action: 'scroll-card', delta, viewHeight: this.geometry.viewHeight })
    const ack = await this.request({ action: 'card', ...this.geometry })
    if (this.closed) return
    if (ack?.card) {
      this.card = ack.card
      this.failure = null
      this.publish()
    }
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
    await this.request({ action: 'context', proposalId: id })
    await this.refresh()
  }

  /** **P**: back to the source diagram's namespace tree. */
  public async namespaceTree(): Promise<void> {
    await this.request({ action: 'namespace-tree' })
    await this.refresh()
  }

  public async declutter(): Promise<void> {
    await this.request({ action: 'declutter' })
    await this.refresh()
  }

  /** A card's relationship row: locate that class in the view. */
  public async locate(id: string): Promise<void> {
    await this.request({ action: 'locate', id })
    await this.refresh()
  }

  /**
   * Open a source window on a module (`member` undefined) or a member. The
   * same file keeps one window, which moves to the new member (egui parity).
   */
  public async openSource(id: string, member?: string): Promise<void> {
    const ack = await this.request({ action: 'open-source', id, ...(member === undefined ? {} : { member }) })
    if (this.closed) return
    const model = ack?.source
    if (!model) return
    const existing = this.sources.findIndex(window => window.model.file === model.file)
    const serial = existing >= 0 ? this.sources[existing]!.serial : ++this.serial
    if (existing >= 0) this.sources.splice(existing, 1)
    this.sources.push({ serial, model })
    this.failure = null
    this.publish()
  }

  public closeSource(serial: number): void {
    this.sources = this.sources.filter(window => window.serial !== serial)
    this.publish()
  }

  /** Close the newest source window; the older ones stay. */
  public closeNewestSource(): void {
    const newest = this.sources[this.sources.length - 1]
    if (newest) this.closeSource(newest.serial)
  }
}
