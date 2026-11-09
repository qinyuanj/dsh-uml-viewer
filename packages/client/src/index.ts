/**
 * The plugin's browser half: one right-sidebar tab bound to the session the
 * sidebar belongs to, and the same panel as a full-width `main` page.
 *
 * React comes from the host's platform module table (DECISIONS.md D1): this
 * bundle requires it, never embeds it, and imports no
 * `dsh-client-ui-primitives`. The chrome is plain markup styled from host
 * theme tokens, which is why the panel reads tokens for its Canvas 2D colors
 * instead of relying on primitive components.
 *
 * The `main` slot is root-scoped: it receives no session prop. Expanding
 * records the sidebar's session id first and only then selects the main panel,
 * so the full-width page keeps watching the same session's resident.
 */
import { createElement } from 'react'
import { UML_VIEWER_REMOTE } from '@dsh-uml-viewer/contracts'
import { FONT_FAMILY, loadSarasa } from './font.js'
import { PanelController, type PanelRemoteApi, type PanelStatus } from './panel.js'
import { paintFrame } from './paint.js'

export const TAB_ID = 'dsh-uml-viewer'
export const TAB_KIND = 'dsh-uml-viewer'
export const PANEL_PATH = 'dsh-uml-viewer'
export const GUIDE_ORDER = 30
const REMOTE_NAMESPACE = 'umlViewer'

