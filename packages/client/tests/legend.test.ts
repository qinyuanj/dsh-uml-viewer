import { readFileSync } from 'node:fs'
import { describe, expect, it } from 'vitest'
import type { SceneFrame } from '@dsh-uml-viewer/contracts'
import { ENGLISH_TEXT } from '../src/locale.js'
import { legendEntries, mountLegend } from '../src/legend.js'
import { Element, makeDocument } from './dom.js'
import type { Theme } from '../src/theme.js'

// The producer's own goldens carry the legend, in the resident's language.
const frame = JSON.parse(readFileSync(new URL('../../../crates/frame/tests/fixtures/frames/root.en.frame.json', import.meta.url), 'utf8')) as SceneFrame
const chinese = JSON.parse(readFileSync(new URL('../../../crates/frame/tests/fixtures/frames/root.frame.json', import.meta.url), 'utf8')) as SceneFrame

/** Every role resolves to its name and grade, so a swatch's color is readable. */
const theme: Theme = {
  mode: 'dark',
  frame: (role, grade) => `${role}${grade === null || grade === undefined ? '' : `:${grade}`}`,
}

function harness() {
  const doc = makeDocument()
  const panel = new Element('div', doc)
  panel.className = 'dsh-uml-viewer-panel'
  const legend = mountLegend(panel as unknown as HTMLElement, { text: ENGLISH_TEXT, theme: () => theme })
  const root = legend.element as unknown as Element
  const rows = (): Element[] => root.descendants()
    .filter(element => element.className === 'dsh-uml-legend-row')
  const row = (id: string): Element => {
    const found = rows().find(element => element.dataset.row === id)
    if (!found) throw new Error(`no legend row ${id}`)
    return found
  }
  return { panel, legend, root, rows, row }
}

describe('the panel legend (#50)', () => {
  it('draws the rows the resident typeset, section by section', () => {
    const { legend, root, rows, row } = harness()
    legend.render(frame)
    const sections = root.descendants()
      .filter(element => element.dataset.section !== undefined)
    expect(sections.map(section => section.dataset.section)).toEqual(['shapes', 'quality', 'relations'])
    expect(rows()).toHaveLength(frame.legend.sections.reduce((count, section) => count + section.rows.length, 0))
    // A row's label is the resident's text, not the panel's locale files.
    expect(row('relation').children[1]!.textContent)
      .toBe('Relation: a grey-green solid line; the head shape names its kind')
    expect(row('no-data').children[1]!.textContent).toBe('No data: CRAP or mutation data is missing')
    // The legend's own title is panel text, so it follows the DSH language.
    expect(root.find('h2').textContent).toBe('Legend')
  })

  it('resolves every swatch through the same theme the canvas paints with', () => {
    const { legend, root, row } = harness()
    legend.render(frame)
    const mark = (id: string): Element => row(id).children[0]!.children[0]!
    expect(mark('class-box').style.background).toBe('quality-fill:3')
    expect(mark('class-box').style.borderColor).toBe('quality-stroke:3')
    expect(mark('class-box').style.borderRadius).toBe('3px')
    expect(mark('component-box').style.borderColor).toBe('package-stroke:7')
    expect(mark('foreign-package').style.borderRadius).toBe('50%')
    expect(mark('relation').style.borderTopColor).toBe('relation')
    expect(mark('violation').style.borderTopColor).toBe('violation')
    expect(mark('relation-hover').style.borderTopColor).toBe('relation-hover')
    expect(mark('dependency-chain').style.borderTopColor).toBe('dependency-chain')
    expect(mark('no-data').style.background).toBe('no-data')
    // 关系淡化 is the same color, faded by the frame's own dim alpha.
    expect(mark('dimmed').style.borderTopColor).toBe('relation')
    expect(mark('dimmed').style.opacity).toBe(String(70 / 255))
    // The 质量色 ramp shows its stops in order, worst first.
    expect(row('quality-ramp').children[0]!.children.map(chip => chip.style.background))
      .toEqual(['quality-fill:0', 'quality-fill:5', 'quality-fill:10'])
  })

  it('follows a new frame, so a language switch rewords the rows', () => {
    const { legend, root, row } = harness()
    legend.render(frame)
    legend.render(chinese)
    expect(root.dataset.mode).toBe('popover')
    expect(row('relation').children[1]!.textContent).toBe('关系连线：灰绿实线，头部形状表示关系种类')
  })

  it('keeps the DOM when a new frame carries the same legend content (F55)', () => {
    const { legend, row } = harness()
    legend.render(frame)
    const before = row('relation')
    legend.render({ ...frame, legend: JSON.parse(JSON.stringify(frame.legend)) })
    expect(row('relation')).toBe(before)
    legend.render(chinese)
    expect(row('relation')).not.toBe(before)
  })

  it('is a popover from 更多 when narrow, and resident from the docked layout', () => {
    const { legend, root } = harness()
    legend.render(frame)
    legend.setLayout('drawer')
    expect(legend.element.hidden).toBe(true)
    expect(root.dataset.mode).toBe('popover')
    expect(legend.isOpen()).toBe(false)
    legend.open()
    expect(legend.isOpen()).toBe(true)
    expect(legend.element.hidden).toBe(false)
    expect(root.dataset.open).toBe('true')
    // A popover closes through its own button, like every other panel layer.
    const close = root.descendants().find(element => element.tagName === 'button')!
    expect(close.getAttribute('aria-label')).toBe('Close legend')
    close.fire('click')
    expect(legend.isOpen()).toBe(false)
    expect(legend.element.hidden).toBe(true)
    // Esc inside the popover closes it and stops there.
    legend.open()
    const escape = root.fire('keydown', { key: 'Escape' })
    expect(escape.defaultPrevented).toBe(true)
    expect(legend.isOpen()).toBe(false)

    // From 900px the legend is always there; opening it is not a state.
    legend.setLayout('docked')
    expect(legend.element.hidden).toBe(false)
    expect(root.dataset.mode).toBe('docked')
    expect(legend.isOpen()).toBe(false)
    legend.show()
    expect(root.dataset.open).toBe('false')
    // Back below 900px it is a popover again, and closed.
    legend.setLayout('drawer')
    expect(legend.isOpen()).toBe(false)
    expect(legend.element.hidden).toBe(true)
  })

  it('offers the 更多 entry only while the legend is a popover (F45)', () => {
    const { legend } = harness()
    expect(legend.isResident()).toBe(false)
    const entries = legendEntries(legend, ENGLISH_TEXT)
    expect(entries.map(entry => entry.id)).toEqual(['legend'])
    expect(entries[0]!.label()).toBe('Legend')
    entries[0]!.run()
    expect(legend.isOpen()).toBe(true)
    legend.close()
    legend.setLayout('docked')
    expect(legend.isResident()).toBe(true)
    expect(legendEntries(legend, ENGLISH_TEXT)).toEqual([])
    expect(legend.isOpen()).toBe(false)
    legend.setLayout('drawer')
    expect(legendEntries(legend, ENGLISH_TEXT).map(entry => entry.id)).toEqual(['legend'])
  })

  it('shows nothing before the first frame', () => {
    const { legend } = harness()
    legend.render(null)
    expect(legend.element.hidden).toBe(true)
    legend.setLayout('docked')
    expect(legend.element.hidden).toBe(true)
    legend.render(chinese)
    expect(legend.element.hidden).toBe(false)
  })
})
