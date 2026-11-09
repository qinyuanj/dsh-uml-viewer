/**
 * The panel's view controller: it owns the browser side of the session-bound
 * panel stream. Everything visual goes through the injected `paint` function so
 * the controller itself can be exercised without a browser canvas.
 *
 * The interaction boundary is ADR 0013's: drill-in/back/P, select and search go
 * through the host to the resident (`drill-in`, `back`, `namespace-tree`,
 * `select`, `search`, `locate`); pan, zoom, fit and hover are local camera or
 * feedback work. The local camera math mirrors `scene::events` (on_wheel,
 * on_scroll, zoom_at, fit) so the panel behaves like the egui window.
 */
import type { DiscussionContext, PanelAck, PanelAction, PanelEvent, SceneFrame, SearchResult } from '@dsh-uml-viewer/contracts'
import { hitTest, paintFrame } from './paint.js'
import { screenToWorld, viewTransform } from './fit.js'

/** A Remote stream handle: async-iterable, with the carrier's own teardown. */
export interface PanelStream extends AsyncIterable<PanelEvent> {
  dispose?(): void
  end?(): void
}

/** What the panel hands to the host side of the Remote. */
export interface PanelRemoteApi {
  watch(sessionId: string): PanelStream
  action(sessionId: string, action: PanelAction): Promise<unknown>
}

/** The search overlay: the egui window's Ctrl+F box and its result list. */
export interface SearchState {
  active: boolean
  query: string
  results: SearchResult[]
  /** The explicitly selected result (Down/Up), Enter locates it. */
  selection: number
}

export interface PanelStatus {
  kind: 'connecting' | 'live' | 'refused' | 'failed' | 'closed'
  reason: string | null
  actions: readonly string[]
  frame: SceneFrame | null
  /** The discussion context carried by this session's stream. */
  context: DiscussionContext | null
  search: SearchState
}

export interface PanelDeps {
  canvas: HTMLCanvasElement
  api: PanelRemoteApi
  /** Injected for tests; defaults to the Canvas 2D painter. */
  paint?: (frame: SceneFrame, canvas: HTMLCanvasElement) => void
  onStatus?: (status: PanelStatus) => void
}

export interface KeyModifiers { control?: boolean; shift?: boolean }

type Listener = () => void

