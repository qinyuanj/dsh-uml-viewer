import type { LegendRow, LegendSample, SceneFrame } from '@dsh-uml-viewer/contracts'
import type { PanelLayout } from './layout.js'
import { ENGLISH_TEXT, type PanelText } from './locale.js'
import type { MoreMenuEntry } from './more-menu.js'
import { resolveTheme, type Theme } from './theme.js'

export const LEGEND_STYLE = `
.dsh-uml-legend {
  box-sizing: border-box; flex-direction: column; min-height: 0; overflow: auto; overscroll-behavior: contain;
  color: var(--dsw-alias-label-primary); font: 12px/1.5 var(--dsw-font-family, system-ui);
}
.dsh-uml-legend[hidden] { display: none; }
.dsh-uml-legend[data-mode=popover][data-open=true] {
  display: flex; pointer-events: auto; position: absolute; z-index: 40; top: 8px; left: 8px; right: 8px;
  max-height: calc(100% - 16px); padding: 8px 12px 12px;
  border: 1px solid var(--dsw-alias-border-l2); border-radius: 6px;
  background: var(--dsw-alias-bg-overlay); box-shadow: var(--dsw-shadow-lv2, 0 4px 16px rgb(0 0 0 / 20%));
}
.dsh-uml-viewer-panel[data-layout=docked] .dsh-uml-viewer-canvas-row { flex-direction: column; }
.dsh-uml-viewer-panel[data-layout=docked] .dsh-uml-legend {
  display: flex; flex: 0 0 auto; width: 100%; max-height: 45%;
  padding: 6px 12px 8px; border-top: 1px solid var(--dsw-alias-border-l2);
  background: var(--dsw-alias-bg-layer-2);
}
.dsh-uml-viewer-panel[data-layout=docked] .dsh-uml-legend-close { display: none; }
.dsh-uml-viewer-panel[data-layout=docked] .dsh-uml-legend-body { display: flex; flex-wrap: wrap; align-content: flex-start; gap: 0 24px; }
.dsh-uml-viewer-panel[data-layout=docked] .dsh-uml-legend-section { flex: 1 1 220px; max-width: 33%; }
.dsh-uml-viewer-panel[data-layout=docked] .dsh-uml-legend-header { margin-bottom: 2px; }
.dsh-uml-legend-header { flex: 0 0 auto; display: flex; align-items: center; gap: 8px; margin-bottom: 4px; }
.dsh-uml-legend-title { flex: 1 1 auto; margin: 0; font: inherit; font-weight: 600; }
.dsh-uml-legend-close {
  flex: 0 0 auto; appearance: none; border: 1px solid transparent; border-radius: 4px; padding: 0 6px;
  background: transparent; color: var(--dsw-alias-label-secondary); font: inherit; cursor: pointer;
}
.dsh-uml-legend-close:hover { background: var(--dsw-alias-interactive-bg-hover); color: var(--dsw-alias-label-primary); }
.dsh-uml-legend-close:focus-visible { outline: var(--dsw-focus-ring-width, 2px) solid var(--dsw-focus-ring-color, var(--dsw-alias-state-business-primary)); outline-offset: 1px; }
.dsh-uml-legend-body { flex: 1 1 auto; min-height: 0; }
.dsh-uml-legend-section { margin: 0 0 8px; }
.dsh-uml-legend-section-title { margin: 0 0 2px; font: inherit; font-weight: 600; color: var(--dsw-alias-label-secondary); }
.dsh-uml-legend-rows { margin: 0; padding: 0; list-style: none; }
.dsh-uml-legend-row { display: flex; align-items: flex-start; gap: 8px; padding: 1px 0; }
.dsh-uml-legend-mark { flex: 0 0 auto; display: flex; align-items: center; gap: 2px; height: 18px; }
.dsh-uml-legend-mark span { display: block; box-sizing: border-box; }
.dsh-uml-legend-label { flex: 1 1 auto; min-width: 0; overflow-wrap: anywhere; }
`

