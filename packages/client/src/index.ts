/**
 * The plugin's browser half: one right-sidebar tab bound to the session the
 * sidebar belongs to. The tab's lifecycle is the host's (ADR 0015): the
 * full-width form is the right sidebar's fullscreen, closing the tab is the
 * explicit close (D13) through the tab-kind close handler, and the tab's
 * refresh command is `reload`. Collapsing, presentation changes and plugin
 * unload only unmount the body, which is a withdrawal.
 *
 * React comes from the host's platform module table (ADR 0013, D1): this
 * bundle requires it, never embeds it, and imports no
 * `dsh-client-ui-primitives`. The chrome is plain markup styled from host
 * theme tokens, which is why the panel reads tokens for its Canvas 2D colors
 * instead of relying on primitive components.
 */
import { createElement, useCallback, useRef, useState } from 'react'
import { OverlayController } from './overlays.js'
import { Overlays, OVERLAY_STYLE } from './overlay-view.js'
import { UML_VIEWER_REMOTE, type PanelOwner, type SceneFrame } from '@dsh-uml-viewer/contracts'
import { FONT_FAMILY, loadSarasa } from './font.js'
import { PanelController, type PanelRemoteApi, type PanelStatus } from './panel.js'
import { paintFrame } from './paint.js'
import { cssToWorld } from './fit.js'
import { DECLUTTER_STYLE, mountDeclutterControl } from './declutter.js'
import { REQUEST_MENU_STYLE, mountQualityMenu } from './requests.js'
import { panelText, type PanelText } from './locale.js'
import { PanelViews, registerToolViews } from './tool-views.js'

export const TAB_ID = 'dsh-uml-viewer'
export const TAB_KIND = 'dsh-uml-viewer'
export const GUIDE_ORDER = 30
const REMOTE_NAMESPACE = 'umlViewer'

const STYLE = `${DECLUTTER_STYLE}${REQUEST_MENU_STYLE}
.dsh-uml-viewer-panel { position: relative; display: flex; flex-direction: column; min-height: 0; height: 100%; width: 100%; }
.dsh-uml-viewer-canvas-host { position: absolute; inset: 0; display: flex; flex-direction: column; min-height: 0; }
.dsh-uml-viewer-panel > .dsh-uml-overlay-root { top: var(--dsh-uml-toolbar-height, 0px); }
.dsh-uml-viewer-toolbar {
  display: flex; flex-wrap: wrap; align-items: center; gap: 4px; flex: 0 0 auto; min-height: 36px; padding: 4px 8px;
  border-bottom: 1px solid var(--dsw-alias-border-l2);
  background: var(--dsw-alias-bg-layer-2); color: var(--dsw-alias-label-primary);
  font: 12px/1.4 var(--dsw-font-family, system-ui);
}
.dsh-uml-viewer-toolbar .dsh-uml-viewer-declutter { position: static; }
.dsh-uml-viewer-toolbar button {
  appearance: none; border: 1px solid transparent; border-radius: 4px; padding: 3px 8px;
  background: transparent; color: var(--dsw-alias-label-secondary);
  font: inherit; cursor: pointer; white-space: nowrap;
}
.dsh-uml-viewer-toolbar button:hover { background: var(--dsw-alias-interactive-bg-hover); color: var(--dsw-alias-label-primary); }
.dsh-uml-viewer-toolbar button:active { background: var(--dsw-alias-interactive-bg-active); }
.dsh-uml-viewer-toolbar button:focus-visible,
.dsh-uml-viewer-search input:focus-visible {
  outline: var(--dsw-focus-ring-width, 2px) solid var(--dsw-focus-ring-color, var(--dsw-alias-state-business-primary));
  outline-offset: 1px;
}
.dsh-uml-viewer-toolbar button:disabled { opacity: 0.45; cursor: default; background: transparent; }
.dsh-uml-viewer-spacer { flex: 1 1 auto; }
.dsh-uml-viewer-search { position: relative; flex: 0 1 260px; min-width: 120px; }
.dsh-uml-viewer-search input {
  box-sizing: border-box; width: 100%; border: 1px solid var(--dsw-alias-border-l2); border-radius: 4px;
  padding: 3px 8px; background: var(--dsw-alias-bg-base); color: var(--dsw-alias-label-primary); font: inherit;
}
.dsh-uml-viewer-results {
  position: absolute; z-index: 20; top: calc(100% + 4px); left: 0; right: 0; max-height: 320px; overflow: auto;
  margin: 0; padding: 4px; list-style: none; border: 1px solid var(--dsw-alias-border-l2); border-radius: 6px;
  background: var(--dsw-alias-bg-overlay); box-shadow: var(--dsw-shadow-lv2, 0 4px 16px rgb(0 0 0 / 20%));
}
.dsh-uml-viewer-results[hidden] { display: none; }
.dsh-uml-viewer-results li { margin: 0; padding: 0; }
.dsh-uml-viewer-results button {
  display: block; width: 100%; border: 0; border-radius: 3px; padding: 5px 8px; text-align: left;
  background: transparent; color: var(--dsw-alias-label-primary); font: inherit; cursor: pointer;
  overflow-wrap: anywhere;
}
.dsh-uml-viewer-results button[aria-selected="true"] { background: var(--dsw-alias-interactive-bg-active); }
.dsh-uml-viewer-results button:hover { background: var(--dsw-alias-interactive-bg-hover); }
.dsh-uml-viewer-empty { padding: 6px 8px; color: var(--dsw-alias-label-tertiary); }
.dsh-uml-viewer-panel canvas { display: block; flex: 1 1 auto; min-height: 0; width: 100%; height: 100%; outline: none; }
.dsh-uml-viewer-summary {
  position: absolute; width: 1px; height: 1px; padding: 0; margin: -1px;
  overflow: hidden; clip-path: inset(50%); white-space: nowrap; border: 0;
}
.dsh-uml-viewer-status {
  position: absolute; left: 12px; right: 12px; bottom: 8px;
  font: 12px/1.4 var(--dsw-font-family, system-ui); color: var(--dsw-alias-state-error-primary);
  pointer-events: none; white-space: pre-wrap;
}
`

