import type { BreadcrumbLevel, DeclutterMode, DiscussionContext, PanelAck, PanelAction, PanelEvent, PanelOwner,
  ProposalEntry, RequestStatusEntry, SceneFrame, SearchResult, SessionStatus } from '@dsh-uml-viewer/contracts'
import { hitTest, paintFrame } from './paint.js'
import { hoverAt, sameHover, showsPopup, withHover, type HoverHit } from './hover.js'
import { cssToWorld, screenToWorld, viewTransform } from './fit.js'

export interface PanelStream extends AsyncIterable<PanelEvent> {
  dispose?(): void
  end?(): void
}

export interface PanelRemoteApi {
  watch(sessionId: string): PanelStream
  watchOwned?(sessionId: string, owner: PanelOwner, language: string): PanelStream
  closeOwned?(sessionId: string, owner: PanelOwner): Promise<unknown>
  action(sessionId: string, action: PanelAction): Promise<unknown>
  close?(sessionId: string): Promise<unknown>
}

export interface SearchState {
  active: boolean
  query: string
  results: SearchResult[]
  selection: number
}

export interface PanelStatus {
  kind: 'connecting' | 'live' | 'refused' | 'failed' | 'closed'
  reason: string | null
  actions: readonly string[]
  frame: SceneFrame | null
  context: DiscussionContext | null
  search: SearchState
  breadcrumbs: readonly BreadcrumbLevel[]
  statusCode: SessionStatus | null
  view: 'source' | 'proposal'
  proposals: readonly ProposalEntry[]
  requests: readonly RequestStatusEntry[]
}

export interface PanelDeps {
  canvas: HTMLCanvasElement
  api: PanelRemoteApi
  paint?: (frame: SceneFrame, canvas: HTMLCanvasElement) => void
  onStatus?: (status: PanelStatus) => void
  owner?: () => PanelOwner
  language?: () => string
}

export interface KeyModifiers { control?: boolean; shift?: boolean }

export interface DrawerHook {
  open(): boolean
  close(): void
}

export interface LegendHook {
  isOpen(): boolean
  close(): void
}

type Listener = () => void

export const ZOOM_STEP = 1.1
const SCROLL_UNIT = 48
const SCENE_BOUNDS_INSET = 16
const SCENE_OUTER_MARGIN = 40
const ZOOM_MIN = 0.1
const ZOOM_MAX = 10

const EMPTY_SEARCH: SearchState = { active: false, query: '', results: [], selection: -1 }

export class PanelController {
  private readonly canvas: HTMLCanvasElement
  private readonly api: PanelRemoteApi
  private readonly paint: (frame: SceneFrame, canvas: HTMLCanvasElement) => void
  private readonly onStatus: ((status: PanelStatus) => void) | undefined
  private abort: AbortController | null = null
  private stream: PanelStream | null = null
  private sessionId: string | null = null
  private owner: PanelOwner | null = null
  private readonly makeOwner: (() => PanelOwner) | undefined
  private readonly language: () => string
  private sentLanguage: string | null = null
  private frame: SceneFrame | null = null
  private status: PanelStatus = { kind: 'connecting', reason: null, actions: [], frame: null, context: null,
    search: EMPTY_SEARCH, breadcrumbs: [], statusCode: null, view: 'source', proposals: [], requests: [] }
  private observer: MutationObserver | null = null
  private readonly listeners = new Set<Listener>()
  private mode = 'full'
  public get declutterMode(): string { return this.mode }
  private selectedHit: string | null = null
  public get selected(): string | null { return this.selectedHit }
  private drag: { x: number; y: number; moved: boolean } | null = null
  private pointer: { x: number; y: number } | null = null
  private hoverHit: HoverHit | null = null
  private drawer: DrawerHook | null = null
  private legend: LegendHook | null = null

  public constructor(deps: PanelDeps) {
    this.canvas = deps.canvas
    this.api = deps.api
    this.paint = deps.paint ?? paintFrame
    this.onStatus = deps.onStatus
    this.makeOwner = deps.owner
    this.language = deps.language ?? (() => 'en')
  }

  public get current(): PanelStatus {
    return this.status
  }

  public subscribe(listener: Listener): () => void {
    this.listeners.add(listener)
    return () => this.listeners.delete(listener)
  }

