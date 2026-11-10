/**
 * The panel's top bar (#46, ADR 0015): the breadcrumb path, the discussion-
 * context switcher (#47, `context-switcher.ts`), the search button with its box and result list, and the 「更多」 menu. It is the
 * first section of the three-part skeleton; `statusbar.ts` is the last and the
 * canvas sits between them. `index.ts` only wires the three together.
 *
 * Plain markup styled from host theme tokens — the host's UI primitives are
 * never loaded (ADR 0013). Every string comes from the locale files, so a DSH
 * language switch relabels the bar in place (#42).
 *
 * Later tickets edit this file alone for the top bar: #50 adds its legend
 * entry through `moreEntries`.
 */
import { defaultProposalName, type BreadcrumbLevel } from '@dsh-uml-viewer/contracts'
import type { PanelController, PanelStatus } from './panel.js'
import { ENGLISH_TEXT, type PanelText } from './locale.js'
import { mountMoreMenu, moreMenuEntries, type MoreMenuEntry } from './more-menu.js'
import { mountContextSwitcher, SWITCHER_STYLE, type SwitcherActions } from './context-switcher.js'

export const TOPBAR_STYLE = `
.dsh-uml-viewer-topbar {
  display: flex; flex-wrap: wrap; align-items: center; gap: 4px; flex: 0 0 auto; min-height: 36px; padding: 4px 8px;
  border-bottom: 1px solid var(--dsw-alias-border-l2);
  background: var(--dsw-alias-bg-layer-2); color: var(--dsw-alias-label-primary);
  font: 12px/1.4 var(--dsw-font-family, system-ui);
}
.dsh-uml-viewer-topbar button {
  appearance: none; border: 1px solid transparent; border-radius: 4px; padding: 3px 8px;
  background: transparent; color: var(--dsw-alias-label-secondary);
  font: inherit; cursor: pointer; white-space: nowrap;
}
.dsh-uml-viewer-topbar button:hover { background: var(--dsw-alias-interactive-bg-hover); color: var(--dsw-alias-label-primary); }
.dsh-uml-viewer-topbar button:active { background: var(--dsw-alias-interactive-bg-active); }
.dsh-uml-viewer-topbar button:focus-visible,
.dsh-uml-viewer-topbar .dsh-uml-viewer-search input:focus-visible {
  outline: var(--dsw-focus-ring-width, 2px) solid var(--dsw-focus-ring-color, var(--dsw-alias-state-business-primary));
  outline-offset: 1px;
}
.dsh-uml-viewer-topbar button:disabled { opacity: 0.45; cursor: default; background: transparent; }
.dsh-uml-viewer-spacer { flex: 1 1 auto; }
.dsh-uml-viewer-crumbs { flex: 1 1 auto; min-width: 0; }
.dsh-uml-viewer-crumbs ol {
  display: flex; align-items: center; gap: 2px; min-width: 0; margin: 0; padding: 0; list-style: none;
}
.dsh-uml-viewer-crumbs li { display: flex; align-items: center; min-width: 0; }
.dsh-uml-viewer-crumbs li + li::before { content: '›'; padding: 0 1px; color: var(--dsw-alias-label-tertiary); }
.dsh-uml-viewer-crumbs button, .dsh-uml-viewer-crumbs [aria-current="page"] {
  max-width: 160px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
}
.dsh-uml-viewer-crumbs [aria-current="page"] { padding: 3px 4px; color: var(--dsw-alias-label-primary); }
.dsh-uml-viewer-search { position: relative; flex: 0 1 260px; min-width: 120px; }
.dsh-uml-viewer-search[hidden] { display: none; }
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
${SWITCHER_STYLE}`

export interface TopBarOptions {
  controller: PanelController
  text?: PanelText
  /** Put the keyboard back where it was after a top-bar control ran. */
  onDone?: () => void
  /** Entries later tickets add to the More menu (#50's legend). */
  moreEntries?: () => readonly MoreMenuEntry[]
  /**
   * What the discussion-context switcher does (#47). `index.ts` routes these
   * through the overlay controller; without them the panel controller sends
   * the same actions itself.
   */
  context?: SwitcherActions
}

export interface TopBar {
  readonly element: HTMLElement
  /** Follow the panel stream: the path, the discussion and the search box. */
  render(status: PanelStatus): void
  /** **Ctrl+F** and the search button: open the box and put the caret in it. */
  openSearch(): void
  /** The controller already opened the search: reveal the box, focus it. */
  focusSearch(): void
  /** Whether this key target is the search box, which handles its own keys. */
  owns(target: EventTarget | null): boolean
  dispose(): void
}

function labelButton(element: HTMLElement, label: string, title: string): void {
  element.textContent = label
  element.title = title
  element.setAttribute('aria-label', title)
}