export interface SlotRegistration {
  name: string
  key?: string
  id?: string
  locale?: string
  order?: number
  label?: () => string
  title?: () => string
  kind?: string
  guide?: readonly Record<string, unknown>[]
}

export interface SlotsFace {
  register(descriptor: SlotRegistration, component: (props: Record<string, unknown>) => unknown): () => void
  inject(name: string, callback: () => (() => void) | undefined): unknown
}

export interface SidebarTabsFace {
  register(tab: Record<string, unknown>): () => void
}

/** The page commands a tab body binds; the host's tab refresh calls `refresh`. */
export interface TabCommands {
  refresh?: () => void
}

/** What a tab body reads from the host's `useTabInfo` hook. */
interface TabInfo {
  tab?: { id?: string; actions?: { bindCommands?(commands: TabCommands): () => void } }
}

/** `ctx.sidebarRight`: the close hook, and the navigation 「在面板中查看」 uses (#54). */
export interface SidebarRightFace {
  registerCloseHandler?(kind: string, handler: (sessionId: string, tab: { id: string }) => void): () => void
  openTab?(kind: string): void
}

/** The client services this bundle uses; unknown services stay `undefined`. */
export interface ClientContext {
  readonly slots: SlotsFace
  readonly effect: (callback: () => (() => void) | void, label?: string) => void
  readonly inject: (names: readonly string[], callback: (ctx: ClientContext) => void) => unknown
  readonly get: (name: string) => unknown
}

export interface ClientPanelProps {
  sessionId?: string
  [key: string]: unknown
}

/** The mountable Remote contribution and its resolved panel API. */
const mountedRemotes = new WeakMap<ClientContext, Promise<PanelRemoteApi | null>>()

