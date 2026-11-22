import type { PanelAck, PanelAction, SceneFrame } from '@dsh-uml-viewer/contracts'
import { ENGLISH_TEXT, type PanelText } from './locale.js'
import { ELEMENT_REQUESTS, sendElementRequest, type ElementRequest } from './requests.js'

export const ACTION_BAR_STYLE = `
.dsh-uml-viewer-action-bar {
  position: absolute; z-index: 30; left: 8px; bottom: 8px; max-width: calc(100% - 16px); box-sizing: border-box;
  display: flex; align-items: center; gap: 4px; padding: 3px 4px 3px 10px;
  border: 1px solid var(--dsw-alias-border-l2); border-radius: 6px;
  background: var(--dsw-alias-bg-overlay); color: var(--dsw-alias-label-primary);
  box-shadow: var(--dsw-shadow-lv2, 0 4px 16px rgb(0 0 0 / 20%));
  font: 12px/1.4 var(--dsw-font-family, system-ui);
}
.dsh-uml-viewer-action-bar[hidden] { display: none; }
.dsh-uml-viewer-action-bar[data-kind="foreign"] { padding-right: 10px; }
.dsh-uml-viewer-action-name {
  flex: 0 1 auto; min-width: 2em; max-width: 220px; margin-right: 4px; font-weight: 600;
  overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
}
.dsh-uml-viewer-action-buttons { display: flex; flex: 0 1 auto; flex-wrap: wrap; align-items: center; gap: 2px; min-width: 0; }
.dsh-uml-viewer-action-bar button {
  appearance: none; border: 1px solid transparent; border-radius: 4px; padding: 3px 8px;
  background: transparent; color: var(--dsw-alias-label-secondary);
  font: inherit; cursor: pointer; white-space: nowrap;
}
.dsh-uml-viewer-action-bar button:hover { background: var(--dsw-alias-interactive-bg-hover); color: var(--dsw-alias-label-primary); }
.dsh-uml-viewer-action-bar button:active { background: var(--dsw-alias-interactive-bg-active); }
.dsh-uml-viewer-action-bar button:focus-visible {
  outline: var(--dsw-focus-ring-width, 2px) solid var(--dsw-focus-ring-color, var(--dsw-alias-state-business-primary));
  outline-offset: 1px;
}
.dsh-uml-viewer-action-menu {
  position: absolute; z-index: 40; right: 0; bottom: calc(100% + 4px); min-width: 180px; padding: 4px;
  border: 1px solid var(--dsw-alias-border-l2); border-radius: 6px;
  background: var(--dsw-alias-bg-overlay); box-shadow: var(--dsw-shadow-lv2, 0 4px 16px rgb(0 0 0 / 20%));
}
.dsh-uml-viewer-action-menu[hidden] { display: none; }
.dsh-uml-viewer-action-menu button { display: block; width: 100%; text-align: left; color: inherit; }
.dsh-uml-viewer-panel:has(.dsh-uml-drawer) .dsh-uml-viewer-action-bar { bottom: calc(50% + 8px); }
.dsh-uml-viewer-panel[data-layout="docked"]:has(.dsh-uml-drawer) .dsh-uml-viewer-action-bar { bottom: 8px; }
`

export interface ElementSelection {
  kind: 'class' | 'component' | 'foreign'
  id: string
  name: string
  member?: boolean
}

function keyword(edn: string, key: string): string | null {
  return new RegExp(`:${key}\\s+(:[^\\s,{}\\[\\]()]+)`).exec(edn)?.[1] ?? null
}

function localName(id: string): string {
  return id.replace(/^:/, '').split(/[./]/).pop() || id
}

export function elementSelection(selected: string | null, frame: SceneFrame | null): ElementSelection | null {
  if (!selected || !frame) return null
  const kind = keyword(selected, 'kind')
  const id = keyword(selected, 'id')
  if (!id) return null
  if (kind === ':package') {
    if (id === ':view') return null
    const item = frame.world.find(entry => entry.kind === 'package' && entry.id === id)
    if (!item || item.kind !== 'package') return null
    return { kind: 'component', id, name: item.text ?? localName(id) }
  }
  if (kind !== ':class' && kind !== ':child') return null
  const member = kind === ':child'
  const shown = frame.world.some(entry => entry.kind === 'class' && entry.id === (member ? keyword(selected, 'parent') : id))
  if (!shown) return null
  const box = frame.world.find(entry => entry.kind === 'class' && entry.id === id)
  const label = frame.world.find(entry => entry.kind === 'text' && entry.owner === id && entry.line === 'name')
  const name = (label?.kind === 'text' ? label.text : null) ?? localName(id)
  if (box?.kind === 'class' && box.shape === 'oval') return { kind: 'foreign', id, name }
  return { kind: !member && /:drill\?\s+true\b/.test(selected) ? 'component' : 'class', id, name, ...(member ? { member: true } : {}) }
}