const STYLE = `
.dsh-uml-viewer-panel { position: relative; display: flex; flex-direction: column; min-height: 0; height: 100%; width: 100%; }
.dsh-uml-viewer-toolbar {
  display: flex; flex-wrap: wrap; align-items: center; gap: 4px; flex: 0 0 auto; min-height: 36px; padding: 4px 8px;
  border-bottom: 1px solid var(--dsw-alias-border-l2);
  background: var(--dsw-alias-bg-layer-2); color: var(--dsw-alias-label-primary);
  font: 12px/1.4 var(--dsw-font-family, system-ui);
}
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

export interface LayoutFace {
  selectPanel(key: string | null): void
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
let mountedRemote: Promise<PanelRemoteApi | null> | null = null

async function mountRemote(ctx: ClientContext): Promise<PanelRemoteApi | null> {
  if (mountedRemote) return mountedRemote
  mountedRemote = (async () => {
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
  return mountedRemote
}

function summaryLine(): HTMLElement {
  const line = document.createElement('div')
  line.className = 'dsh-uml-viewer-summary'
  line.setAttribute('role', 'status')
  line.setAttribute('aria-label', 'UML panel summary')
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
  element.textContent = label
  element.title = title
  element.setAttribute('aria-label', title)
  element.addEventListener('click', onClick)
  return element
}

export interface MountOptions {
  sessionId: string
  ctx: ClientContext
  /** The sidebar offers expansion; the main page offers the way back. */
  main?: boolean
  onExpand?: (sessionId: string) => void
  onCollapse?: () => void
}

/**
 * Attach a controller to a host element and return the disposer. The canvas
 * fills the element; the panel is bound to `sessionId` for its whole life.
 */
export async function mountPanel(host: HTMLElement, options: MountOptions): Promise<() => void> {
  host.dataset.sessionId = options.sessionId
  host.dataset.surface = options.main ? 'main' : 'sidebar'
  const canvas = document.createElement('canvas')
  canvas.style.touchAction = 'none'
  canvas.tabIndex = 0
  canvas.setAttribute('aria-label', 'UML diagram canvas')
  const summary = summaryLine()
  const status = statusLine()
  const toolbar = document.createElement('div')
  toolbar.className = 'dsh-uml-viewer-toolbar'
  const search = document.createElement('div')
  search.className = 'dsh-uml-viewer-search'
  const input = document.createElement('input')
  input.type = 'search'
  input.placeholder = 'Search (Ctrl+F)'
  input.setAttribute('aria-label', 'Search classes')
  input.autocomplete = 'off'
  const results = document.createElement('ul')
  results.className = 'dsh-uml-viewer-results'
  results.setAttribute('role', 'listbox')
  results.setAttribute('aria-label', 'Search results')
  results.hidden = true
  search.append(input, results)
  const spacer = document.createElement('span')
  spacer.className = 'dsh-uml-viewer-spacer'
  host.append(toolbar, summary, canvas, status)
  const api = await mountRemote(options.ctx)
  if (!api) {
    status.textContent = 'The viewer panel needs the Web Remote gateway.'
    status.style.display = ''
    return () => { host.replaceChildren() }
  }
  let lastResults = ''
  const controller = new PanelController({
    canvas,
    api,
    paint: (frame, target) => paintFrame(frame, target),
    onStatus: (state: PanelStatus) => {
      const frame = state.frame
      summary.dataset.kind = frame ? 'frame' : state.kind
      summary.dataset.context = state.context?.kind ?? 'real'
      summary.dataset.search = state.search.active ? 'active' : 'closed'
      summary.dataset.results = String(state.search.results.length)
      if (frame) {
        const heading = frame.chrome.find(item => item.kind === 'title')
        const title = heading && 'text' in heading ? heading.text : 'Untitled diagram'
        const proposal = frame.chrome.find(item => item.kind === 'proposal-banner')
        summary.dataset.view = proposal && 'text' in proposal ? 'proposal' : 'source'
        summary.dataset.depth = frame.chrome.some(item => item.kind === 'back') ? 'drilled' : 'root'
        const selected = frame.world.find(item => 'selected' in item && item.selected)
        summary.dataset.selected = selected && 'id' in selected ? selected.id : ''
        summary.dataset.chain = String(frame.edges.filter(item => item.kind === 'stroke' && item.stroke.role === 'dependency-chain').length)
        summary.dataset.dimmed = String(frame.world.filter(item => item.dimmed).length)
        const discussion = state.context?.kind === 'proposal' ? state.context.name ?? state.context.proposalId : 'source diagram'
        const camera = frame.camera
        summary.textContent = `Diagram: ${title}. Discussion: ${discussion}. View: ${proposal && 'text' in proposal ? 'proposal diagram' : 'source diagram'}. Camera: ${camera.x.toFixed(3)}, ${camera.y.toFixed(3)}, ${camera.zoom.toFixed(3)}.`
      } else {
        summary.textContent = `UML panel: ${state.kind}.`
      }
      const reason = state.kind === 'refused' || state.kind === 'failed' ? state.reason : null
      status.textContent = reason ?? ''
      status.style.display = reason ? '' : 'none'
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
          empty.textContent = 'No matches'
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
    },
  })
  const back = button('‹ Back', 'Back (Esc)', () => {
    void controller.back()
    canvas.focus({ preventScroll: true })
  })
  back.disabled = true
  const tree = button('Namespace tree', 'Namespace tree (P)', () => {
    void controller.namespaceTree()
    canvas.focus({ preventScroll: true })
  })
  const expand = button(options.main ? 'Conversation' : 'Expand',
    options.main ? 'Return to conversation' : 'Expand to main area', () => {
      if (options.main) options.onCollapse?.()
      else options.onExpand?.(options.sessionId)
    })
  toolbar.append(back, tree, search, spacer, expand)
  const unsubscribe = controller.subscribe(() => { /* React panels poll `current`; nothing to do here. */ })
  const onResize = (): void => {
    void controller.measureViewport().then(size => controller.setViewport(size.width, size.height))
  }
  const resize = host.ownerDocument.defaultView?.ResizeObserver
  const observer = resize ? new resize(() => onResize()) : null
  observer?.observe(host)
  const onPointerDown = (event: PointerEvent): void => {
    canvas.focus({ preventScroll: true })
    controller.pointerDown(event.offsetX, event.offsetY)
  }
  const onPointerMove = (event: PointerEvent): void => {
    controller.hover(event.offsetX, event.offsetY)
    if (event.buttons !== 0) controller.pointerMove(event.offsetX, event.offsetY)
  }
  const onPointerUp = (event: PointerEvent): void => controller.pointerUp(event.offsetX, event.offsetY)
  const onDoubleClick = (event: MouseEvent): void => controller.doubleClick(event.offsetX, event.offsetY)
  const onWheel = (event: WheelEvent): void => {
    event.preventDefault()
    controller.wheel(event.offsetX, event.offsetY, -event.deltaY / 100, {
      control: event.ctrlKey || event.metaKey,
      shift: event.shiftKey,
    }, -event.deltaX / 100)
  }
  const onKeyDown = (event: KeyboardEvent): void => {
    if (event.isComposing || event.keyCode === 229 || event.target === input) return
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
  canvas.addEventListener('dblclick', onDoubleClick)
  canvas.addEventListener('wheel', onWheel, { passive: false })
  host.addEventListener('keydown', onKeyDown)
  input.addEventListener('keydown', onSearchKey)
  input.addEventListener('input', onSearchInput)
  void loadSarasa().then(() => controller.repaint())
  await controller.open(options.sessionId)
  return () => {
    observer?.disconnect()
    unsubscribe()
    canvas.removeEventListener('pointerdown', onPointerDown)
    canvas.removeEventListener('pointermove', onPointerMove)
    canvas.removeEventListener('pointerup', onPointerUp)
    canvas.removeEventListener('dblclick', onDoubleClick)
    canvas.removeEventListener('wheel', onWheel)
    host.removeEventListener('keydown', onKeyDown)
    input.removeEventListener('keydown', onSearchKey)
    input.removeEventListener('input', onSearchInput)
    controller.close()
    host.replaceChildren()
  }
}

export interface PanelComponentOptions {
  main?: boolean
  onExpand?: (sessionId: string) => void
  onCollapse?: () => void
  /** The root-scoped main page reads the session recorded at expansion. */
  mainSessionId?: () => string | null
}

/** One panel component, used by both the sidebar tab and the `main` page. */
export function makePanelComponent(ctx: ClientContext, options: PanelComponentOptions = {}): (props: ClientPanelProps) => unknown {
  return function UmlViewerPanel(props: ClientPanelProps): unknown {
    const disposeRef: { current: (() => void) | null } = { current: null }
    const attach = (element: HTMLElement | null): void => {
      if (!element) {
        disposeRef.current?.()
        disposeRef.current = null
        return
      }
      if (disposeRef.current) return
      const sessionId = options.main ? options.mainSessionId?.() : props.sessionId
      if (typeof sessionId !== 'string') return
      void mountPanel(element, { sessionId, ctx, ...options }).then(dispose => { disposeRef.current = dispose })
    }
    return createElement('div', {
      className: 'dsh-uml-viewer-panel',
      'data-font': FONT_FAMILY,
      ref: attach,
    })
  }
}

/** Inject the panel stylesheet once per page. */
function installStyles(): () => void {
  const id = 'dsh-uml-viewer-style'
  if (document.querySelector(`style[data-plugin-css="${id}"]`) === null) {
    const tag = document.createElement('style')
    tag.dataset.pluginCss = id
    tag.textContent = STYLE
    document.head.appendChild(tag)
  }
  return () => {
    document.querySelector(`style[data-plugin-css="${id}"]`)?.remove()
  }
}

/** The session the root-scoped main page belongs to, recorded before selection. */
let mainSessionId: string | null = null
try {
  // `main` has no session prop; keep a page-reload on the full-width view on
  // the same conversation, as well as an expand in the current client.
  mainSessionId = window.sessionStorage.getItem('dsh-uml-viewer:main-session')
} catch { /* storage is optional */ }

function layoutOf(ctx: ClientContext): LayoutFace | undefined {
  return ctx.get('layout') as LayoutFace | undefined
}

function registerPanels(ctx: ClientContext): void {
  const Panel = makePanelComponent(ctx, {
    onExpand(sessionId) {
      // Set the binding before selecting a root-scoped panel: `main` has no
      // session prop of its own and must never fall back to another session.
      mainSessionId = sessionId
      try { window.sessionStorage.setItem('dsh-uml-viewer:main-session', sessionId) } catch { /* optional */ }
      layoutOf(ctx)?.selectPanel(PANEL_PATH)
    },
  })
  const MainPanel = makePanelComponent(ctx, {
    main: true,
    mainSessionId: () => mainSessionId,
    onCollapse() {
      mainSessionId = null
      try { window.sessionStorage.removeItem('dsh-uml-viewer:main-session') } catch { /* optional */ }
      layoutOf(ctx)?.selectPanel(null)
    },
  })
  ctx.effect(() => installStyles(), 'dsh-uml-viewer: panel style')

  // The tab is bound to the session the right sidebar belongs to; the pane
  // receives that session id from the shell.
  ctx.inject(['sidebarRightTabs'], (injected) => {
    const tabs = (injected as unknown as { sidebarRightTabs?: SidebarTabsFace }).sidebarRightTabs
    if (!tabs) return
    const disposers: (() => void)[] = []
    const own = (result: unknown): void => {
      if (typeof result === 'function') disposers.push(result as () => void)
    }
    own(tabs.register({
      id: TAB_ID,
      kind: TAB_KIND,
      title: () => 'UML',
      keepMounted: true,
      guide: [{ id: TAB_ID, order: GUIDE_ORDER, title: () => 'UML', description: () => 'The session project’s namespace tree and quality' }],
    }))
    own(ctx.slots.inject('sidebar.right.pane.tab', () => ctx.slots.register(
      { name: 'sidebar.right.pane.tab', key: TAB_ID },
      props => Panel(props as ClientPanelProps),
    )))
    own(ctx.slots.inject('sidebar.right.pane.tab.title', () => ctx.slots.register(
      { name: 'sidebar.right.pane.tab.title', key: TAB_ID },
      () => createElement('span', null, 'UML'),
    )))
    own(ctx.slots.inject('main', () => ctx.slots.register(
      { name: 'main', key: PANEL_PATH },
      props => MainPanel(props as ClientPanelProps),
    )))
    if (typeof tabs.register !== 'function') return
    return () => { for (const dispose of disposers) dispose() }
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
