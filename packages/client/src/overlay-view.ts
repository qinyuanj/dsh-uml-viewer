import { createElement as h, useEffect, useRef, useState } from 'react'
import type { CardItem, FrameColor } from '@dsh-uml-viewer/contracts'
import { OverlayController, type OverlaySnapshot, type SourceWindow } from './overlays.js'

/** Overlay chrome uses only host tokens. Quality roles choose semantic host
 * states; figures, grades and text remain the Rust producer's. */
export function roleStyle(color: FrameColor): string {
  const tokens: Record<string, string> = {
    ink: 'label-primary', muted: 'label-tertiary', accent: 'state-business-primary',
    marker: 'label-primary', good: 'state-success-primary', 'no-data': 'state-error-primary',
    violation: 'state-error-primary', coverage: 'state-warn-primary',
    'quality-stroke': 'state-error-primary',
  }
  if (color.role === 'quality-stroke' && color.grade != null) {
    return `var(--dsw-alias-${color.grade >= 8 ? 'state-success-primary' : color.grade >= 5 ? 'state-warn-primary' : 'state-error-primary'})`
  }
  return `var(--dsw-alias-${tokens[color.role] ?? 'label-primary'})`
}

export const OVERLAY_STYLE = `
.dsh-uml-overlay-root { position: absolute; inset: 0; pointer-events: none; font: 12px/1.5 var(--dsw-font-family, system-ui); color: var(--dsw-alias-label-primary); }
.dsh-uml-overlay-root button { font: inherit; color: inherit; background: transparent; border: 1px solid var(--dsw-alias-border-l2); border-radius: 4px; padding: 4px 8px; text-align: left; cursor: pointer; }
.dsh-uml-overlay-root button:hover { background: var(--dsw-alias-interactive-bg-hover); }
.dsh-uml-overlay-root button:focus-visible { outline: 2px solid var(--dsw-alias-state-business-primary); outline-offset: 1px; }
.dsh-uml-inspector { pointer-events: auto; position: absolute; right: 0; top: 0; bottom: 0; width: 230px; max-width: 46%; box-sizing: border-box; background: var(--dsw-alias-bg-layer-3); border-left: 1px solid var(--dsw-alias-border-l2); padding: 12px; display: flex; flex-direction: column; gap: 8px; overflow: hidden; }
.dsh-uml-inspector h2 { font: inherit; font-weight: 600; margin: 0; }
.dsh-uml-proposals { display: flex; flex-direction: column; gap: 4px; max-height: 35%; overflow: auto; flex-shrink: 0; }
.dsh-uml-proposals button[aria-pressed=true] { background: var(--dsw-alias-interactive-bg-active); border-color: var(--dsw-alias-state-business-primary); }
.dsh-uml-summary-lines { overflow: auto; min-height: 0; white-space: pre-wrap; overflow-wrap: anywhere; }
.dsh-uml-summary-lines p { margin: 0 0 8px; }
.dsh-uml-summary-segments { display: flex; flex-wrap: wrap; gap: 2px 8px; margin-bottom: 8px; }
.dsh-uml-summary-segments span { white-space: nowrap; }
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
@media (max-width: 600px) { .dsh-uml-inspector { width: 180px; padding: 8px; } .dsh-uml-card,.dsh-uml-source { left: 0; max-width: 100%; } }
`

export function Inspector({ state, controller }: { state: OverlaySnapshot; controller: OverlayController }): unknown {
  const model = state.inspector
  if (!model) return null
  return h('aside', { className: 'dsh-uml-inspector', 'aria-label': model.labels.inspector },
    h('h2', null, model.labels.inspector),
    h('div', { className: 'dsh-uml-proposals' }, ...model.entries.map((entry, index) => h('div', { key: entry.id ?? 'source' },
      index === 1 ? h('div', { style: { color: 'var(--dsw-alias-label-tertiary)' } }, model.labels.proposals) : null,
      h('button', { type: 'button', 'aria-pressed': entry.active, style: { width: '100%' },
        onClick: () => { void controller.choose(entry.id) } }, entry.label)))),
    h('button', { type: 'button', onClick: () => { void controller.namespaceTree() } }, model.labels.pHint),
    h('button', { type: 'button', onClick: () => { void controller.declutter() } }, model.declutterLabel),
    h('div', { className: 'dsh-uml-summary-lines', 'data-summary-kind': model.summary.kind }, ...model.summary.lines.map((line, index) =>
      line.type === 'label'
        ? h('p', { key: index, style: { color: roleStyle(line.color) } }, line.text)
        : h('div', { key: index, className: 'dsh-uml-summary-segments', style: { color: roleStyle(line.color) } },
          ...line.parts.map((part, n) => h('span', { key: n }, part))))))
}