  public async open(sessionId: string): Promise<void> {
    const previous = this.sessionId
    const previousOwner = this.owner
    this.teardown()
    if (previous !== null && previous !== sessionId) this.sayClosed(previous, previousOwner)
    this.sessionId = sessionId
    const abort = new AbortController()
    this.abort = abort
    this.selectedHit = null
    this.publish({ kind: 'connecting', reason: null, actions: [], frame: this.frame, context: null,
      search: EMPTY_SEARCH, breadcrumbs: [], statusCode: null, view: 'source', proposals: [], requests: [] })
    this.observeTheme()
    const size = await this.measureViewport()
    this.setViewport(size.width, size.height)
    if (abort.signal.aborted) return
    this.owner = this.makeOwner && this.api.watchOwned && this.api.closeOwned ? this.makeOwner() : null
    this.sentLanguage = this.language()
    let stream = this.owner
      ? this.api.watchOwned!(sessionId, this.owner, this.sentLanguage)
      : this.api.watch(sessionId)
    this.stream = stream
    void (async () => {
      try {
        let received = false
        for (;;) {
          try {
            for await (const event of stream) {
              if (abort.signal.aborted) return
              received = true
              this.handle(event, abort.signal)
            }
            break
          } catch (error) {
            if (abort.signal.aborted || received || this.owner === null) throw error
            this.owner = null
            stream = this.api.watch(sessionId)
            this.stream = stream
          }
        }
        if (!abort.signal.aborted) this.publish({ kind: 'closed', reason: null, actions: [], frame: this.frame,
          context: this.status.context, search: this.status.search,
          breadcrumbs: this.status.breadcrumbs, statusCode: this.status.statusCode, view: this.status.view,
          proposals: this.status.proposals, requests: this.status.requests })
      } catch (error) {
        if (abort.signal.aborted) return
        this.publish({ kind: 'failed', reason: messageOf(error), actions: [], frame: this.frame,
          context: this.status.context, search: this.status.search,
          breadcrumbs: this.status.breadcrumbs, statusCode: this.status.statusCode, view: this.status.view,
          proposals: this.status.proposals, requests: this.status.requests })
      }
    })()
  }

  public async setLanguage(language: string): Promise<void> {
    if (this.sessionId === null || language === this.sentLanguage) return
    this.sentLanguage = language
    await this.send({ action: 'language', language })
  }

  public detach(): void {
    this.teardown()
    this.listeners.clear()
  }

  public close(): Promise<void> {
    const sessionId = this.sessionId
    const owner = this.owner
    this.teardown()
    this.publish({ kind: 'closed', reason: null, actions: [], frame: this.frame, context: this.status.context,
      search: EMPTY_SEARCH, breadcrumbs: this.status.breadcrumbs, statusCode: this.status.statusCode, view: this.status.view,
          proposals: this.status.proposals, requests: this.status.requests })
    return sessionId !== null ? this.sayClosed(sessionId, owner) : Promise.resolve()
  }

  private sayClosed(sessionId: string, owner: PanelOwner | null): Promise<void> {
    try {
      const closing = owner ? this.api.closeOwned?.(sessionId, owner) : this.api.close?.(sessionId)
      return Promise.resolve(closing).then(() => {}, () => {})
    } catch {
      return Promise.resolve()
    }
  }

  private teardown(): void {
    this.abort?.abort()
    this.abort = null
    this.sessionId = null
    this.owner = null
    const stream = this.stream
    this.stream = null
    try { stream?.end?.() } catch {}
    try { stream?.dispose?.() } catch {}
    this.observer?.disconnect()
    this.observer = null
  }

  public repaint(): void {
    if (this.frame) this.draw(this.frame)
  }

  private draw(frame: SceneFrame): void {
    const bounds = this.canvas.getBoundingClientRect()
    const pointer = this.pointer && bounds.width > 0 && bounds.height > 0
      ? [this.pointer.x * frame.viewport.width / bounds.width, this.pointer.y * frame.viewport.height / bounds.height] as const
      : null
    this.paint(withHover(frame, this.hoverHit, pointer), this.canvas)
  }

  public async send(action: PanelAction): Promise<PanelAck | null> {
    if (this.sessionId === null) return null
    const binding = this.abort
    const result = await this.api.action(this.sessionId, action)
    if (binding !== this.abort || binding?.signal.aborted) return null
    return ackOf(result)
  }

  private updateCamera(camera: SceneFrame['camera']): void {
    if (!this.frame) return
    const frame = { ...this.frame, camera }
    this.frame = frame
    this.publish({ ...this.status, frame })
    this.draw(frame)
    void this.send({ action: 'camera', ...camera })
  }

