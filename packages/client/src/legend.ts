/**
 * The diagram's legend (#50, ADR 0015): what the boxes, the quality colors and
 * the relation colors mean. The rows are the resident's text — the frame's
 * `legend` field, typeset in Rust from the language table (#44) — so a DSH
 * language switch rewords them with the next frame; this module only lays them
 * out as DOM and resolves each sample's semantic role.
 *
 * Where it lives follows #49's layout (`layout.ts`), never the slot the panel
 * sits in: below 900px of *container* width the legend is a popover opened from
 * 「更多」 (`more-menu.ts`), and from 900px it is resident. A resident legend
 * takes a strip of the canvas row below the diagram, so the canvas — and the
 * resident's viewport — shrink with it and the legend never covers a class.
 * The docked card likewise reserves a right-hand column from the drawing stage
 * (F28). Below
 * the diagram the strip keeps the drawing's full width, which a side column
 * would take from it; the overlay root stops above it (index.ts), so a docked
 * drawer never covers it either.
 *
 * Colors go through `theme.ts`'s resolver, the one the canvas paints with, so a
 * legend swatch is the diagram's own color in both light and dark mode. The
 * host's UI primitives are never loaded (ADR 0013).
 */
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
/* Below 900px: a popover over the canvas, opened from 「更多」. */
.dsh-uml-legend[data-mode=popover][data-open=true] {
  display: flex; pointer-events: auto; position: absolute; z-index: 40; top: 8px; left: 8px; right: 8px;
  max-height: calc(100% - 16px); padding: 8px 12px 12px;
  border: 1px solid var(--dsw-alias-border-l2); border-radius: 6px;
  background: var(--dsw-alias-bg-overlay); box-shadow: var(--dsw-shadow-lv2, 0 4px 16px rgb(0 0 0 / 20%));
}
/* From 900px the row keeps a strip for it below the diagram: the canvas and
   the resident's viewport shrink instead of the legend covering a class. */
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
  /** Injected for tests; defaults to the host theme the canvas paints with. */
  theme?: () => Theme
  /** Where the keyboard goes after the popover closed, so shortcuts keep working. */
  onDone?: () => void
}

export interface Legend {
  readonly element: HTMLElement
  /** Follow the panel stream: the rows are in every frame, in its language. */
  render(frame: SceneFrame | null): void
  /** What `layout.ts` measured: a popover when narrow, resident from 900px. */
  setLayout(layout: PanelLayout): void
  /** True when the legend has its own reserved strip (F45). */
  isResident(): boolean
  /** The 「更多」 entry: open the popover (narrow), nothing when it is resident. */
  show(): void
  open(): void
  close(): void
  /** Whether Esc has a popover to close; a resident legend is not a layer. */
  isOpen(): boolean
  dispose(): void
}

let counter = 0

/** Attach the legend to the canvas row: the reserved strip, or the popover. */
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
  // The frame's legend is pure data. Compare its content, not the object
  // identity: a status publish with an equivalent legend should not rebuild
  // ~11 rows of DOM (F55).
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
    // 关系淡化 multiplies every channel, so a faded swatch is an opacity.
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
          // A relation line's sample, at the width the diagram draws it.
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
        // An additive sample kind the panel does not know: the row keeps its
        // label and draws no swatch, like an unknown frame item kind.
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

  /** The frame's rows in the host's colors; a theme switch repaints them. */
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
    // The close button that closed it is hidden now; the canvas takes the
    // keyboard back so Esc and the other shortcuts still reach the panel.
    if (layout !== 'docked') options.onDone?.()
  }
  const onClose = (): void => close()
  const onKey = (event: KeyboardEvent): void => {
    if (event.key !== 'Escape' || !open || layout === 'docked') return
    event.preventDefault()
    event.stopPropagation()
    close()
  }
  // A host theme switch recolors every swatch, as it repaints the canvas.
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
      // Rows are the frame's, and only their content, language or the dim
      // alpha changes them; a new frame object with the same legend repaints
      // nothing (F55).
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

/** The 「更多」 entry that opens the legend; a resident legend needs none (F45). */
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
