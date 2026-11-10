import { createElement as h, useEffect, useRef, useState } from 'react'
import type { CardItem, FrameColor } from '@dsh-uml-viewer/contracts'
import { OverlayController, type OverlaySnapshot, type SourceWindow } from './overlays.js'
import { resolveTheme, type Theme } from './theme.js'
import { ENGLISH_TEXT, type PanelText } from './locale.js'

/** The controller's panel text; English when a caller supplies none. */
function textOf(controller: OverlayController): PanelText {
  return (controller as Partial<OverlayController>).text ?? ENGLISH_TEXT
}

/** Map the Rust-provided semantic role through the Canvas palette too. */
export function roleStyle(color: FrameColor, theme: Theme = resolveTheme(document.body)): string {
  return theme.frame(color.role, color.grade, color.ratio)
}

export const OVERLAY_STYLE = `
.dsh-uml-overlay-root { position: absolute; inset: 0; pointer-events: none; font: 12px/1.5 var(--dsw-font-family, system-ui); color: var(--dsw-alias-label-primary); }
.dsh-uml-overlay-root button { font: inherit; color: inherit; background: transparent; border: 1px solid var(--dsw-alias-border-l2); border-radius: 4px; padding: 4px 8px; text-align: left; cursor: pointer; }
.dsh-uml-overlay-root button:hover { background: var(--dsw-alias-interactive-bg-hover); }
.dsh-uml-overlay-root button:focus-visible { outline: 2px solid var(--dsw-alias-state-business-primary); outline-offset: 1px; }
.dsh-uml-window { pointer-events: auto; position: absolute; box-sizing: border-box; display: flex; flex-direction: column; border: 1px solid var(--dsw-alias-border-l2); border-radius: 6px; background: var(--dsw-alias-bg-layer-2); overflow: hidden; resize: both; min-width: 220px; min-height: 150px; max-width: 100%; max-height: 96%; }
.dsh-uml-window-header { flex-shrink: 0; display: flex; align-items: center; gap: 8px; padding: 8px 12px; border-bottom: 1px solid var(--dsw-alias-border-l2); background: var(--dsw-alias-bg-layer-3); }
.dsh-uml-window-header strong { flex: 1; overflow-wrap: anywhere; font-size: 13px; }
.dsh-uml-card { left: 12px; top: 40px; width: 650px; height: 700px; max-width: calc(100% - 12px); }
.dsh-uml-card-scroll { flex: 1; min-height: 0; overflow: auto; overscroll-behavior: contain; position: relative; }
.dsh-uml-card-plan { position: relative; min-width: 560px; font: 12px/18px "UML Viewer Sarasa", monospace; }
.dsh-uml-card-row { position: absolute; left: 0; right: 0; overflow: hidden; }
.dsh-uml-card-row[data-stripe=true] { background: var(--dsw-alias-bg-layer-3); }
.dsh-uml-card-row button { border: 0; padding: 0; border-radius: 0; width: 100%; height: 100%; position: relative; font: inherit; }
.dsh-uml-card-row button:hover { background: var(--dsw-alias-interactive-bg-hover); }
.dsh-uml-card-text,.dsh-uml-card-cell { position: absolute; top: 0; height: 100%; white-space: nowrap; }
.dsh-uml-card-text { overflow: hidden; }
.dsh-uml-card-bar { position: absolute; bottom: 0; height: 3px; background: var(--dsw-alias-border-l2); }
.dsh-uml-card-bar span { display: block; height: 100%; }
.dsh-uml-source { left: 24px; top: 24px; width: 760px; height: 620px; }
.dsh-uml-source-scroll { flex: 1; min-height: 0; overflow: auto; overscroll-behavior: contain; font: 13px/20px "UML Viewer Sarasa", monospace; }
.dsh-uml-source-line { display: flex; min-width: max-content; height: 20px; white-space: pre; }
.dsh-uml-source-line[data-marked=true] { background: var(--dsw-alias-state-business-tertiary); }
.dsh-uml-source-gutter { position: sticky; left: 0; flex: 0 0 52px; text-align: right; box-sizing: border-box; padding-right: 12px; user-select: none; color: var(--dsw-alias-label-tertiary); background: var(--dsw-alias-bg-layer-3); }
.dsh-uml-source-line code { font: inherit; padding-right: 16px; }
.dsh-uml-source-line ::selection { background: var(--dsw-alias-state-business-tertiary); }
.dsh-uml-source-line [data-token=keyword] { color: var(--dsw-alias-state-business-primary); }
.dsh-uml-source-line [data-token=string] { color: var(--dsw-alias-state-warn-primary); }
.dsh-uml-source-line [data-token=comment] { color: var(--dsw-alias-label-tertiary); }
.dsh-uml-overlay-failure { pointer-events: auto; position: absolute; bottom: 8px; left: 12px; background: var(--dsw-alias-bg-layer-2); color: var(--dsw-alias-state-error-primary); }
@media (max-width: 600px) { .dsh-uml-card,.dsh-uml-source { left: 0; max-width: 100%; } }
`

