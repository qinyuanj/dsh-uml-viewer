/**
 * The panel's view controller: it owns the browser side of the session-bound
 * panel stream. Everything visual goes through the injected `paint` function so
 * the controller itself can be exercised without a browser canvas.
 *
 * The controller never computes scene geometry: frames arrive already laid out
 * from the resident, and pointer input is forwarded to the host as panel
 * actions (`viewport`, `camera`, `select`). Only pan/zoom drag deltas are local,
 * because they are a camera update, not a layout decision.
 */
import type { PanelAck, PanelAction, PanelEvent, SceneFrame } from '@dsh-uml-viewer/contracts'
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

export interface PanelStatus {
  kind: 'connecting' | 'live' | 'refused' | 'failed' | 'closed'
  reason: string | null
  actions: readonly string[]
  frame: SceneFrame | null
}

export interface PanelDeps {
  canvas: HTMLCanvasElement
  api: PanelRemoteApi
  /** Injected for tests; defaults to the Canvas 2D painter. */
  paint?: (frame: SceneFrame, canvas: HTMLCanvasElement) => void
  onStatus?: (status: PanelStatus) => void
}

type Listener = () => void

export class PanelController {
  private readonly canvas: HTMLCanvasElement
  private readonly api: PanelRemoteApi
  private readonly paint: (frame: SceneFrame, canvas: HTMLCanvasElement) => void
  private readonly onStatus: ((status: PanelStatus) => void) | undefined
  private abort: AbortController | null = null
  private stream: PanelStream | null = null
  private sessionId: string | null = null
  private frame: SceneFrame | null = null
  private status: PanelStatus = { kind: 'connecting', reason: null, actions: [], frame: null }
  private observer: MutationObserver | null = null
  private readonly listeners = new Set<Listener>()
  // Pending pointer gesture, in screen pixels.
  private drag: { x: number; y: number; moved: boolean } | null = null

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
    this.publish({ kind: 'connecting', reason: null, actions: [], frame: this.frame })
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
        if (!abort.signal.aborted) this.publish({ kind: 'closed', reason: null, actions: [], frame: this.frame })
      } catch (error) {
        if (abort.signal.aborted) return
        this.publish({ kind: 'failed', reason: messageOf(error), actions: [], frame: this.frame })
      }
    })()
  }

  public close(): void {
    this.abort?.abort()
    this.abort = null
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
    const result = await this.api.action(this.sessionId, action)
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

  /** Pointer down starts a pan gesture; a click without motion selects. */
  public pointerDown(x: number, y: number): void {
    this.drag = { x, y, moved: false }
  }

  public pointerMove(x: number, y: number): void {
    if (!this.drag || !this.frame || !this.viewport) return
    const dx = x - this.drag.x
    const dy = y - this.drag.y
    if (Math.abs(dx) < 2 && Math.abs(dy) < 2) return
    this.drag = { x, y, moved: true }
    const view = viewTransform(this.frame, this.viewport.width, this.viewport.height)
    this.updateCamera({
      x: this.frame.camera.x - dx / view.zoom,
      y: this.frame.camera.y - dy / view.zoom,
      zoom: this.frame.camera.zoom,
    })
  }

  public pointerUp(x: number, y: number): void {
    const gesture = this.drag
    this.drag = null
    if (!gesture || gesture.moved || !this.frame || !this.viewport) return
    const view = viewTransform(this.frame, this.viewport.width, this.viewport.height)
    const world = screenToWorld(view, x, y)
    void this.send({ action: 'select', at: [world[0], world[1]] })
  }

  /** Zoom about a screen point; the resident saves the resulting top-left camera. */
  public wheel(x: number, y: number, delta: number): void {
    if (!this.frame || !this.viewport) return
    const zoom = Math.min(8, Math.max(this.frame.style.lodZoom.frames / 2, this.frame.camera.zoom * (delta > 0 ? 0.9 : 1.1)))
    const view = viewTransform(this.frame, this.viewport.width, this.viewport.height)
    const [wx, wy] = screenToWorld(view, x, y)
    this.updateCamera({
      x: wx - x / zoom,
      y: wy - y / zoom,
      zoom,
    })
  }

  /** The class or package under a screen point, from the frame's own bounds. */
  public hit(x: number, y: number): string | null {
    if (!this.frame || !this.viewport) return null
    const view = viewTransform(this.frame, this.viewport.width, this.viewport.height)
    return hitTest(this.frame, screenToWorld(view, x, y))
  }

  private handle(event: PanelEvent, signal: AbortSignal): void {
    switch (event.kind) {
      case 'ready':
        this.publish({ kind: 'live', reason: null, actions: event.actions, frame: this.frame })
        // The panel names its viewport before the first frame can be laid out.
        if (this.viewport) void this.send({ action: 'viewport', viewport: this.viewport })
        return
      case 'frame':
        this.frame = event.frame
        this.publish({ kind: 'live', reason: null, actions: this.status.actions, frame: event.frame })
        this.paint(event.frame, this.canvas)
        return
      case 'refused':
        this.publish({ kind: 'refused', reason: event.reason, actions: [], frame: this.frame })
        return
      case 'failed':
        this.publish({ kind: 'failed', reason: event.reason, actions: [], frame: this.frame })
        return
      default:
        // context/state/search events belong to later tickets (#22–#27).
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