/** `scene::events`: one wheel notch, the pan unit and the chrome top strip. */
export const ZOOM_STEP = 1.1
const SCROLL_UNIT = 48
const CHROME_INSET = 36
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
  private frame: SceneFrame | null = null
  private status: PanelStatus = { kind: 'connecting', reason: null, actions: [], frame: null, context: null, search: EMPTY_SEARCH }
  private observer: MutationObserver | null = null
  private readonly listeners = new Set<Listener>()
  /** Last mode reported by the resident's existing `state` event. */
  private mode = 'full'
  public get declutterMode(): string { return this.mode }
  // Pending pointer gesture, in screen pixels.
  private drag: { x: number; y: number; moved: boolean } | null = null
  /** Last pointer position: the zoom anchor, like `Dims.pointer`. */
  private pointer: { x: number; y: number } | null = null
  private hovered: string | null = null

  public constructor(deps: PanelDeps) {
    this.canvas = deps.canvas
    this.api = deps.api
    this.paint = deps.paint ?? paintFrame
    this.onStatus = deps.onStatus
  }

  public get current(): PanelStatus {
    return this.status
  }

  public subscribe(listener: Listener): () => void {
    this.listeners.add(listener)
    return () => this.listeners.delete(listener)
  }

  /** Open the stream bound to this session; a second open replaces the first. */
  public async open(sessionId: string): Promise<void> {
    this.close()
    this.sessionId = sessionId
    const abort = new AbortController()
    this.abort = abort
    this.publish({ kind: 'connecting', reason: null, actions: [], frame: this.frame, context: null, search: EMPTY_SEARCH })
    this.observeTheme()
    const size = await this.measureViewport()
    this.setViewport(size.width, size.height)
    const stream = this.api.watch(sessionId)
    this.stream = stream
    void (async () => {
      try {
        for await (const event of stream) {
          if (abort.signal.aborted) return
          this.handle(event, abort.signal)
        }
        if (!abort.signal.aborted) this.publish({ kind: 'closed', reason: null, actions: [], frame: this.frame, context: this.status.context, search: this.status.search })
      } catch (error) {
        if (abort.signal.aborted) return
        this.publish({ kind: 'failed', reason: messageOf(error), actions: [], frame: this.frame, context: this.status.context, search: this.status.search })
      }
    })()
  }

  public close(): void {
    this.abort?.abort()
    this.abort = null
    this.sessionId = null
    const stream = this.stream
    this.stream = null
    // The Remote handle owns the carrier subscription; ending it is what tells
    // the host this panel closed (and releases the session's resident).
    try { stream?.end?.() } catch { /* already closed */ }
    try { stream?.dispose?.() } catch { /* already closed */ }
    this.observer?.disconnect()
    this.observer = null
  }

  /** Repaint the last frame, e.g. after the host theme flips light/dark. */
  public repaint(): void {
    if (this.frame) this.paint(this.frame, this.canvas)
  }

  public async send(action: PanelAction): Promise<PanelAck | null> {
    if (this.sessionId === null) return null
    const binding = this.abort
    const result = await this.api.action(this.sessionId, action)
    // A continuation must not act on a closed or replacement panel binding,
    // even when the replacement opens the same session id.
    if (binding !== this.abort || binding?.signal.aborted) return null
    return ackOf(result)
  }

  /** Apply local camera work now; the resident only acknowledges and saves it. */
  private updateCamera(camera: SceneFrame['camera']): void {
    if (!this.frame) return
    const frame = { ...this.frame, camera }
    this.frame = frame
    this.publish({ ...this.status, frame })
    this.paint(frame, this.canvas)
    void this.send({ action: 'camera', ...camera })
  }

  /**
   * Tell the host the panel's canvas size so it lays the frame out for this
   * viewport. The size is in CSS pixels; the painter renders at device pixels.
   */
  public setViewport(width: number, height: number): void {
    if (width <= 0 || height <= 0) return
    this.viewport = { width, height }
    void this.send({ action: 'viewport', viewport: { width, height } })
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

  /** Pointer down starts a pan gesture; a click without motion selects. */
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

  /** Double-click: the back hotspot goes back, otherwise drill into what is there. */
  public doubleClick(x: number, y: number): void {
    if (this.status.search.active) return
    if (this.hasBack() && x < 320 && y < 44) {
      void this.back()
      return
    }
    const world = this.world(x, y)
    // The resident's own hit test decides whether this is drillable; a class
    // card (double-click on a leaf class) is #25's surface.
    if (world) void this.send({ action: 'drill-in', at: world })
  }

  /** Hover is local feedback: no round trip, just a repaint of the hovered box. */
  public hover(x: number, y: number): void {
    this.pointer = { x, y }
    const world = this.world(x, y)
    const id = world ? hitTest(this.frame!, world) : null
    if (id === this.hovered) return
    this.hovered = id
    if (!this.frame) return
    const frame: SceneFrame = {
      ...this.frame,
      world: this.frame.world.map(item => ('hovered' in item ? { ...item, hovered: item.id === id } : item)),
    }
    this.frame = frame
    this.paint(frame, this.canvas)
  }

  /** Wheel: plain pans (shift/horizontal pans sideways), Ctrl zooms at the cursor. */
  public wheel(x: number, y: number, delta: number, modifiers: KeyModifiers = {}, horizontalDelta = 0): void {
    this.pointer = { x, y }
    if (!this.frame || !this.viewport || this.status.search.active) return
    if (modifiers.control) {
      this.zoomAt(ZOOM_STEP ** delta, x, y)
    } else {
      // Match scene::events::on_wheel: a native horizontal axis takes
      // precedence; Shift maps a vertical-only wheel onto that same axis.
      const amount = horizontalDelta !== 0 ? horizontalDelta : delta
      if (amount !== 0) this.scroll(-amount, horizontalDelta !== 0 || modifiers.shift === true)
    }
  }

  /** Zoom about a screen point, keeping the world point under it fixed. */
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

  /** The scene extent, the same `size`/`min-x` bounds `scene::events` scrolls in. */
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

  /** `on_scroll`: 48/zoom world units per notch, clamped to the scene. */
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

  /** Ctrl+0: fit the whole scene into the usable canvas, like `scene::events::fit`. */
  public fit(): void {
    if (!this.frame || !this.viewport) return
    const bounds = this.sceneBounds()
    if (!bounds) return
    const { width: vw, height: vh } = this.viewport
    const w = Math.max(1, bounds.x1 - bounds.x0)
    const h = Math.max(1, bounds.y1 - bounds.y0)
    const usable = Math.max(1, vh - CHROME_INSET)
    const zoom = Math.min(ZOOM_MAX, Math.max(ZOOM_MIN, Math.min(vw / w, usable / h, 1)))
    this.updateCamera({
      x: bounds.x0 + w / 2 - vw / (2 * zoom),
      y: bounds.y0 + h / 2 - usable / (2 * zoom) - CHROME_INSET / zoom,
      zoom,
    })
  }

  /** The class or package under a screen point, from the frame's own bounds. */
  public hit(x: number, y: number): string | null {
    const world = this.world(x, y)
    return world && this.frame ? hitTest(this.frame, world) : null
  }

  private hasBack(): boolean {
    return this.frame?.chrome.some(item => item.kind === 'back') ?? false
  }

  public async back(): Promise<PanelAck | null> {
    return this.send({ action: 'back' })
  }

  /** **P**: return to the source diagram's namespace tree. */
  public async namespaceTree(): Promise<PanelAck | null> {
    return this.send({ action: 'namespace-tree' })
  }

  /** Escape: close search, else back one level, else clear the selection. */
  public escape(): void {
    if (this.status.search.active) {
      this.closeSearch()
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

  /** Typing or pasting into the search box asks the resident; no local matching. */
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

  /** Enter: locate the explicitly selected result (the search box stays open). */
  public activateSearch(): void {
    const result = this.status.search.results[this.status.search.selection]
    if (result) void this.send({ action: 'locate', id: result.id })
  }

  /**
   * One key, as the egui window maps it. Returns true when the key was consumed;
   * an open search box captures every canvas shortcut and pointer gesture.
   */
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
        this.publish({ kind: 'live', reason: null, actions: event.actions, frame: this.frame, context: this.status.context, search: this.status.search })
        // The panel names its viewport before the first frame can be laid out.
        if (this.viewport) void this.send({ action: 'viewport', viewport: this.viewport })
        return
      case 'state':
        this.mode = event.state.declutter?.replace(/^:/, '') ?? 'full'
        this.publish(this.status)
        return
      case 'frame':
        this.frame = event.frame
        this.publish({ kind: 'live', reason: null, actions: this.status.actions, frame: event.frame, context: this.status.context ?? { kind: 'real', edn: '{:context :real}' }, search: this.status.search })
        this.paint(event.frame, this.canvas)
        // Rebuilds and display can change the mode. Read actual resident state,
        // including a restored mode, instead of advancing a local click count.
        void this.send({ action: 'save' })
        return
      case 'context':
        this.publish({ ...this.status, context: event.context })
        return
      case 'search': {
        const search = this.status.search
        const selection = Math.max(-1, Math.min(search.selection, event.results.length - 1))
        this.publish({ ...this.status, search: { ...search, query: event.query, results: event.results, selection } })
        return
      }
      case 'refused':
        this.publish({ kind: 'refused', reason: event.reason, actions: [], frame: this.frame, context: this.status.context, search: this.status.search })
        return
      case 'failed':
        this.publish({ kind: 'failed', reason: event.reason, actions: [], frame: this.frame, context: this.status.context, search: this.status.search })
        return
      default:
        // state events are the host's own persistence traffic, not panel UI.
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

/** Unary Remote calls return `{ok, value|error}`; a stub may return the ack itself. */
function ackOf(result: unknown): PanelAck | null {
  if (result && typeof result === 'object' && 'ok' in result) {
    const envelope = result as { ok: boolean; value?: unknown; error?: { message?: string } }
    if (!envelope.ok) return { accepted: false, reason: envelope.error?.message ?? 'panel action failed' }
    return (envelope.value as PanelAck | undefined) ?? null
  }
  return (result as PanelAck | null) ?? null
}