function CardRow({ item, id, controller, theme }: { item: CardItem; id: string; controller: OverlayController; theme: Theme }): unknown {
  const contents = [
    item.swatch ? h('span', { key: 'swatch', style: { position: 'absolute', left: 16, top: 4, width: 10,
      height: item.h - 8, background: roleStyle(item.swatch, theme) } }) : null,
    item.text === null ? null : h('span', { key: 'text', className: 'dsh-uml-card-text', style: {
      left: item.x, right: item.clipRight === null ? 0 : undefined,
      width: item.clipRight === null ? undefined : item.clipRight - item.x,
      color: roleStyle(item.color, theme), fontSize: item.titleFont ? 16 : 12,
    } }, item.text),
    ...item.cells.map((cell, n) => h('span', { key: `cell-${n}`, className: 'dsh-uml-card-cell', title: cell.tip ?? undefined,
      style: { left: cell.left, width: cell.right - cell.left, textAlign: cell.center ? 'center' : 'right',
        color: roleStyle(cell.color, theme) } }, cell.text)),
    ...item.bars.map((bar, n) => h('span', { key: `bar-${n}`, className: 'dsh-uml-card-bar',
      style: { left: bar.left, width: bar.right - bar.left } }, h('span', {
      style: { width: `${bar.fraction * 100}%`, background: roleStyle(bar.color, theme) },
    }))),
  ]
  return h('div', { className: 'dsh-uml-card-row', 'data-stripe': item.stripe,
    style: { top: item.y, height: item.h } }, item.clickable ? h('button', {
    type: 'button', 'data-member': item.opName ?? undefined, 'data-rel': item.rel ?? undefined,
    'data-card-module': item.module ? '' : undefined,
    'aria-label': item.text ?? item.opName ?? textOf(controller).t('card.member'),
    onClick: () => { void (item.rel ? controller.locate(item.rel) : controller.openSource(id, item.opName ?? undefined)) },
  }, ...contents) : contents)
}

