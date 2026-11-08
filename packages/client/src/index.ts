/**
 * The plugin's browser half: one right-sidebar tab bound to the session the
 * sidebar belongs to, and the same panel as a full-width `main` page.
 *
 * React comes from the host's platform module table (DECISIONS.md D1): this
 * bundle requires it, never embeds it, and imports no
 * `dsh-client-ui-primitives`. The chrome is plain markup styled from host
 * theme tokens, which is why the panel reads tokens for its Canvas 2D colors
 * instead of relying on primitive components.
 */
import { createElement } from 'react'
import { UML_VIEWER_REMOTE } from '@dsh-uml-viewer/contracts'
import { FONT_FAMILY, loadSarasa } from './font.js'
import { PanelController, type PanelRemoteApi } from './panel.js'
import { paintFrame } from './paint.js'

export const TAB_ID = 'dsh-uml-viewer'
export const TAB_KIND = 'dsh-uml-viewer'
export const PANEL_PATH = 'dsh-uml-viewer'
export const GUIDE_ORDER = 30
const REMOTE_NAMESPACE = 'umlViewer'

const STYLE = `
.dsh-uml-viewer-panel { position: relative; display: flex; flex-direction: column; min-height: 0; height: 100%; width: 100%; }
.dsh-uml-viewer-panel canvas { display: block; flex: 1 1 auto; min-height: 0; width: 100%; height: 100%; }
.dsh-uml-viewer-status {
  position: absolute; left: 12px; right: 12px; bottom: 8px;
  font: 12px/1.4 var(--dsw-font-family, system-ui); color: var(--dsw-alias-state-error-primary, #ff4030);
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
async function mountRemote(ctx: ClientContext): Promise<PanelRemoteApi | null> {
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
}

function statusLine(): HTMLElement {
  const line = document.createElement('div')
  line.className = 'dsh-uml-viewer-status'
  line.setAttribute('role', 'status')
  line.style.display = 'none'
  return line
}

/**
 * Attach a controller to a host element and return the disposer. The canvas
 * fills the element; the panel is bound to `sessionId` for its whole life.
 */
export async function mountPanel(host: HTMLElement, options: {
  sessionId: string
  ctx: ClientContext
}): Promise<() => void> {
  const canvas = document.createElement('canvas')
  canvas.style.touchAction = 'none'
  const status = statusLine()
  host.append(canvas, status)
  const api = await mountRemote(options.ctx)
  if (!api) {
    status.textContent = 'The viewer panel needs the Web Remote gateway.'
    status.style.display = ''
    return () => { host.replaceChildren() }
  }
  const controller = new PanelController({
    canvas,
    api,
    paint: (frame, target) => paintFrame(frame, target),
    onStatus: (state) => {
      const reason = state.kind === 'refused' || state.kind === 'failed' ? state.reason : null
      status.textContent = reason ?? ''
      status.style.display = reason ? '' : 'none'
    },
  })
  const unsubscribe = controller.subscribe(() => { /* React panels poll `current`; nothing to do here. */ })
  const onResize = (): void => {
    void controller.measureViewport().then(size => controller.setViewport(size.width, size.height))
  }
  const resize = host.ownerDocument.defaultView?.ResizeObserver
  const observer = resize ? new resize(() => onResize()) : null
  observer?.observe(host)
  const onPointerDown = (event: PointerEvent): void => controller.pointerDown(event.offsetX, event.offsetY)
  const onPointerMove = (event: PointerEvent): void => {
    if (event.buttons !== 0) controller.pointerMove(event.offsetX, event.offsetY)
  }
  const onPointerUp = (event: PointerEvent): void => controller.pointerUp(event.offsetX, event.offsetY)
  const onWheel = (event: WheelEvent): void => {
    event.preventDefault()
    controller.wheel(event.offsetX, event.offsetY, event.deltaY)
  }
  canvas.addEventListener('pointerdown', onPointerDown)
  canvas.addEventListener('pointermove', onPointerMove)
  canvas.addEventListener('pointerup', onPointerUp)
  canvas.addEventListener('wheel', onWheel, { passive: false })
  void loadSarasa().then(() => controller.repaint())
  await controller.open(options.sessionId)
  return () => {
    observer?.disconnect()
    unsubscribe()
    canvas.removeEventListener('pointerdown', onPointerDown)
    canvas.removeEventListener('pointermove', onPointerMove)
    canvas.removeEventListener('pointerup', onPointerUp)
    canvas.removeEventListener('wheel', onWheel)
    controller.close()
    host.replaceChildren()
  }
}

/** One panel component, used by both the sidebar tab and the `main` page. */
export function makePanelComponent(ctx: ClientContext): (props: ClientPanelProps) => unknown {
  return function UmlViewerPanel(props: ClientPanelProps): unknown {
    const disposeRef: { current: (() => void) | null } = { current: null }
    const attach = (element: HTMLElement | null): void => {
      if (!element) {
        disposeRef.current?.()
        disposeRef.current = null
        return
      }
      if (disposeRef.current) return
      const sessionId = props.sessionId
      if (typeof sessionId !== 'string') return
      void mountPanel(element, { sessionId, ctx }).then(dispose => { disposeRef.current = dispose })
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

function registerPanels(ctx: ClientContext): void {
  const Panel = makePanelComponent(ctx)
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
      { name: 'sidebar.right.pane.tab', key: TAB_ID, locale: 'dsh-uml-viewer' },
      props => Panel(props as ClientPanelProps),
    )))
    own(ctx.slots.inject('sidebar.right.pane.tab.title', () => ctx.slots.register(
      { name: 'sidebar.right.pane.tab.title', key: TAB_ID },
      () => createElement('span', null, 'UML'),
    )))
    own(ctx.slots.inject('main', () => ctx.slots.register(
      { name: 'main', key: PANEL_PATH, locale: 'dsh-uml-viewer' },
      props => Panel(props as ClientPanelProps),
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
