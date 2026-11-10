import { describe, expect, it, vi } from 'vitest'
import type { DiscussionContext } from '@dsh-uml-viewer/contracts'
import { mountContextSwitcher, returnToSourceTree, type SwitcherActions } from '../src/context-switcher.js'
import { panelLanguage, translate, type PanelText } from '../src/locale.js'
import { makeDocument, type Element } from './dom.js'

/** The switcher's own behaviour (#47); the browser walk is `navigation.spec.mjs`. */
const proposals = [{ id: ':p-shop', name: 'Shop split' }, { id: ':p-2', name: '2026-11-09 14:05:06' }]
const real: DiscussionContext = { kind: 'real', edn: '{:context :real}' }
const shop: DiscussionContext = { kind: 'proposal', proposalId: ':p-shop', name: 'Shop split', edn: '{:context :proposal}' }

function zhText(): PanelText {
  const language = panelLanguage('zh')
  return { language, t: (key, params) => translate(language, key, params), subscribe: () => () => {} }
}

function mounted(text?: PanelText) {
  const doc = makeDocument()
  const actions: SwitcherActions = { choose: vi.fn(), namespaceTree: vi.fn(), newProposal: vi.fn() }
  const done = vi.fn()
  const switcher = mountContextSwitcher(doc as unknown as Document, { actions, onDone: done, ...(text ? { text } : {}) })
  const root = switcher.element as unknown as Element
  const trigger = root.children[0]!
  const menu = root.children[1]!
  const rows = () => menu.descendants().filter(el => el.tagName === 'button')
  const open = () => trigger.fire('click')
  return { switcher, actions, done, trigger, menu, rows, open }
}

describe('discussion-context switcher (#47)', () => {
  it('names the discussion context and lists the source diagram, the proposals, then 新建提案', () => {
    const h = mounted(zhText())
    h.switcher.render({ context: shop, proposals })
    expect(h.trigger.attributes['aria-label']).toBe('讨论上下文：Shop split')
    // The menu is a menu only while open: a hidden [role=menu] blocks host commands.
    expect(h.menu.hidden).toBe(true)
    expect(h.menu.hasAttribute('role')).toBe(false)
    h.open()
    expect(h.menu.attributes.role).toBe('menu')
    expect(h.trigger.attributes['aria-expanded']).toBe('true')
    expect(h.rows().map(row => [row.attributes.role, row.descendants().map(el => el.textContent).join(''),
      row.attributes['aria-checked'] ?? null])).toEqual([
      ['menuitemradio', '源码图', 'false'],
      ['menuitemradio', '✓Shop split', 'true'],
      ['menuitemradio', '2026-11-09 14:05:06', 'false'],
      ['menuitem', '新建提案', null],
    ])
  })

  it('chooses another proposal with the context action and closes', () => {
    const h = mounted()
    h.switcher.render({ context: shop, proposals })
    h.open()
    h.rows()[2]!.fire('click')
    expect(h.actions.choose).toHaveBeenCalledWith(':p-2')
    expect(h.done).toHaveBeenCalledTimes(1)
    expect(h.menu.hidden).toBe(true)
    expect(h.menu.hasAttribute('role')).toBe(false)
  })

  it('sends nothing for the proposal already under discussion', () => {
    const h = mounted()
    h.switcher.render({ context: shop, proposals })
    h.open()
    h.rows()[1]!.fire('click')
    expect(h.actions.choose).not.toHaveBeenCalled()
    expect(h.actions.namespaceTree).not.toHaveBeenCalled()
  })

  it('asks for a new proposal from its last row', () => {
    const h = mounted()
    h.switcher.render({ context: real, proposals })
    h.open()
    h.rows().at(-1)!.fire('click')
    expect(h.actions.newProposal).toHaveBeenCalledTimes(1)
  })

  it('returns to the source tree like P: leaving a proposal is a context switch, else the tree root', () => {
    const h = mounted()
    h.switcher.render({ context: shop, proposals })
    h.open()
    h.rows()[0]!.fire('click')
    expect(h.actions.choose).toHaveBeenCalledWith(null)
    const actions: SwitcherActions = { choose: vi.fn(), namespaceTree: vi.fn(), newProposal: vi.fn() }
    returnToSourceTree(real, actions)
    returnToSourceTree(null, actions)
    expect(actions.namespaceTree).toHaveBeenCalledTimes(2)
    expect(actions.choose).not.toHaveBeenCalled()
  })

  it('closes on Escape and gives the keyboard back to its button', () => {
    const h = mounted()
    h.switcher.render({ context: real, proposals: [] })
    h.open()
    expect(h.rows().map(row => row.attributes.role)).toEqual(['menuitemradio', 'menuitem'])
    const escape = h.rows()[0]!.fire('keydown', { key: 'Escape' })
    expect(escape.defaultPrevented).toBe(true)
    expect(h.menu.hidden).toBe(true)
    expect(h.trigger.ownerDocument.activeElement).toBe(h.trigger)
  })
})