  public setViewport(width: number, height: number): void {
    if (width <= 0 || height <= 0) return
    this.viewport = { width, height }
    if (this.status.kind === 'live') void this.send({ action: 'viewport', viewport: { width, height } })
  }

  private viewport: { width: number; height: number } | null = null

  public async measureViewport(): Promise<{ width: number; height: number }> {
    const rect = this.canvas.getBoundingClientRect()
    return { width: Math.max(1, Math.round(rect.width)), height: Math.max(1, Math.round(rect.height)) }
  }

  private world(x: number, y: number): [number, number] | null {
    if (!this.frame || !this.viewport) return null
    return screenToWorld(viewTransform(this.frame, this.viewport.width, this.viewport.height), x, y)
  }

  public pointerDown(x: number, y: number): void {
    this.pointer = { x, y }
    if (this.status.search.active) return
    this.drag = { x, y, moved: false }
  }

  public pointerMove(x: number, y: number): void {
    this.pointer = { x, y }
    if (!this.drag || !this.frame || !this.viewport) return
    const dx = x - this.drag.x
    const dy = y - this.drag.y
    if (Math.abs(dx) < 2 && Math.abs(dy) < 2) return
    this.drag = { x, y, moved: true }
    const zoom = this.frame.camera.zoom
    this.updateCamera({
      x: this.frame.camera.x - dx / zoom,
      y: this.frame.camera.y - dy / zoom,
      zoom,
    })
  }

  public pointerUp(x: number, y: number): void {
    const gesture = this.drag
    this.drag = null
    if (!gesture || gesture.moved || this.status.search.active) return
    const world = this.world(x, y)
    if (world) void this.send({ action: 'select', at: world })
  }

  public doubleClick(x: number, y: number): void {
    if (this.status.search.active) return
    const world = this.world(x, y)
    if (world) void this.send({ action: 'drill-in', at: world })
  }

  public hover(x: number, y: number): void {
    this.pointer = { x, y }
    if (!this.frame) return
    const world = cssToWorld(this.frame, this.canvas.getBoundingClientRect(), x, y)
    const hit = world ? hoverAt(this.frame, world) : null
    const followsPointer = showsPopup(this.frame, hit)
    if (sameHover(hit, this.hoverHit) && !followsPointer) return
    this.hoverHit = hit
    this.draw(this.frame)
  }

  public hoverEnd(): void {
    if (this.hoverHit === null) return
    this.hoverHit = null
    if (this.frame) this.draw(this.frame)
  }

  public wheel(x: number, y: number, delta: number, modifiers: KeyModifiers = {}, horizontalDelta = 0): void {
    this.pointer = { x, y }
    if (!this.frame || !this.viewport || this.status.search.active) return
    if (modifiers.control) {
      this.zoomAt(ZOOM_STEP ** delta, x, y)
    } else {
      const amount = horizontalDelta !== 0 ? horizontalDelta : delta
      if (amount !== 0) this.scroll(-amount, horizontalDelta !== 0 || modifiers.shift === true)
    }
  }

  public zoomAt(factor: number, x: number, y: number): void {
    if (!this.frame) return
    const old = this.frame.camera.zoom
    const zoom = Math.min(ZOOM_MAX, Math.max(ZOOM_MIN, old * factor))
    this.updateCamera({
      x: this.frame.camera.x + x / old - x / zoom,
      y: this.frame.camera.y + y / old - y / zoom,
      zoom,
    })
  }

  private sceneBounds(): { x0: number; y0: number; x1: number; y1: number } | null {
    if (!this.frame) return null
    let x0 = Infinity; let y0 = Infinity; let x1 = -Infinity; let y1 = -Infinity
    for (const item of this.frame.world) {
      const b = item.bounds
      if (!b) continue
      x0 = Math.min(x0, b.x); y0 = Math.min(y0, b.y)
      x1 = Math.max(x1, b.x + b.w); y1 = Math.max(y1, b.y + b.h)
    }
    return Number.isFinite(x0) ? { x0, y0, x1, y1 } : null
  }