export function ClassCard({ state, controller, theme = resolveTheme(document.body) }: { state: OverlaySnapshot; controller: OverlayController; theme?: Theme }): unknown {
  const model = state.card
  const scrollRef = useRef<HTMLElement | null>(null)
  const nativeScrolling = useRef(false)
  const scrollTimer = useRef<ReturnType<typeof setTimeout> | null>(null)
  const [width, setWidth] = useState(624)
  useEffect(() => {
    const element = scrollRef.current
    if (!element || !model) return
    if (!nativeScrolling.current) element.scrollTop = model.scroll
  }, [model])
  useEffect(() => {
    const element = scrollRef.current
    if (!element) return
    const measure = () => {
      const next = Math.max(560, element.clientWidth - 16)
      controller.setCardGeometry({ width: next, viewHeight: element.clientHeight })
      setWidth(next)
      void controller.refresh()
    }
    const wheel = (event: WheelEvent) => {
      if (event.shiftKey || Math.abs(event.deltaX) > Math.abs(event.deltaY)) return
      event.preventDefault()
      event.stopPropagation()
      if (scrollTimer.current !== null) clearTimeout(scrollTimer.current)
      nativeScrolling.current = false
      void controller.syncNativeScroll(element.scrollTop)
        .then(() => controller.scrollCard(Math.sign(event.deltaY)))
    }
    element.addEventListener('wheel', wheel, { passive: false })
    const observer = new ResizeObserver(measure)
    observer.observe(element)
    measure()
    return () => {
      observer.disconnect()
      element.removeEventListener('wheel', wheel)
      if (scrollTimer.current !== null) clearTimeout(scrollTimer.current)
    }
  }, [controller, model?.id])
  if (!model) return null
  return h('section', { className: 'dsh-uml-window dsh-uml-card', role: 'dialog', 'aria-label': textOf(controller).t('card.label', { title: model.title }) },
    h('header', { className: 'dsh-uml-window-header' }, h('strong', null, model.title),
      h('button', { type: 'button', 'aria-label': textOf(controller).t('card.close'), onClick: () => { void controller.closeCard() } }, '×')),
    h('div', { className: 'dsh-uml-card-scroll', ref: scrollRef, tabIndex: 0,
      onScroll: (event: { currentTarget: HTMLElement }) => {
        const offset = event.currentTarget.scrollTop
        if (Math.abs(offset - model.scroll) > 0.5) {
          nativeScrolling.current = true
          if (scrollTimer.current !== null) clearTimeout(scrollTimer.current)
          // Let native keyboard/scrollbar movement settle before publishing its
          // final offset, so producer replies cannot interrupt browser scrolling.
          scrollTimer.current = setTimeout(() => {
            nativeScrolling.current = false
            scrollTimer.current = null
            void controller.syncNativeScroll(offset)
          }, 100)
        }
      },
    }, h('div', { className: 'dsh-uml-card-plan', style: { width, height: model.contentHeight } },
      ...model.items.map((item, index) => h(CardRow, { key: index, item, id: model.id, controller, theme })))))
}

/** Source rows are a fixed 20 CSS px (`.dsh-uml-source-line`). */
export const SOURCE_ROW_HEIGHT = 20
/** Rows rendered beyond each edge of the visible range. */
export const SOURCE_OVERSCAN_ROWS = 30
/** Hard ceiling on rows in the DOM per source window, whatever the box height. */
export const SOURCE_MAX_RENDERED_ROWS = 240
/** Box height assumed before the first measurement. */
const SOURCE_DEFAULT_VIEW = 600

/**
 * The bounded `[start, end)` row range a source window renders. Only the
 * visible rows plus overscan become DOM, so the node count is independent of
 * the file length (the producer's row limit bounds the model itself).
 */
export function sourceRowWindow(rowCount: number, scrollTop: number, viewHeight: number): { start: number; end: number } {
  const top = Number.isFinite(scrollTop) ? Math.max(0, scrollTop) : 0
  const height = Number.isFinite(viewHeight) && viewHeight > 0 ? viewHeight : SOURCE_DEFAULT_VIEW
  const visible = Math.ceil(height / SOURCE_ROW_HEIGHT) + 1
  // Past the end (stale offset) still shows the tail rather than nothing.
  const first = Math.min(Math.floor(top / SOURCE_ROW_HEIGHT), Math.max(0, rowCount - visible))
  const start = Math.max(0, Math.min(rowCount, first - SOURCE_OVERSCAN_ROWS))
  const end = Math.min(rowCount, start + Math.min(SOURCE_MAX_RENDERED_ROWS, visible + 2 * SOURCE_OVERSCAN_ROWS))
  return { start, end }
}