async function mountRemote(ctx: ClientContext): Promise<PanelRemoteApi | null> {
  const previous = mountedRemotes.get(ctx)
  if (previous) return previous
  const mountedRemote = (async () => {
    const remote = ctx.get('remote') as
      | { $mount(contribution: unknown): Promise<unknown>; [name: string]: unknown }
      | undefined
    if (!remote) return null
    try {
      await remote.$mount(UML_VIEWER_REMOTE)
    } catch {
      // A host without the Remote gateway simply has no panel; the tools still work.
      return null
    }
    const service = ctx.get(`remote.${REMOTE_NAMESPACE}`) as PanelRemoteApi | undefined
    return service ?? null
  })()
  mountedRemotes.set(ctx, mountedRemote)
  return mountedRemote
}

/** Only a primitive survives a client bundle swap. A new document gets a new ID. */
function pageId(doc: Document): string {
  const data = doc as Document & { __umlViewerPageId?: string }
  return data.__umlViewerPageId ??= crypto.randomUUID()
}

/**
 * Each UML tab's latest panel holder, by session and tab id, so the tab's
 * close handler closes exactly that holder. A live body closes through its
 * controller; a body that already unmounted (withdrawn, waiting out its grace)
 * is closed by its last owner, which stops the resident at once.
 */
const tabHolders = new WeakMap<ClientContext, Map<string, () => Promise<unknown>>>()

function holdersOf(ctx: ClientContext): Map<string, () => Promise<unknown>> {
  let holders = tabHolders.get(ctx)
  if (!holders) tabHolders.set(ctx, holders = new Map())
  return holders
}

const tabKey = (sessionId: string, tabId: string): string => `${sessionId}\u0000${tabId}`

/** The host's tab close handler: user close and replacement alike (D13). */
function closeTab(ctx: ClientContext, sessionId: string, tabId: string): void {
  const holders = holdersOf(ctx)
  const key = tabKey(sessionId, tabId)
  const close = holders.get(key)
  holders.delete(key)
  // The handler is synchronous; the close itself is best effort.
  void close?.().catch(() => {})
}

function summaryLine(): HTMLElement {
  const line = document.createElement('div')
  line.className = 'dsh-uml-viewer-summary'
  line.setAttribute('role', 'status')
  line.dataset.kind = 'connecting'
  return line
}

function statusLine(): HTMLElement {
  const line = document.createElement('div')
  line.className = 'dsh-uml-viewer-status'
  line.setAttribute('role', 'status')
  line.style.display = 'none'
  return line
}

function button(label: string, title: string, onClick: () => void): HTMLButtonElement {
  const element = document.createElement('button')
  element.type = 'button'
  labelButton(element, label, title)
  element.addEventListener('click', onClick)
  return element
}

function labelButton(element: HTMLButtonElement, label: string, title: string): void {
  element.textContent = label
  element.title = title
  element.setAttribute('aria-label', title)
}

export interface MountOptions {
  sessionId: string
  ctx: ClientContext
  /** The conversation's 「在面板中查看」 target: this mount's overlays and its live frames. */
  views?: PanelViews
  /** The host tab this body draws; its close handler closes this mount's holder. */
  tabId?: string
  /** The host tab's `bindCommands`: the tab refresh command reloads the diagram. */
  bindCommands?: (commands: TabCommands) => () => void
  /** Receives this mount's overlay controller (inspector, card, sources). */
  onOverlays?: (controller: OverlayController | null) => void
}

/**
 * Attach a controller to a host element and return the disposer. The canvas
 * fills the element; the panel is bound to `sessionId` for its whole life.
 */