function CardRow({ item, id, controller }: { item: CardItem; id: string; controller: OverlayController }): unknown {
  const contents = [
    item.swatch ? h('span', { key: 'swatch', style: { position: 'absolute', left: 16, top: 4, width: 10,
      height: item.h - 8, background: roleStyle(item.swatch) } }) : null,
    item.text === null ? null : h('span', { key: 'text', className: 'dsh-uml-card-text', style: {
      left: item.x, right: item.clipRight === null ? 0 : undefined,
      width: item.clipRight === null ? undefined : item.clipRight - item.x,
      color: roleStyle(item.color), fontSize: item.titleFont ? 16 : 12,
    } }, item.text),
    ...item.cells.map((cell, n) => h('span', { key: `cell-${n}`, className: 'dsh-uml-card-cell', title: cell.tip ?? undefined,
      style: { left: cell.left, width: cell.right - cell.left, textAlign: cell.center ? 'center' : 'right',
        color: roleStyle(cell.color) } }, cell.text)),
    ...item.bars.map((bar, n) => h('span', { key: `bar-${n}`, className: 'dsh-uml-card-bar',
      style: { left: bar.left, width: bar.right - bar.left } }, h('span', {
      style: { width: `${bar.fraction * 100}%`, background: roleStyle(bar.color) },
    }))),
  ]
  return h('div', { className: 'dsh-uml-card-row', 'data-stripe': item.stripe,
    style: { top: item.y, height: item.h } }, item.clickable ? h('button', {
    type: 'button', 'data-member': item.opName ?? undefined, 'data-rel': item.rel ?? undefined,
    'data-card-module': item.module ? '' : undefined,
    'aria-label': item.text ?? item.opName ?? '成员',
    onClick: () => { void (item.rel ? controller.locate(item.rel) : controller.openSource(id, item.opName ?? undefined)) },
  }, ...contents) : contents)
}

export function ClassCard({ state, controller }: { state: OverlaySnapshot; controller: OverlayController }): unknown {
  const model = state.card
  const scrollRef = useRef<HTMLElement | null>(null)
  const [width, setWidth] = useState(624)
  useEffect(() => {
    const element = scrollRef.current
    if (!element || !model) return
    element.scrollTop = model.scroll
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
      void controller.scrollCard(Math.sign(event.deltaY))
    }
    element.addEventListener('wheel', wheel, { passive: false })
    const observer = new ResizeObserver(measure)
    observer.observe(element)
    measure()
    return () => { observer.disconnect(); element.removeEventListener('wheel', wheel) }
  }, [controller, model?.id])
  if (!model) return null
  return h('section', { className: 'dsh-uml-window dsh-uml-card', role: 'dialog', 'aria-label': `类卡片 ${model.title}` },
    h('header', { className: 'dsh-uml-window-header' }, h('strong', null, model.title),
      h('button', { type: 'button', 'aria-label': '关闭类卡片', onClick: () => { void controller.closeCard() } }, '×')),
    h('div', { className: 'dsh-uml-card-scroll', ref: scrollRef }, h('div', { className: 'dsh-uml-card-plan', style: { width, height: model.contentHeight } },
      ...model.items.map((item, index) => h(CardRow, { key: index, item, id: model.id, controller })))))
}

export function SourceView({ window, controller, index }: { window: SourceWindow; controller: OverlayController; index: number }): unknown {
  const scrollRef = useRef<HTMLElement | null>(null)
  const model = window.model
  useEffect(() => {
    const element = scrollRef.current
    if (!element) return
    element.scrollTop = model.line === null ? 0 : Math.max(0, (model.line - 1) * 20 - element.clientHeight / 4)
  }, [model])
  return h('section', { className: 'dsh-uml-window dsh-uml-source', role: 'dialog',
    'aria-label': `源码窗口 ${model.title}`, style: { left: index * 16, top: 16 + index * 16, zIndex: index + 2, maxWidth: `calc(100% - ${index * 16}px)` } },
    h('header', { className: 'dsh-uml-window-header' }, h('strong', null, model.title),
      h('button', { type: 'button', 'aria-label': '关闭源码窗口', onClick: () => controller.closeSource(window.serial) }, '×')),
    h('div', { className: 'dsh-uml-source-scroll', ref: scrollRef }, ...model.rows.map(row =>
      h('div', { key: row.number, className: 'dsh-uml-source-line', 'data-source-line': row.number, 'data-marked': row.marked },
        h('span', { className: 'dsh-uml-source-gutter', 'aria-hidden': true }, String(row.number)),
        h('code', null, ...row.tokens.map((token, n) => h('span', { key: n, 'data-token': token.kind }, token.text)))))))
}

export function Overlays({ controller }: { controller: OverlayController }): unknown {
  const [state, setState] = useState<OverlaySnapshot>(controller.current)
  useEffect(() => controller.subscribe(() => setState(controller.current)), [controller])
  return h('div', { className: 'dsh-uml-overlay-root' },
    h(Inspector, { state, controller }), h(ClassCard, { state, controller }),
    ...state.sources.map((window, index) => h(SourceView, { key: window.serial, window, controller, index })),
    state.failure ? h('div', { role: 'status', className: 'dsh-uml-overlay-failure' }, state.failure) : null)
}