/** Scroll offset that puts the marked declaration row a quarter down the box. */
export function sourceJumpOffset(rows: readonly { number: number }[], line: number | null, viewHeight: number): number {
  if (line === null) return 0
  const index = rows.findIndex(row => row.number === line)
  return Math.max(0, (index < 0 ? 0 : index) * SOURCE_ROW_HEIGHT - viewHeight / 4)
}

export function SourceView({ window, controller, index }: { window: SourceWindow; controller: OverlayController; index: number }): unknown {
  const scrollRef = useRef<HTMLElement | null>(null)
  const model = window.model
  const [view, setView] = useState<{ top: number; height: number }>({
    top: sourceJumpOffset(model.rows, model.line, SOURCE_DEFAULT_VIEW), height: SOURCE_DEFAULT_VIEW })
  useEffect(() => {
    const element = scrollRef.current
    if (!element) return
    const top = sourceJumpOffset(model.rows, model.line, element.clientHeight)
    element.scrollTop = top
    setView({ top: element.scrollTop, height: element.clientHeight })
  }, [model])
  const rows = sourceRowWindow(model.rows.length, view.top, view.height)
  return h('section', { className: 'dsh-uml-window dsh-uml-source', role: 'dialog',
    'aria-label': textOf(controller).t('source.label', { title: model.title }), style: { left: index * 16, top: 16 + index * 16, zIndex: index + 2, maxWidth: `calc(100% - ${index * 16}px)` } },
    h('header', { className: 'dsh-uml-window-header' }, h('strong', null, model.title),
      h('button', { type: 'button', 'aria-label': textOf(controller).t('source.close'), onClick: () => controller.closeSource(window.serial) }, '×')),
    h('div', { className: 'dsh-uml-source-scroll', ref: scrollRef, 'data-source-rows': model.rows.length,
      onScroll: (event: { currentTarget: HTMLElement }) => {
        const element = event.currentTarget
        setView({ top: element.scrollTop, height: element.clientHeight })
      } },
    h('div', { key: 'before', 'aria-hidden': true, style: { height: rows.start * SOURCE_ROW_HEIGHT } }),
    ...model.rows.slice(rows.start, rows.end).map(row =>
      h('div', { key: row.number, className: 'dsh-uml-source-line', 'data-source-line': row.number, 'data-marked': row.marked },
        h('span', { className: 'dsh-uml-source-gutter', 'aria-hidden': true }, String(row.number)),
        h('code', null, ...row.tokens.map((token, n) => h('span', { key: n, 'data-token': token.kind }, token.text))))),
    h('div', { key: 'after', 'aria-hidden': true, style: { height: (model.rows.length - rows.end) * SOURCE_ROW_HEIGHT } })))
}

export function Overlays({ controller }: { controller: OverlayController }): unknown {
  const [state, setState] = useState<OverlaySnapshot>(controller.current)
  const root = useRef<HTMLElement | null>(null)
  const [theme, setTheme] = useState<Theme>(resolveTheme(document.body))
  useEffect(() => {
    const update = () => setTheme(resolveTheme(root.current ?? document.body))
    update()
    const observer = new MutationObserver(update)
    observer.observe(document.body, { attributes: true, attributeFilter: ['class', 'style', 'data-theme', 'data-ds-dark-theme'] })
    return () => observer.disconnect()
  }, [])
  useEffect(() => {
    setState(controller.current)
    return controller.subscribe(() => setState(controller.current))
  }, [controller])
  // A DSH language switch re-renders the labels without reopening the tab.
  const [, setLanguage] = useState(textOf(controller).language)
  useEffect(() => textOf(controller).subscribe(() => setLanguage(textOf(controller).language)), [controller])
  return h('div', { className: 'dsh-uml-overlay-root', ref: root },
    h(ClassCard, { state, controller, theme }),
    ...state.sources.map((window, index) => h(SourceView, { key: window.serial, window, controller, index })),
    state.failure ? h('div', { role: 'status', className: 'dsh-uml-overlay-failure' }, state.failure) : null)
}