export async function mountPanel(host: HTMLElement, options: MountOptions): Promise<() => void> {
  // The React panel mounts the canvas into an inner host beside its overlays;
  // the panel element itself names the session and surface.
  const panelElement = host.closest?.<HTMLElement>('.dsh-uml-viewer-panel') ?? host
  panelElement.dataset.sessionId = options.sessionId
  const text: PanelText = panelText(name => options.ctx.get(name))
  const canvas = document.createElement('canvas')
  canvas.style.touchAction = 'none'
  canvas.tabIndex = 0
  const summary = summaryLine()
  const status = statusLine()
  const toolbar = document.createElement('div')
  toolbar.className = 'dsh-uml-viewer-toolbar'
  const search = document.createElement('div')
  search.className = 'dsh-uml-viewer-search'
  const input = document.createElement('input')
  input.type = 'search'
  input.autocomplete = 'off'
  const results = document.createElement('ul')
  results.className = 'dsh-uml-viewer-results'
  results.setAttribute('role', 'listbox')
  results.hidden = true
  /** The static labels; a DSH language switch reapplies them in place. */
  const labelStatic = (): void => {
    canvas.setAttribute('aria-label', text.t('canvas.label'))
    summary.setAttribute('aria-label', text.t('summary.label'))
    input.placeholder = text.t('search.placeholder')
    input.setAttribute('aria-label', text.t('search.label'))
    results.setAttribute('aria-label', text.t('search.results'))
  }
  search.append(input, results)
  const spacer = document.createElement('span')
  spacer.className = 'dsh-uml-viewer-spacer'
  host.append(toolbar, summary, canvas, status)
  labelStatic()
  const api = await mountRemote(options.ctx)
  if (!api) {
    status.textContent = text.t('gateway.missing')
    status.style.display = ''
    const stop = text.subscribe(() => { labelStatic(); status.textContent = text.t('gateway.missing') })
    return () => { stop(); host.replaceChildren() }
  }
  let overlays: OverlayController | null = null
  let lastResults = ''
  /** A refusal from the element menu, re-rendered on a language switch. */
  let reported: string | (() => string) | null = null
  const showReport = (): void => {
    const reason = typeof reported === 'function' ? reported() : reported
    status.textContent = reason ?? ''
    status.style.display = reason ? '' : 'none'
  }
  /** The frame the overlays last refreshed for; search/status updates reuse it. */
  let overlayFrame: SceneFrame | null = null
  /** The holder this mount last opened; a withdrawn tab is closed by it. */
  let lastOwner: PanelOwner | null = null
  const renderStatus = (state: PanelStatus): void => {
    const frame = state.frame
    // A new view frame refreshes the inspector/card; a search or status
    // publish that keeps the same frame does not re-request them.
    if (state.kind === 'live' && frame && overlays && frame !== overlayFrame) {
      overlayFrame = frame
      void overlays.refresh()
      // A locate from the conversation waited for this panel to be able to take
      // actions; the first live frame is that moment (#54).
      options.views?.shown(options.sessionId)
    }
    summary.dataset.kind = frame ? 'frame' : state.kind
    summary.dataset.context = state.context?.kind ?? 'real'
    summary.dataset.search = state.search.active ? 'active' : 'closed'
    summary.dataset.results = String(state.search.results.length)
    if (frame) {
      const heading = frame.chrome.find(item => item.kind === 'title')
      const title = (heading && 'text' in heading ? heading.text : null) ?? text.t('summary.untitled')
      const proposal = frame.chrome.find(item => item.kind === 'proposal-banner')
      const view = proposal && 'text' in proposal ? 'proposal' : 'source'
      summary.dataset.view = view
      summary.dataset.depth = frame.chrome.some(item => item.kind === 'back') ? 'drilled' : 'root'
      const selected = frame.world.find(item => 'selected' in item && item.selected)
      summary.dataset.selected = selected && 'id' in selected ? selected.id : ''
      summary.dataset.chain = String(frame.edges.filter(item => item.kind === 'stroke' && item.stroke.role === 'dependency-chain').length)
      summary.dataset.dimmed = String(frame.world.filter(item => item.dimmed).length)
      const discussion = state.context?.kind === 'proposal' ? state.context.name ?? state.context.proposalId ?? '' : text.t('summary.source')
      const camera = frame.camera
      summary.textContent = text.t('summary.frame', {
        title, discussion, view: text.t(view === 'proposal' ? 'summary.proposal' : 'summary.source'),
        camera: `${camera.x.toFixed(3)}, ${camera.y.toFixed(3)}, ${camera.zoom.toFixed(3)}`,
      })
    } else {
      summary.textContent = text.t('summary.state', { state: text.t(`state.${state.kind}` as const) })
    }
    reported = state.kind === 'refused' || state.kind === 'failed' ? state.reason : null
    showReport()
    back.disabled = !frame?.chrome.some(item => item.kind === 'back')
    if (document.activeElement !== input && input.value !== state.search.query) input.value = state.search.query
    const signature = JSON.stringify([state.search.active, state.search.results, state.search.selection])
    if (signature !== lastResults) {
      lastResults = signature
      results.replaceChildren()
      results.hidden = !state.search.active
      if (state.search.active && state.search.query.trim() && state.search.results.length === 0) {
        const empty = document.createElement('li')
        empty.className = 'dsh-uml-viewer-empty'
        empty.textContent = text.t('search.empty')
        results.appendChild(empty)
      }
      state.search.results.forEach((result, index) => {
        const row = document.createElement('li')
        const choice = document.createElement('button')
        choice.type = 'button'
        choice.setAttribute('role', 'option')
        choice.setAttribute('aria-selected', String(index === state.search.selection))
        choice.textContent = result.label
        choice.addEventListener('click', () => {
          controller.moveSearch(index - controller.current.search.selection)
          controller.activateSearch()
          canvas.focus({ preventScroll: true })
        })
        row.appendChild(choice)
        results.appendChild(row)
      })
    }
  }
  const controller = new PanelController({
    canvas,
    api,
    owner: () => (lastOwner = { pageId: pageId(host.ownerDocument), holderId: crypto.randomUUID() }),
    paint: (frame, target) => paintFrame(frame, target),
    onStatus: renderStatus,
  })
  const disposeQualityMenu = mountQualityMenu(host, canvas, {
    frame: () => controller.current.frame,
    send: action => controller.send(action),
    report(reason) { reported = reason; showReport() },
    text,
  })
  overlays = new OverlayController(action => controller.send(action), text)
  options.views?.attach(options.sessionId, overlays)
  options.onOverlays?.(overlays)
  /** **P** and the toolbar's tree button: one dispatch, through the overlays so
   * a card reply in flight cannot repopulate the card after the tree resets. */
  const namespaceTree = (): void => {
    if (overlays) void overlays.namespaceTree()
    else void controller.namespaceTree()
  }
  const back = button(text.t('back.label'), text.t('back.title'), () => {
    void controller.back()
    canvas.focus({ preventScroll: true })
  })
  back.disabled = true
  const tree = button('Namespace tree', 'Namespace tree (P)', () => {
    namespaceTree()
    canvas.focus({ preventScroll: true })
  })
  toolbar.append(back, tree, search)
  const disposeDeclutter = mountDeclutterControl(toolbar, controller, text)
  toolbar.append(spacer)
  // Closing the host tab is the explicit close; while this body is mounted the
  // controller closes its live holder (the stream ends without a withdrawal).
  const tabId = options.tabId
  const holders = holdersOf(options.ctx)
  const closeLive = (): Promise<unknown> => controller.close()
  if (tabId !== undefined) holders.set(tabKey(options.sessionId, tabId), closeLive)
  // The host tab's refresh command is the same reload as **R**.
  const unbindCommands = options.bindCommands?.({ refresh: () => { void controller.send({ action: 'reload' }) } })
  // The overlays sit below the (wrapping) toolbar instead of covering it.
  const placeOverlays = (): void => {
    panelElement.style.setProperty?.('--dsh-uml-toolbar-height', `${toolbar.offsetHeight ?? 0}px`)
  }
  placeOverlays()
  const unsubscribe = controller.subscribe(() => { /* state is published to React through overlays */ })
  // A DSH language switch relabels this mount in place; the tab stays open.
  const unsubscribeText = text.subscribe(() => {
    labelStatic()
    labelButton(back, text.t('back.label'), text.t('back.title'))
    lastResults = ''
    // Keep an element-menu refusal; the status publish would otherwise clear it.
    const kept = reported
    renderStatus(controller.current)
    reported = kept
    showReport()
  })
  // Keyboard input belongs to this panel; never capture the chat composer.
  // **P** from the overlay windows (outside the canvas host) also returns to
  // the tree; keys inside the host are handled once by `onKeyDown` below.
  const onPanelKey = (event: KeyboardEvent): void => {
    if (event.isComposing || event.keyCode === 229 || host.contains?.(event.target as Node)) return
    if (event.key.toLowerCase() !== 'p' || event.ctrlKey || event.metaKey || event.altKey) return
    const target = event.target as HTMLElement
    if (target.matches?.('input,textarea,select,[contenteditable=""],[contenteditable=true]')) return
    if (controller.current.search.active) return
    event.preventDefault()
    namespaceTree()
  }
  if (panelElement !== host) panelElement.addEventListener('keydown', onPanelKey)
  const onResize = (): void => {
    placeOverlays()
    void controller.measureViewport().then(size => controller.setViewport(size.width, size.height))
  }
  const resize = host.ownerDocument.defaultView?.ResizeObserver
  const observer = resize ? new resize(() => onResize()) : null
  observer?.observe(host)
  const onPointerDown = (event: PointerEvent): void => {
    if (event.button !== undefined && event.button !== 0) return
    canvas.focus({ preventScroll: true })
    controller.pointerDown(event.offsetX, event.offsetY)
  }
  const onPointerMove = (event: PointerEvent): void => {
    controller.hover(event.offsetX, event.offsetY)
    if (event.buttons !== 0) controller.pointerMove(event.offsetX, event.offsetY)
  }
  const onPointerUp = (event: PointerEvent): void => controller.pointerUp(event.offsetX, event.offsetY)
  const onPointerLeave = (): void => controller.hoverEnd()
  // One double-click dispatch: the back hotspot goes back (native parity);
  // anything else is one `open-card`, where Rust decides class detail vs.
  // layer drill-in. There is no second `drill-in` handler. CSS stretches the
  // frame-sized bitmap per axis, so each axis is mapped separately.
  const onDoubleClick = (event: MouseEvent): void => {
    const frame = controller.current.frame
    if (!frame || controller.current.search.active) return
    const bounds = canvas.getBoundingClientRect()
    if (!(bounds.width > 0) || !(bounds.height > 0)) return
    const fx = event.offsetX * frame.viewport.width / bounds.width
    const fy = event.offsetY * frame.viewport.height / bounds.height
    if (frame.chrome.some(item => item.kind === 'back') && fx < 320 && fy < 44) {
      void controller.back()
      return
    }
    const world = cssToWorld(frame, bounds, event.offsetX, event.offsetY)
    if (world) void overlays?.openCardAt(world)
  }
  const onWheel = (event: WheelEvent): void => {
    event.preventDefault()
    controller.wheel(event.offsetX, event.offsetY, -event.deltaY / 100, {
      control: event.ctrlKey || event.metaKey,
      shift: event.shiftKey,
    }, -event.deltaX / 100)
  }
  const onKeyDown = (event: KeyboardEvent): void => {
    if (event.isComposing || event.keyCode === 229 || event.target === input) return
    // `controller.key` would also map P; route it once, through the overlays.
    if (!controller.current.search.active && event.key.toLowerCase() === 'p'
      && !event.ctrlKey && !event.metaKey) {
      event.preventDefault()
      event.stopPropagation()
      namespaceTree()
      return
    }
    const handled = controller.key(event.key, { control: event.ctrlKey || event.metaKey, shift: event.shiftKey })
    if (!handled) return
    event.preventDefault()
    event.stopPropagation()
    if (controller.current.search.active) input.focus({ preventScroll: true })
  }
  const onSearchKey = (event: KeyboardEvent): void => {
    if (event.isComposing || event.keyCode === 229) return
    if (!['ArrowDown', 'ArrowUp', 'Enter', 'Escape'].includes(event.key)
      && !((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === 'f')) return
    event.preventDefault()
    event.stopPropagation()
    controller.key(event.key, { control: event.ctrlKey || event.metaKey })
    if (event.key === 'Escape') input.blur()
  }
  const onSearchInput = (): void => controller.setSearchQuery(input.value)
  canvas.addEventListener('pointerdown', onPointerDown)
  canvas.addEventListener('pointermove', onPointerMove)
  canvas.addEventListener('pointerup', onPointerUp)
  canvas.addEventListener('pointerleave', onPointerLeave)
  canvas.addEventListener('dblclick', onDoubleClick)
  canvas.addEventListener('wheel', onWheel, { passive: false })
  host.addEventListener('keydown', onKeyDown)
  input.addEventListener('keydown', onSearchKey)
  input.addEventListener('input', onSearchInput)
  void loadSarasa().then(() => controller.repaint())
  await controller.open(options.sessionId)
  return () => {
    unbindCommands?.()
    if (tabId !== undefined) {
      const key = tabKey(options.sessionId, tabId)
      // Unmounting is a withdrawal. A later close of this tab still means
      // "stop now", so it is left with this mount's last owner, unless a newer
      // body of the same tab registered meanwhile.
      if (holders.get(key) === closeLive) {
        const owner = lastOwner
        if (owner && api.closeOwned) holders.set(key, () => api.closeOwned!(options.sessionId, owner))
        else holders.delete(key)
      }
    }
    observer?.disconnect()
    if (overlays) options.views?.detach(options.sessionId, overlays)
    overlays?.dispose()
    overlays = null
    options.onOverlays?.(null)
    if (panelElement !== host) panelElement.removeEventListener('keydown', onPanelKey)
    disposeDeclutter()
    disposeQualityMenu()
    unsubscribeText()
    unsubscribe()
    canvas.removeEventListener('pointerdown', onPointerDown)
    canvas.removeEventListener('pointermove', onPointerMove)
    canvas.removeEventListener('pointerup', onPointerUp)
    canvas.removeEventListener('pointerleave', onPointerLeave)
    canvas.removeEventListener('dblclick', onDoubleClick)
    canvas.removeEventListener('wheel', onWheel)
    host.removeEventListener('keydown', onKeyDown)
    input.removeEventListener('keydown', onSearchKey)
    input.removeEventListener('input', onSearchInput)
    controller.detach()
    host.replaceChildren()
  }
}

/** The sidebar tab's body component. */
export function makePanelComponent(ctx: ClientContext, views?: PanelViews): (props: ClientPanelProps) => unknown {
  return function UmlViewerPanel(props: ClientPanelProps): unknown {
    // One live mount per attach. A detach or rebind while `mountPanel` is
    // still awaiting invalidates that mount: its late disposer runs at once
    // and its overlay controller is never published.
    const mountRef = useRef<{ dispose: (() => void) | null } | null>(null)
    const [overlays, setOverlays] = useState<OverlayController | null>(null)
    // The host's tab information hook (`sidebar.right.pane.tab`): the tab id
    // the close handler is keyed by, and the tab's command binding.
    const info = typeof props.useTabInfo === 'function' ? (props.useTabInfo as () => TabInfo)() : undefined
    const tabRef = useRef<TabInfo['tab']>(undefined)
    tabRef.current = info?.tab
    const tabId = info?.tab?.id
    const attach = useCallback((element: HTMLElement | null): void => {
      if (!element) {
        const mount = mountRef.current
        mountRef.current = null
        mount?.dispose?.()
        setOverlays(null)
        return
      }
      if (mountRef.current) return
      const sessionId = props.sessionId
      if (typeof sessionId !== 'string') return
      const tab = tabRef.current
      const actions = tab?.actions
      const mount: { dispose: (() => void) | null } = { dispose: null }
      mountRef.current = mount
      void mountPanel(element, {
        sessionId, ctx, ...(views ? { views } : {}),
        ...(typeof tab?.id === 'string' ? { tabId: tab.id } : {}),
        ...(typeof actions?.bindCommands === 'function' ? { bindCommands: commands => actions.bindCommands!(commands) } : {}),
        onOverlays: controller => { if (mountRef.current === mount) setOverlays(controller) },
      }).then(dispose => {
        if (mountRef.current === mount) mount.dispose = dispose
        else dispose()
      })
    }, [ctx, props.sessionId, tabId, views])
    return createElement('div', { className: 'dsh-uml-viewer-panel', 'data-font': FONT_FAMILY },
      createElement('div', { className: 'dsh-uml-viewer-canvas-host', ref: attach }),
      overlays ? createElement(Overlays, { controller: overlays }) : null)
  }
}

/** Inject the panel stylesheet once per page. */
function installStyles(): () => void {
  const id = 'dsh-uml-viewer-style'
  if (document.querySelector(`style[data-plugin-css="${id}"]`) === null) {
    const tag = document.createElement('style')
    tag.dataset.pluginCss = id
    tag.textContent = STYLE + OVERLAY_STYLE
    document.head.appendChild(tag)
  }
  return () => {
    document.querySelector(`style[data-plugin-css="${id}"]`)?.remove()
  }
}

/**
 * 「在面板中查看」 (#54): open this session's UML tab through the host's
 * navigation, which reveals the column and records the navigation like the
 * sidebar's own controls. The tab kind is this plugin's page; a host without
 * the service simply has no column to open.
 */
function openPanelTab(ctx: ClientContext): void {
  const sidebar = ctx.get('sidebarRight') as SidebarRightFace | undefined
  sidebar?.openTab?.(TAB_KIND)
}

function registerPanels(ctx: ClientContext): void {
  const views = new PanelViews(() => openPanelTab(ctx))
  const Panel = makePanelComponent(ctx, views)
  ctx.effect(() => installStyles(), 'dsh-uml-viewer: panel style')
  // The five `uml_*` tool result views (#54); they locate into this panel.
  ctx.effect(() => registerToolViews(ctx, views), 'dsh-uml-viewer: tool views')

  // The tab is bound to the session the right sidebar belongs to; the pane
  // receives that session id from the shell.
  ctx.inject(['sidebarRightTabs'], (injected) => {
    const tabs = (injected as unknown as { sidebarRightTabs?: SidebarTabsFace }).sidebarRightTabs
    if (!tabs) return
    const disposers: (() => void)[] = []
    const own = (result: unknown): void => {
      if (typeof result === 'function') disposers.push(result as () => void)
    }
    const text = panelText(name => ctx.get(name))
    own(tabs.register({
      id: TAB_ID,
      kind: TAB_KIND,
      title: () => text.t('tab.title'),
      keepMounted: true,
      guide: [{ id: TAB_ID, order: GUIDE_ORDER, title: () => text.t('tab.title'), description: () => text.t('guide.description') }],
    }))
    own(ctx.slots.inject('sidebar.right.pane.tab', () => ctx.slots.register(
      { name: 'sidebar.right.pane.tab', key: TAB_ID },
      props => Panel(props as ClientPanelProps),
    )))
    own(ctx.slots.inject('sidebar.right.pane.tab.title', () => ctx.slots.register(
      { name: 'sidebar.right.pane.tab.title', key: TAB_ID },
      () => createElement('span', null, text.t('tab.title')),
    )))
    if (typeof tabs.register !== 'function') return
    return () => { for (const dispose of disposers) dispose() }
  })

  // Closing the UML tab, or a replacement that removes it, is the explicit
  // close (D13). Collapse, fullscreen or presentation changes and plugin
  // unload never call this; they only unmount the body (a withdrawal).
  ctx.inject(['sidebarRight'], (injected) => {
    const sidebar = (injected as unknown as { sidebarRight?: SidebarRightFace }).sidebarRight
    if (typeof sidebar?.registerCloseHandler !== 'function') return
    const dispose = sidebar.registerCloseHandler(TAB_KIND, (sessionId, tab) => closeTab(ctx, sessionId, tab.id))
    return typeof dispose === 'function' ? dispose : undefined
  })
}

export const name = 'dsh-uml-viewer'
export const inject = ['slots']

export function apply(ctx: ClientContext): void {
  // The Remote mount is lazy: it happens the first time a panel is shown, so a
  // page that never opens the panel makes no Remote calls.
  registerPanels(ctx)
}

const exported = { name, inject, apply }
export default exported
