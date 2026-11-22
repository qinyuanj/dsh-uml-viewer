import { createElement, useCallback, useRef, useState } from 'react'
import { OverlayController } from './overlays.js'
import { Overlays, OVERLAY_STYLE } from './overlay-view.js'
import { UML_VIEWER_REMOTE, type PanelOwner, type SceneFrame } from '@dsh-uml-viewer/contracts'
import { FONT_FAMILY, loadSarasa } from './font.js'
import { PanelController, type PanelRemoteApi, type PanelStatus } from './panel.js'
import { paintFrame } from './paint.js'
import { cssToWorld } from './fit.js'
import { REQUEST_MENU_STYLE, mountQualityMenu } from './requests.js'
import { panelText, type PanelText } from './locale.js'
import { PanelViews, registerToolViews } from './tool-views.js'
import { MORE_MENU_STYLE } from './more-menu.js'
import { STATUSBAR_STYLE, mountStatusBar, type StatusBar } from './statusbar.js'
import { TOPBAR_STYLE, mountTopBar, type TopBar } from './topbar.js'
import { returnToSourceTree, type SwitcherActions } from './context-switcher.js'
import { ACTION_BAR_STYLE, elementSelection, mountActionBar, type ActionBar } from './action-bar.js'
import { LEGEND_STYLE, legendEntries, mountLegend, type Legend } from './legend.js'
import { DRY_CLUSTER_STYLE, dryClusterEntries, mountDryClusters, type DryClusters } from './dry-clusters.js'
import { observeLayout } from './layout.js'

export const TAB_ID = 'dsh-uml-viewer'
export const TAB_KIND = 'dsh-uml-viewer'
export const GUIDE_ORDER = 30
const REMOTE_NAMESPACE = 'umlViewer'