  public scroll(amount: number, horizontal: boolean): void {
    if (!this.frame || !this.viewport) return
    const camera = this.frame.camera
    const bounds = this.sceneBounds()
    const zoom = camera.zoom
    const minimum = bounds ? (horizontal ? bounds.x0 : bounds.y0) : 0
    const span = bounds ? (horizontal ? bounds.x1 : bounds.y1) : (horizontal ? 800 : 600)
    const visible = (horizontal ? this.viewport.width : this.viewport.height) / zoom
    const maximum = Math.max(minimum, span - visible)
    const current = horizontal ? camera.x : camera.y
    const value = Math.max(minimum, Math.min(maximum, current + amount * SCROLL_UNIT / zoom))
    this.updateCamera({ ...camera, ...(horizontal ? { x: value } : { y: value }) })
  }

  public fit(): void {
    if (!this.frame || !this.viewport) return
    const bounds = this.sceneBounds()
    if (!bounds) return
    const { width: vw, height: vh } = this.viewport
    const margin = SCENE_BOUNDS_INSET + SCENE_OUTER_MARGIN
    const x0 = Math.min(0, bounds.x0 - margin)
    const y0 = Math.min(0, bounds.y0 - margin)
    const w = Math.max(1, bounds.x1 + margin - x0)
    const h = Math.max(1, bounds.y1 + margin - y0)
    const usable = Math.max(1, vh)
    const zoom = Math.min(ZOOM_MAX, Math.max(ZOOM_MIN, Math.min(vw / w, usable / h, 1)))
    this.updateCamera({
      x: x0 + w / 2 - vw / (2 * zoom),
      y: y0 + h / 2 - usable / (2 * zoom),
      zoom,
    })
  }

  public hit(x: number, y: number): string | null {
    const world = this.world(x, y)
    return world && this.frame ? hitTest(this.frame, world) : null
  }

  private hasBack(): boolean {
    return this.status.breadcrumbs.length > 1
  }

  public async back(): Promise<PanelAck | null> {
    return this.send({ action: 'back' })
  }

  public async backTo(target: string | null): Promise<PanelAck | null> {
    return this.send({ action: 'back', target })
  }

  public async setDeclutter(mode: DeclutterMode): Promise<void> {
    const ack = await this.send({ action: 'declutter', mode })
    if (!ack?.accepted) return
    this.mode = mode
    try {
      await this.send({ action: 'save' })
    } catch {
    }
  }

  public async namespaceTree(): Promise<PanelAck | null> {
    return this.send({ action: 'namespace-tree' })
  }

  public setDrawer(drawer: DrawerHook | null): void {
    this.drawer = drawer
  }

  public setLegend(legend: LegendHook | null): void {
    this.legend = legend
  }

  public escape(): void {
    if (this.status.search.active) {
      this.closeSearch()
    } else if (this.legend?.isOpen()) {
      this.legend.close()
    } else if (this.drawer?.open()) {
      this.drawer.close()
    } else if (this.hasBack()) {
      void this.back()
    } else {
      void this.send({ action: 'select', at: null })
    }
  }

  public openSearch(): void {
    this.publish({ ...this.status, search: { ...this.status.search, active: true } })
  }

  public closeSearch(): void {
    this.drag = null
    this.publish({ ...this.status, search: { ...this.status.search, active: false } })
  }

  public setSearchQuery(query: string): void {
    this.publish({ ...this.status, search: { ...this.status.search, active: true, query, selection: -1 } })
    void this.send({ action: 'search', query })
  }

  public moveSearch(delta: number): void {
    const search = this.status.search
    if (!search.results.length) return
    const selection = Math.max(0, Math.min(search.results.length - 1, search.selection + delta))
    this.publish({ ...this.status, search: { ...search, selection } })
  }

  public activateSearch(): void {
    const result = this.status.search.results[this.status.search.selection]
    if (result) void this.send({ action: 'locate', id: result.id })
  }

  public key(key: string, modifiers: KeyModifiers = {}): boolean {
    const k = key.toLowerCase()
    const control = modifiers.control === true
    if (this.status.search.active) {
      if (k === 'escape') this.closeSearch()
      else if (k === 'arrowdown') this.moveSearch(1)
      else if (k === 'arrowup') this.moveSearch(-1)
      else if (k === 'enter') this.activateSearch()
      return true
    }
    if (control && k === 'f') { this.openSearch(); return true }
    if (control && (k === '0' || k === 'kp0')) { this.fit(); return true }
    if (control && (k === '=' || k === '+' || k === 'kp+')) { this.zoomBy(ZOOM_STEP); return true }
    if (control && (k === '-' || k === '_' || k === 'kp-')) { this.zoomBy(1 / ZOOM_STEP); return true }
    if (control) return false
    if (k === 'p') { void this.namespaceTree(); return true }
    if (k === 'r') { void this.send({ action: 'reload' }); return true }
    if (k === 'escape') { this.escape(); return true }
    if (k === 'arrowleft') { this.scroll(-2, true); return true }
    if (k === 'arrowright') { this.scroll(2, true); return true }
    if (k === 'arrowup') { this.scroll(-2, false); return true }
    if (k === 'arrowdown') { this.scroll(2, false); return true }
    return false
  }