/** Attach the top bar to its section of the panel. */
export function mountTopBar(host: HTMLElement, options: TopBarOptions): TopBar {
  const text = options.text ?? ENGLISH_TEXT
  const controller = options.controller
  const doc = host.ownerDocument
  const bar = doc.createElement('div')
  bar.className = 'dsh-uml-viewer-topbar'
  // One button per level above the current one, the current level as text.
  const crumbs = doc.createElement('nav')
  crumbs.className = 'dsh-uml-viewer-crumbs'
  const spacer = doc.createElement('span')
  spacer.className = 'dsh-uml-viewer-spacer'
  const switcher = mountContextSwitcher(doc, {
    text,
    actions: options.context ?? {
      choose: proposalId => { void controller.send({ action: 'context', proposalId }) },
      namespaceTree: () => { void controller.namespaceTree() },
      newProposal: () => { void controller.send({ action: 'new-proposal', name: defaultProposalName(new Date()) }) },
    },
    onDone: () => options.onDone?.(),
  })
  const searchButton = doc.createElement('button')
  searchButton.type = 'button'
  searchButton.className = 'dsh-uml-viewer-search-open'
  searchButton.setAttribute('aria-controls', 'dsh-uml-viewer-search')
  const search = doc.createElement('div')
  search.className = 'dsh-uml-viewer-search'
  search.id = 'dsh-uml-viewer-search'
  search.hidden = true
  const input = doc.createElement('input')
  input.type = 'search'
  input.autocomplete = 'off'
  const results = doc.createElement('ul')
  results.className = 'dsh-uml-viewer-results'
  results.setAttribute('role', 'listbox')
  results.hidden = true
  search.append(input, results)
  const moreButton = doc.createElement('button')
  moreButton.type = 'button'
  moreButton.className = 'dsh-uml-viewer-more'
  bar.append(crumbs, spacer, switcher.element, searchButton, search, moreButton)
  host.appendChild(bar)

  const more = mountMoreMenu(bar, moreButton, {
    text,
    entries: () => [...moreMenuEntries(controller, text), ...(options.moreEntries?.() ?? [])],
    onDone: () => options.onDone?.(),
  })

  let lastCrumbs = ''
  let lastResults = ''
  let lastLanguage: string = text.language
  const labels = (): void => {
    crumbs.setAttribute('aria-label', text.t('breadcrumbs.label'))
    labelButton(searchButton, text.t('search.button'), text.t('search.button.title'))
    labelButton(moreButton, text.t('more.button'), text.t('more.button.title'))
    input.placeholder = text.t('search.placeholder')
    input.setAttribute('aria-label', text.t('search.label'))
    results.setAttribute('aria-label', text.t('search.results'))
  }
  const renderCrumbs = (levels: readonly BreadcrumbLevel[]): void => {
    const signature = JSON.stringify(levels.map(level => [level.id, level.name]))
    if (signature === lastCrumbs) return
    lastCrumbs = signature
    const list = doc.createElement('ol')
    levels.forEach((level, index) => {
      const item = doc.createElement('li')
      if (index + 1 === levels.length) {
        const current = doc.createElement('span')
        current.setAttribute('aria-current', 'page')
        current.textContent = level.name
        item.appendChild(current)
      } else {
        const choice = doc.createElement('button')
        choice.type = 'button'
        choice.dataset.level = String(index)
        labelButton(choice, level.name, text.t('breadcrumb.go', { name: level.name }))
        choice.addEventListener('click', () => {
          void controller.backTo(level.id)
          options.onDone?.()
        })
        item.appendChild(choice)
      }
      list.appendChild(item)
    })
    crumbs.replaceChildren(list)
  }
  const renderSearch = (status: PanelStatus): void => {
    searchButton.setAttribute('aria-expanded', String(status.search.active))
    search.hidden = !status.search.active
    if (doc.activeElement !== input && input.value !== status.search.query) input.value = status.search.query
    const signature = JSON.stringify([status.search.active, status.search.results, status.search.selection])
    if (signature === lastResults) return
    lastResults = signature
    results.replaceChildren()
    results.hidden = !status.search.active
    if (status.search.active && status.search.query.trim() && status.search.results.length === 0) {
      const empty = doc.createElement('li')
      empty.className = 'dsh-uml-viewer-empty'
      empty.textContent = text.t('search.empty')
      results.appendChild(empty)
    }
    status.search.results.forEach((result, index) => {
      const row = doc.createElement('li')
      const choice = doc.createElement('button')
      choice.type = 'button'
      choice.setAttribute('role', 'option')
      choice.setAttribute('aria-selected', String(index === status.search.selection))
      choice.textContent = result.label
      choice.addEventListener('click', () => {
        controller.moveSearch(index - controller.current.search.selection)
        controller.activateSearch()
        options.onDone?.()
      })
      row.appendChild(choice)
      results.appendChild(row)
    })
  }
  const render = (status: PanelStatus): void => {
    renderCrumbs(status.breadcrumbs)
    switcher.render({ context: status.context, proposals: status.proposals })
    renderSearch(status)
  }
  const focusSearch = (): void => {
    search.hidden = false
    searchButton.setAttribute('aria-expanded', 'true')
    input.focus()
  }
  const openSearch = (): void => {
    controller.openSearch()
    focusSearch()
  }
  const onSearchButton = (): void => openSearch()
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
  searchButton.addEventListener('click', onSearchButton)
  input.addEventListener('keydown', onSearchKey)
  input.addEventListener('input', onSearchInput)
  labels()
  // A DSH language switch relabels the bar in place; the level names are the
  // tree's, but their button titles are ours.
  const unsubscribe = text.subscribe(() => {
    if (text.language === lastLanguage) return
    lastLanguage = text.language
    labels()
    lastCrumbs = ''
    lastResults = ''
    const current = controller.current
    renderCrumbs(current.breadcrumbs)
    switcher.render({ context: current.context, proposals: current.proposals })
    renderSearch(current)
  })
  return {
    element: bar,
    render,
    openSearch,
    focusSearch,
    // The search box and the switcher handle their own keys (#47).
    owns: target => target === input || (target !== null && switcher.element.contains?.(target as Node) === true),
    dispose() {
      unsubscribe()
      searchButton.removeEventListener('click', onSearchButton)
      input.removeEventListener('keydown', onSearchKey)
      input.removeEventListener('input', onSearchInput)
      more.dispose()
      switcher.dispose()
      bar.remove()
    },
  }
}