const STYLE = `${TOPBAR_STYLE}${MORE_MENU_STYLE}${STATUSBAR_STYLE}${REQUEST_MENU_STYLE}${ACTION_BAR_STYLE}${LEGEND_STYLE}${DRY_CLUSTER_STYLE}
.dsh-uml-viewer-panel { position: relative; display: flex; flex-direction: column; min-height: 0; height: 100%; width: 100%; --dsh-uml-dock-width: clamp(420px, 42%, 680px); }
.dsh-uml-viewer-canvas-host { position: absolute; inset: 0; display: flex; flex-direction: column; min-height: 0; }
.dsh-uml-viewer-canvas-row { position: relative; display: flex; flex-direction: column; flex: 1 1 auto; min-height: 0; min-width: 0; }
.dsh-uml-viewer-canvas-stage { position: relative; display: flex; flex: 1 1 auto; min-height: 0; min-width: 0; }
.dsh-uml-viewer-canvas-area { position: relative; display: flex; flex: 1 1 auto; flex-shrink: 1; min-height: 0; min-width: 0; }
.dsh-uml-viewer-dock-space { flex: 0 0 0; min-width: 0; }
.dsh-uml-viewer-panel[data-layout=docked]:has(.dsh-uml-drawer) .dsh-uml-viewer-dock-space { flex-basis: var(--dsh-uml-dock-width); }
.dsh-uml-viewer-panel canvas { display: block; width: 100%; height: 100%; outline: none; }
.dsh-uml-viewer-panel > .dsh-uml-overlay-root {
  top: var(--dsh-uml-toolbar-height, 0px); bottom: var(--dsh-uml-statusbar-height, 0px);
}
.dsh-uml-viewer-summary {
  position: absolute; width: 1px; height: 1px; padding: 0; margin: -1px;
  overflow: hidden; clip-path: inset(50%); white-space: nowrap; border: 0;
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

export interface TabCommands {
  refresh?: () => void
}

interface TabInfo {
  tab?: { id?: string; actions?: { bindCommands?(commands: TabCommands): () => void } }
}

export interface SidebarRightFace {
  registerCloseHandler?(kind: string, handler: (sessionId: string, tab: { id: string }) => void): () => void
  openTab?(kind: string): void
}

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
      return null
    }
    const service = ctx.get(`remote.${REMOTE_NAMESPACE}`) as PanelRemoteApi | undefined
    return service ?? null
  })()
  mountedRemotes.set(ctx, mountedRemote)
  return mountedRemote
}

function pageId(doc: Document): string {
  const data = doc as Document & { __umlViewerPageId?: string }
  return data.__umlViewerPageId ??= crypto.randomUUID()
}

const tabHolders = new WeakMap<ClientContext, Map<string, () => Promise<unknown>>>()

function holdersOf(ctx: ClientContext): Map<string, () => Promise<unknown>> {
  let holders = tabHolders.get(ctx)
  if (!holders) tabHolders.set(ctx, holders = new Map())
  return holders
}

const tabKey = (sessionId: string, tabId: string): string => `${sessionId}\u0000${tabId}`

function closeTab(ctx: ClientContext, sessionId: string, tabId: string): void {
  const holders = holdersOf(ctx)
  const key = tabKey(sessionId, tabId)
  const close = holders.get(key)
  holders.delete(key)
  void close?.().catch(() => {})
}

function summaryLine(): HTMLElement {
  const line = document.createElement('div')
  line.className = 'dsh-uml-viewer-summary'
  line.setAttribute('role', 'status')
  line.dataset.kind = 'connecting'
  return line
}

export interface MountOptions {
  sessionId: string
  ctx: ClientContext
  views?: PanelViews
  tabId?: string
  bindCommands?: (commands: TabCommands) => () => void
  onOverlays?: (controller: OverlayController | null) => void
}

export async function mountPanel(host: HTMLElement, options: MountOptions): Promise<() => void> {
  const panelElement = host.closest?.<HTMLElement>('.dsh-uml-viewer-panel') ?? host
  panelElement.dataset.sessionId = options.sessionId
  const text: PanelText = panelText(name => options.ctx.get(name))
  const canvas = document.createElement('canvas')
  canvas.style.touchAction = 'none'
  canvas.className = 'dsh-uml-viewer-canvas'
  canvas.tabIndex = 0
  const summary = summaryLine()
  const canvasArea = document.createElement('div')
  canvasArea.className = 'dsh-uml-viewer-canvas-area'
  canvasArea.appendChild(canvas)
  const focusCanvas = (): void => canvas.focus({ preventScroll: true })
  const labelStatic = (): void => {
    canvas.setAttribute('aria-label', text.t('canvas.label'))
    summary.setAttribute('aria-label', text.t('summary.label'))
  }
  labelStatic()
  const stopLabels = text.subscribe(labelStatic)
  host.appendChild(summary)
  let topBar: TopBar | null = null
  let statusBar: StatusBar | null = null
  let legend: Legend | null = null
  let dryClusters: DryClusters | null = null
  const disposeSections = (): void => {
    stopLabels()
    topBar?.dispose()
    topBar = null
    dryClusters?.dispose()
    dryClusters = null
    statusBar?.dispose()
    statusBar = null
    legend?.dispose()
    legend = null
  }
  const api = await mountRemote(options.ctx)
  if (!api) {
    host.appendChild(canvasArea)
    statusBar = mountStatusBar(host, { text })
    statusBar.setMessage(() => text.t('gateway.missing'))
    return () => {
      disposeSections()
      host.replaceChildren()
    }
  }
  let overlays: OverlayController | null = null
  let placeOverlays: () => void = () => {}
  let legendVisible: boolean | null = null
  let actionBar: ActionBar | null = null
  let reported: string | (() => string) | null = null
  let overlayFrame: SceneFrame | null = null
  let lastOwner: PanelOwner | null = null
  const renderStatus = (state: PanelStatus): void => {
    const frame = state.frame
    if (state.kind === 'live' && frame && overlays && frame !== overlayFrame) {
      overlayFrame = frame
      void overlays.refresh()
      options.views?.shown(options.sessionId)
    }
    summary.dataset.kind = frame ? 'frame' : state.kind
    summary.dataset.context = state.context?.kind ?? 'real'
    summary.dataset.status = state.statusCode?.code ?? ''
    summary.dataset.breadcrumbs = String(state.breadcrumbs.length)
    summary.dataset.search = state.search.active ? 'active' : 'closed'
    summary.dataset.results = String(state.search.results.length)
    if (frame) {
      const title = state.breadcrumbs[0]?.name ?? text.t('summary.untitled')
      const view = state.view
      summary.dataset.view = view
      summary.dataset.depth = state.breadcrumbs.length > 1 ? 'drilled' : 'root'
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
    statusBar?.setStatus(state.statusCode)
    statusBar?.setMessage(reported)
    statusBar?.setRequests(state.requests)
    topBar?.render(state)
    legend?.render(state.frame)
    dryClusters?.render(state.frame)
    const visible = legend !== null && !legend.element.hidden
    if (visible !== legendVisible) {
      legendVisible = visible
      placeOverlays()
    }
    actionBar?.render(elementSelection(controller.selected, frame))
  }
  const controller = new PanelController({
    canvas,
    api,
    language: () => text.language,
    owner: () => (lastOwner = { pageId: pageId(host.ownerDocument), holderId: crypto.randomUUID() }),
    paint: (frame, target) => paintFrame(frame, target),
    onStatus: renderStatus,
  })
  const switcherActions: SwitcherActions = {
    choose: proposalId => { void overlays?.choose(proposalId) },
    namespaceTree: () => {
      if (overlays) void overlays.namespaceTree()
      else void controller.namespaceTree()
    },
    newProposal: () => { void overlays?.newProposal(new Date()) },
  }
  const canvasRow = document.createElement('div')
  canvasRow.className = 'dsh-uml-viewer-canvas-row'
  topBar = mountTopBar(host, {
    controller, text, onDone: focusCanvas, context: switcherActions,
    moreEntries: () => [
      ...(legend ? legendEntries(legend, text) : []),
      ...(dryClusters ? dryClusterEntries(dryClusters, text) : []),
    ],
  })
  const canvasStage = document.createElement('div')
  canvasStage.className = 'dsh-uml-viewer-canvas-stage'
  const dockSpace = document.createElement('div')
  dockSpace.className = 'dsh-uml-viewer-dock-space'
  dockSpace.setAttribute('aria-hidden', 'true')
  canvasStage.append(canvasArea, dockSpace)
  legend = mountLegend(canvasRow, { text, onDone: focusCanvas })
  dryClusters = mountDryClusters(canvasStage, {
    text,
    locate: id => { void overlays?.locate(id) },
    onOpen: () => legend?.close(),
    onDone: focusCanvas,
  })
  canvasRow.append(canvasStage, legend.element)
  host.appendChild(canvasRow)
  statusBar = mountStatusBar(host, {
    text,
    onDismiss: id => {
      void controller.send({ action: 'dismiss-request', id }).then(ack => {
        if (ack === null || ack.accepted) {
          statusBar?.setMessage(null)
          return
        }
        const reason = ack.reason ?? text.t('request.refused')
        statusBar?.setMessage(() => text.t('request.dismiss.refused', { reason }))
      }, error => {
        const reason = error instanceof Error ? error.message : String(error)
        statusBar?.setMessage(() => text.t('request.dismiss.refused', { reason }))
      })
    },
  })
  const disposeQualityMenu = mountQualityMenu(host, canvas, {
    frame: () => controller.current.frame,
    send: action => controller.send(action),
    report(reason) {
      reported = reason
      statusBar?.setMessage(reason)
    },
    text,
  })
  overlays = new OverlayController(action => controller.send(action), text)
  dryClusters?.render(controller.current.frame)
  actionBar = mountActionBar(canvasArea, {
    text,
    send: action => controller.send(action),
    openCard: id => overlays?.openCard(id),
    report(reason) {
      reported = reason
      statusBar?.setMessage(reason)
    },
  })
  options.views?.attach(options.sessionId, overlays)
  options.onOverlays?.(overlays)
  controller.setDrawer({
    open: () => overlays?.drawerOpen === true,
    close: () => { void overlays?.closeDrawer() },
  })
  controller.setLegend({
    isOpen: () => legend?.isOpen() === true || dryClusters?.isOpen() === true,
    close: () => { legend?.close(); dryClusters?.close() },
  })
  placeOverlays = (): void => {
    panelElement.style.setProperty?.('--dsh-uml-toolbar-height', `${topBar?.element.offsetHeight ?? 0}px`)
    const legendHeight = legend?.element.offsetHeight ?? 0
    panelElement.style.setProperty?.('--dsh-uml-statusbar-height', `${(statusBar?.element.offsetHeight ?? 0) + legendHeight}px`)
  }
  const stopLayout = observeLayout(panelElement, layout => {
    legend?.setLayout(layout)
    placeOverlays()
  })
  const namespaceTree = (): void => returnToSourceTree(controller.current.context, switcherActions)
  const tabId = options.tabId
  const holders = holdersOf(options.ctx)
  const closeLive = (): Promise<void> => controller.close()
  if (tabId !== undefined) holders.set(tabKey(options.sessionId, tabId), closeLive)
  const unbindCommands = options.bindCommands?.({ refresh: () => { void controller.send({ action: 'reload' }) } })
  placeOverlays()
  const unsubscribe = controller.subscribe(() => {})
  const unsubscribeText = text.subscribe(() => {
    void controller.setLanguage(text.language)
    const kept = reported
    renderStatus(controller.current)
    reported = kept
    statusBar?.setMessage(kept)
  })
  const onPanelKey = (event: KeyboardEvent): void => {
    if (event.isComposing || event.keyCode === 229 || host.contains?.(event.target as Node)) return
    if (event.ctrlKey || event.metaKey || event.altKey) return
    const target = event.target as HTMLElement
    if (target.matches?.('input,textarea,select,[contenteditable=""],[contenteditable=true]')) return
    if (event.key === 'Escape') {
      event.preventDefault()
      controller.escape()
      focusCanvas()
      return
    }
    if (event.key.toLowerCase() !== 'p') return
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
  observer?.observe(canvasArea)
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
  const onDoubleClick = (event: MouseEvent): void => {
    const frame = controller.current.frame
    if (!frame || controller.current.search.active) return
    const bounds = canvas.getBoundingClientRect()
    if (!(bounds.width > 0) || !(bounds.height > 0)) return
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
    if (event.isComposing || event.keyCode === 229 || topBar?.owns(event.target) === true) return
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
    if (controller.current.search.active) topBar?.focusSearch()
  }
  canvas.addEventListener('pointerdown', onPointerDown)
  canvas.addEventListener('pointermove', onPointerMove)
  canvas.addEventListener('pointerup', onPointerUp)
  canvas.addEventListener('pointerleave', onPointerLeave)
  canvas.addEventListener('dblclick', onDoubleClick)
  canvas.addEventListener('wheel', onWheel, { passive: false })
  host.addEventListener('keydown', onKeyDown)
  void loadSarasa().then(() => controller.repaint())
  await controller.open(options.sessionId)
  return () => {
    unbindCommands?.()
    if (tabId !== undefined) {
      const key = tabKey(options.sessionId, tabId)
      if (holders.get(key) === closeLive) {
        const owner = lastOwner
        if (owner && api.closeOwned) holders.set(key, () => api.closeOwned!(options.sessionId, owner))
        else holders.delete(key)
      }
    }
    observer?.disconnect()
    stopLayout()
    controller.setDrawer(null)
    controller.setLegend(null)
    if (overlays) options.views?.detach(options.sessionId, overlays)
    overlays?.dispose()
    overlays = null
    options.onOverlays?.(null)
    if (panelElement !== host) panelElement.removeEventListener('keydown', onPanelKey)
    disposeQualityMenu()
    actionBar?.dispose()
    actionBar = null
    disposeSections()
    unsubscribeText()
    unsubscribe()
    canvas.removeEventListener('pointerdown', onPointerDown)
    canvas.removeEventListener('pointermove', onPointerMove)
    canvas.removeEventListener('pointerup', onPointerUp)
    canvas.removeEventListener('pointerleave', onPointerLeave)
    canvas.removeEventListener('dblclick', onDoubleClick)
    canvas.removeEventListener('wheel', onWheel)
    host.removeEventListener('keydown', onKeyDown)
    controller.detach()
    host.replaceChildren()
  }
}

export function makePanelComponent(ctx: ClientContext, views?: PanelViews): (props: ClientPanelProps) => unknown {
  return function UmlViewerPanel(props: ClientPanelProps): unknown {
    const mountRef = useRef<{ dispose: (() => void) | null } | null>(null)
    const [overlays, setOverlays] = useState<OverlayController | null>(null)
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

function openPanelTab(ctx: ClientContext): void {
  const sidebar = ctx.get('sidebarRight') as SidebarRightFace | undefined
  sidebar?.openTab?.(TAB_KIND)
}

function registerPanels(ctx: ClientContext): void {
  const views = new PanelViews(() => openPanelTab(ctx))
  const Panel = makePanelComponent(ctx, views)
  ctx.effect(() => installStyles(), 'dsh-uml-viewer: panel style')
  ctx.effect(() => registerToolViews(ctx, views), 'dsh-uml-viewer: tool views')

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
  registerPanels(ctx)
}

const exported = { name, inject, apply }
export default exported