export interface ActionBarOptions {
  text?: PanelText
  send(action: PanelAction): Promise<PanelAck | null>
  openCard(id: string): void | Promise<void>
  report(reason: string | (() => string) | null): void
}

export interface ActionBar {
  readonly element: HTMLElement
  render(selection: ElementSelection | null): void
  dispose(): void
}

export function mountActionBar(host: HTMLElement, options: ActionBarOptions): ActionBar {
  const text = options.text ?? ENGLISH_TEXT
  const doc = host.ownerDocument
  const bar = doc.createElement('div')
  bar.className = 'dsh-uml-viewer-action-bar'
  bar.setAttribute('role', 'toolbar')
  bar.hidden = true
  const name = doc.createElement('span')
  name.className = 'dsh-uml-viewer-action-name'
  const buttons = doc.createElement('span')
  buttons.className = 'dsh-uml-viewer-action-buttons'
  const menu = doc.createElement('div')
  menu.className = 'dsh-uml-viewer-action-menu'
  menu.hidden = true
  bar.append(name, buttons, menu)
  let selection: ElementSelection | null = null
  let trigger: HTMLButtonElement | null = null
  let disposed = false

  const closeMenu = (): void => {
    menu.hidden = true
    menu.removeAttribute('role')
    trigger?.setAttribute('aria-expanded', 'false')
  }
  const button = (label: string, run: () => void): HTMLButtonElement => {
    const element = doc.createElement('button')
    element.type = 'button'
    element.textContent = label
    element.addEventListener('click', () => {
      closeMenu()
      run()
    })
    return element
  }
  const request = (entry: ElementRequest, id: string): void =>
    sendElementRequest(entry.action(id), { send: options.send, report: options.report, text, live: () => !disposed })

  const render = (): void => {
    closeMenu()
    trigger = null
    bar.hidden = selection === null
    bar.setAttribute('aria-label', text.t('actionBar.label'))
    menu.setAttribute('aria-label', text.t('actionBar.menu'))
    buttons.replaceChildren()
    menu.replaceChildren()
    if (!selection) {
      delete bar.dataset.kind
      delete bar.dataset.id
      name.textContent = ''
      return
    }
    const { id, kind } = selection
    bar.dataset.kind = kind
    bar.dataset.id = id
    name.textContent = selection.name
    name.setAttribute('title', id)
    if (kind === 'foreign') return
    if (!selection.member) buttons.appendChild(kind === 'class'
      ? button(text.t('actionBar.openCard'), () => { void options.openCard(id) })
      : button(text.t('actionBar.drillIn'), () => { void options.send({ action: 'drill-in', id }).catch(() => {}) }))
    for (const entry of ELEMENT_REQUESTS) {
      const item = button(text.t(entry.label), () => request(entry, id))
      if (!entry.secondary) {
        buttons.appendChild(item)
        continue
      }
      item.setAttribute('role', 'menuitem')
      menu.appendChild(item)
    }
    const more = doc.createElement('button')
    more.type = 'button'
    more.className = 'dsh-uml-viewer-action-more'
    more.setAttribute('aria-label', text.t('actionBar.more'))
    more.setAttribute('title', text.t('actionBar.more.title', { name: selection.name }))
    more.setAttribute('aria-haspopup', 'menu')
    more.setAttribute('aria-expanded', 'false')
    const glyph = doc.createElement('span')
    glyph.setAttribute('aria-hidden', 'true')
    glyph.textContent = '⋯'
    more.appendChild(glyph)
    more.addEventListener('click', () => {
      if (!menu.hidden) {
        closeMenu()
        return
      }
      menu.hidden = false
      menu.setAttribute('role', 'menu')
      more.setAttribute('aria-expanded', 'true')
      ;(menu.children[0] as HTMLElement | undefined)?.focus?.()
    })
    trigger = more
    buttons.appendChild(more)
  }

  const onKey = (event: KeyboardEvent): void => {
    if (menu.hidden || event.isComposing || event.key !== 'Escape') return
    event.preventDefault()
    event.stopPropagation()
    closeMenu()
    trigger?.focus()
  }
  const inside = (node: Element | null, of: Element | null): boolean => !!node && !!of && (node === of || of.contains?.(node) === true)
  const dismiss = (event: Event): void => {
    const target = event.target as Element | null
    if (menu.hidden || inside(target, menu) || inside(target, trigger)) return
    closeMenu()
  }
  bar.addEventListener('keydown', onKey)
  host.addEventListener('pointerdown', dismiss)
  render()
  const unsubscribe = text.subscribe(render)
  host.appendChild(bar)
  return {
    element: bar,
    render(next) {
      if (next?.id === selection?.id && next?.kind === selection?.kind
        && next?.name === selection?.name && next?.member === selection?.member) return
      selection = next
      render()
    },
    dispose() {
      disposed = true
      unsubscribe()
      bar.removeEventListener('keydown', onKey)
      host.removeEventListener('pointerdown', dismiss)
      bar.remove()
    },
  }
}