  private zoomBy(factor: number): void {
    const anchor = this.pointer ?? (this.viewport ? { x: this.viewport.width / 2, y: this.viewport.height / 2 } : { x: 0, y: 0 })
    this.zoomAt(factor, anchor.x, anchor.y)
  }

  private handle(event: PanelEvent, signal: AbortSignal): void {
    switch (event.kind) {
      case 'ready':
        this.publish({ kind: 'live', reason: null, actions: event.actions, frame: this.frame,
          context: this.status.context, search: this.status.search,
          breadcrumbs: this.status.breadcrumbs, statusCode: this.status.statusCode, view: this.status.view,
          proposals: this.status.proposals, requests: this.status.requests })
        if (this.viewport) void this.send({ action: 'viewport', viewport: this.viewport })
        return
      case 'state':
        this.mode = event.state.declutter?.replace(/^:/, '') ?? 'full'
        this.selectedHit = event.state.selected
        this.publish({ ...this.status, breadcrumbs: event.breadcrumbs, statusCode: event.status,
          view: event.state.proposalId ? 'proposal' : 'source', proposals: event.proposals })
        return
      case 'frame':
        this.frame = event.frame
        this.publish({ kind: 'live', reason: null, actions: this.status.actions, frame: event.frame,
          context: this.status.context ?? { kind: 'real', edn: '{:context :real}' }, search: this.status.search,
          breadcrumbs: this.status.breadcrumbs, statusCode: this.status.statusCode, view: this.status.view,
          proposals: this.status.proposals, requests: this.status.requests })
        this.draw(event.frame)
        void this.send({ action: 'save' })
        return
      case 'context':
        this.publish({ ...this.status, context: event.context })
        return
      case 'request-status':
        this.publish({ ...this.status, requests: event.requests })
        return
      case 'search': {
        const search = this.status.search
        const selection = Math.max(-1, Math.min(search.selection, event.results.length - 1))
        this.publish({ ...this.status, search: { ...search, query: event.query, results: event.results, selection } })
        return
      }
      case 'refused':
        this.publish({ kind: 'refused', reason: event.reason, actions: [], frame: this.frame,
          context: this.status.context, search: this.status.search,
          breadcrumbs: this.status.breadcrumbs, statusCode: this.status.statusCode, view: this.status.view,
          proposals: this.status.proposals, requests: this.status.requests })
        return
      case 'failed':
        this.publish({ kind: 'failed', reason: event.reason, actions: [], frame: this.frame,
          context: this.status.context, search: this.status.search,
          breadcrumbs: this.status.breadcrumbs, statusCode: this.status.statusCode, view: this.status.view,
          proposals: this.status.proposals, requests: this.status.requests })
        return
      default:
        if (signal.aborted) return
        return
    }
  }

  private publish(status: PanelStatus): void {
    this.status = status
    this.onStatus?.(status)
    for (const listener of this.listeners) listener()
  }

  private observeTheme(): void {
    const body = this.canvas.ownerDocument?.body
    const MutationObserverCtor = this.canvas.ownerDocument?.defaultView?.MutationObserver
    if (!body || !MutationObserverCtor) return
    this.observer?.disconnect()
    this.observer = new MutationObserverCtor(() => this.repaint())
    this.observer.observe(body, { attributes: true, attributeFilter: ['data-ds-dark-theme', 'data-theme'] })
  }
}

function messageOf(error: unknown): string {
  return error instanceof Error ? error.message : String(error)
}

function ackOf(result: unknown): PanelAck | null {
  if (result && typeof result === 'object' && 'ok' in result) {
    const envelope = result as { ok: boolean; value?: unknown; error?: { message?: string } }
    if (!envelope.ok) return { accepted: false, reason: envelope.error?.message ?? null }
    return (envelope.value as PanelAck | undefined) ?? null
  }
  return (result as PanelAck | null) ?? null
}