export interface LegendOptions {
  text?: PanelText
  theme?: () => Theme
  onDone?: () => void
}

export interface Legend {
  readonly element: HTMLElement
  render(frame: SceneFrame | null): void
  setLayout(layout: PanelLayout): void
  isResident(): boolean
  show(): void
  open(): void
  close(): void
  isOpen(): boolean
  dispose(): void
}

let counter = 0

export function mountLegend(host: HTMLElement, options: LegendOptions = {}): Legend {
  const text = options.text ?? ENGLISH_TEXT
  const doc = host.ownerDocument
  const themeOf = options.theme ?? (() => resolveTheme(element))
  const titleId = `dsh-uml-viewer-legend-${++counter}`
  const element = doc.createElement('section')
  element.className = 'dsh-uml-legend'
  element.setAttribute('role', 'region')
  element.setAttribute('aria-labelledby', titleId)
  element.hidden = true
  const header = doc.createElement('div')
  header.className = 'dsh-uml-legend-header'
  const title = doc.createElement('h2')
  title.className = 'dsh-uml-legend-title'
  title.id = titleId
  const closeButton = doc.createElement('button')
  closeButton.type = 'button'
  closeButton.className = 'dsh-uml-legend-close'
  header.append(title, closeButton)
  const body = doc.createElement('div')
  body.className = 'dsh-uml-legend-body'
  element.append(header, body)
  host.appendChild(element)

  let layout: PanelLayout = 'drawer'
  let open = false
  let dimAlpha = 70
  let frame: SceneFrame | null = null
  let legendSignature = ''

  const mark = (sample: LegendSample, theme: Theme): HTMLElement => {
    const holder = doc.createElement('span')
    holder.className = 'dsh-uml-legend-mark'
    holder.dataset.kind = sample.kind
    holder.setAttribute('aria-hidden', 'true')
    const chip = (apply: (chip: HTMLElement) => void): void => {
      const part = doc.createElement('span')
      apply(part)
      holder.appendChild(part)
    }
    const faded = (): string | null => (dimAlpha < 255 ? String(dimAlpha / 255) : null)
    const box = (width: string, height: string, radius: string, fill: string, stroke: string, dimmed: boolean): void => {
      chip(part => {
        part.style.width = width
        part.style.height = height
        part.style.borderWidth = '2px'
        part.style.borderStyle = 'solid'
        part.style.borderRadius = radius
        part.style.background = fill
        part.style.borderColor = stroke
        const opacity = dimmed ? faded() : null
        if (opacity) part.style.opacity = opacity
      })
    }
    switch (sample.kind) {
      case 'box':
        box('26px', '16px', sample.shape === 'oval' ? '50%' : '3px',
          theme.frame(sample.fill.role, sample.fill.grade, sample.fill.ratio),
          theme.frame(sample.stroke.role, sample.stroke.grade, sample.stroke.ratio), sample.dimmed)
        break
      case 'line':
        chip(part => {
          part.style.width = '26px'
          part.style.height = '0'
          part.style.borderTopWidth = '2px'
          part.style.borderTopStyle = 'solid'
          part.style.borderTopColor = theme.frame(sample.stroke.role, sample.stroke.grade, sample.stroke.ratio)
          const opacity = sample.dimmed ? faded() : null
          if (opacity) part.style.opacity = opacity
        })
        break
      case 'dot':
        chip(part => {
          part.style.width = '12px'
          part.style.height = '12px'
          part.style.borderRadius = '50%'
          part.style.background = theme.frame(sample.fill.role, sample.fill.grade, sample.fill.ratio)
        })
        break
      case 'ramp':
        for (const stop of sample.stops) {
          chip(part => {
            part.style.width = '12px'
            part.style.height = '16px'
            part.style.borderRadius = '2px'
            part.style.background = theme.frame(stop.role, stop.grade, stop.ratio)
          })
        }
        break
      default:
        break
    }
    return holder
  }

  const row = (entry: LegendRow, theme: Theme): HTMLElement => {
    const item = doc.createElement('li')
    item.className = 'dsh-uml-legend-row'
    item.dataset.row = entry.id
    const label = doc.createElement('span')
    label.className = 'dsh-uml-legend-label'
    label.textContent = entry.label
    item.append(mark(entry.sample, theme), label)
    return item
  }

  const paint = (): void => {
    const theme = themeOf()
    dimAlpha = frame?.style.dimAlpha ?? dimAlpha
    body.replaceChildren()
    for (const section of frame?.legend.sections ?? []) {
      const group = doc.createElement('section')
      group.className = 'dsh-uml-legend-section'
      group.dataset.section = section.id
      const heading = doc.createElement('h3')
      heading.className = 'dsh-uml-legend-section-title'
      heading.textContent = section.title
      const rows = doc.createElement('ul')
      rows.className = 'dsh-uml-legend-rows'
      for (const entry of section.rows) rows.appendChild(row(entry, theme))
      group.append(heading, rows)
      body.appendChild(group)
    }
    apply()
  }
  const apply = (): void => {
    element.dataset.mode = layout === 'docked' ? 'docked' : 'popover'
    element.dataset.open = String(open && layout !== 'docked')
    element.hidden = !hasRows() || (layout !== 'docked' && !open)
    closeButton.hidden = layout === 'docked'
  }
  const hasRows = (): boolean => (frame?.legend.sections.length ?? 0) > 0
  const label = (): void => {
    title.textContent = text.t('legend.title')
    closeButton.textContent = '×'
    closeButton.setAttribute('aria-label', text.t('legend.close'))
    closeButton.title = text.t('legend.close')
  }
  const close = (): void => {
    if (!open) return
    open = false
    apply()
    if (layout !== 'docked') options.onDone?.()
  }
  const onClose = (): void => close()
  const onKey = (event: KeyboardEvent): void => {
    if (event.key !== 'Escape' || !open || layout === 'docked') return
    event.preventDefault()
    event.stopPropagation()
    close()
  }
  const themeRoot = doc.body as HTMLElement | null
  const Observer = doc.defaultView?.MutationObserver
  const observer = themeRoot && Observer ? new Observer(() => { if (hasRows()) paint() }) : null
  observer?.observe(themeRoot as HTMLElement, { attributes: true, attributeFilter: ['data-ds-dark-theme', 'data-theme'] })
  closeButton.addEventListener('click', onClose)
  element.addEventListener('keydown', onKey)
  label()
  paint()
  const stopText = text.subscribe(() => {
    label()
    closeButton.setAttribute('aria-label', text.t('legend.close'))
  })
  return {
    element,
    render(next) {
      const signature = next === null ? '' : JSON.stringify(next.legend)
      const changed = signature !== legendSignature
        || (next?.style.dimAlpha ?? dimAlpha) !== dimAlpha
      legendSignature = signature
      frame = next
      if (changed) paint()
      else apply()
    },
    setLayout(next) {
      if (next === layout) return
      layout = next
      apply()
    },
    isResident: () => layout === 'docked',
    show() {
      if (layout === 'docked') return
      open = true
      apply()
    },
    open() {
      open = true
      apply()
    },
    close,
    isOpen: () => open && layout !== 'docked',
    dispose() {
      stopText()
      observer?.disconnect()
      closeButton.removeEventListener('click', onClose)
      element.removeEventListener('keydown', onKey)
      element.remove()
    },
  }
}

export function legendEntries(legend: Legend, text: PanelText): MoreMenuEntry[] {
  if (legend.isResident()) return []
  return [
    {
      id: 'legend',
      label: () => text.t('legend.title'),
      run: () => legend.show(),
    },
  ]
}
