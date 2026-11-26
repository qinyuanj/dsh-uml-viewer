import { DECLUTTER_MODES } from '@dsh-uml-viewer/contracts'
import type { PanelController } from './panel.js'
import { ENGLISH_TEXT, type PanelText } from './locale.js'

export interface MoreMenuEntry {
  readonly id: string
  label(): string
  readonly shortcut?: string
  readonly role?: 'menuitemradio'
  readonly group?: 'declutter'
  checked?(): boolean
  disabled?(): boolean
  run(): void
}

export const MORE_MENU_STYLE = `
.dsh-uml-viewer-more-menu {
  position: absolute; z-index: 60; min-width: 180px; max-width: calc(100% - 16px); padding: 4px;
  border: 1px solid var(--dsw-alias-border-l2); border-radius: 6px;
  background: var(--dsw-alias-bg-overlay); color: var(--dsw-alias-label-primary);
  box-shadow: var(--dsw-shadow-lv2, 0 4px 16px rgb(0 0 0 / 20%));
  font: 12px/1.4 var(--dsw-font-family, system-ui);
}
.dsh-uml-viewer-more-menu[hidden] { display: none; }
.dsh-uml-viewer-more-menu button {
  display: flex; align-items: center; gap: 12px; width: 100%; border: 0; border-radius: 3px;
  padding: 6px 10px; text-align: left; background: transparent; color: inherit; font: inherit; cursor: pointer;
}
.dsh-uml-viewer-more-menu button:hover, .dsh-uml-viewer-more-menu button:focus-visible {
  background: var(--dsw-alias-interactive-bg-hover);
  outline: 1px solid var(--dsw-focus-ring-color, var(--dsw-alias-state-business-primary));
}
.dsh-uml-viewer-more-menu button:disabled { opacity: .45; cursor: default; background: transparent; }
.dsh-uml-viewer-more-menu .dsh-uml-viewer-more-check { flex: 0 0 auto; width: 1em; text-align: center; }
.dsh-uml-viewer-more-menu .dsh-uml-viewer-more-text { flex: 1 1 auto; }
.dsh-uml-viewer-more-menu .dsh-uml-viewer-more-shortcut { flex: 0 0 auto; color: var(--dsw-alias-label-tertiary); }
`

export function moreMenuEntries(controller: PanelController, text: PanelText): MoreMenuEntry[] {
  return [
    {
      id: 'fit',
      label: () => text.t('more.fit'),
      shortcut: 'Ctrl+0',
      disabled: () => controller.current.frame === null,
      run: () => controller.fit(),
    },
    ...DECLUTTER_MODES.map((mode): MoreMenuEntry => ({
      id: `declutter-${mode}`,
      label: () => text.t(mode === 'triangles' ? 'more.removeArrows' : `more.declutter.${mode}`),
      role: 'menuitemradio',
      group: 'declutter',
      checked: () => controller.declutterMode === mode,
      run: () => {
        void controller.setDeclutter(mode).catch(() => {})
      },
    })),
  ]
}

export interface MoreMenuOptions {
  text?: PanelText
  entries: () => readonly MoreMenuEntry[]
  onDone?: () => void
}

export interface MoreMenu {
  toggle(): void
  open(): void
  close(): void
  isOpen(): boolean
  dispose(): void
}

let counter = 0

// The popover is anchored to its trigger inside the top bar, so it can never
// reach the host's tab strip above the panel (#140).
const MENU_GAP = 4
const MENU_EDGE = 4

export function mountMoreMenu(host: HTMLElement, trigger: HTMLButtonElement, options: MoreMenuOptions): MoreMenu {
  const text = options.text ?? ENGLISH_TEXT
  const doc = host.ownerDocument
  const menuId = `dsh-uml-viewer-more-${++counter}`
  const menu = doc.createElement('div')
  menu.className = 'dsh-uml-viewer-more-menu'
  menu.id = menuId
  menu.hidden = true
  let items: HTMLButtonElement[] = []
  let open = false
  trigger.setAttribute('aria-haspopup', 'menu')
  trigger.setAttribute('aria-expanded', 'false')

  const bounds = (element: Element | null): DOMRect | null =>
    element !== null && typeof (element as HTMLElement).getBoundingClientRect === 'function'
      ? (element as HTMLElement).getBoundingClientRect()
      : null
  // Hang the menu off the trigger: below it when the panel has room, above it
  // otherwise, with the right edge following the trigger and the whole box kept
  // inside the bar. Without this the menu's static position is the bar's
  // centred flex line, which lifts it over the host tab strip (#140).
  const place = (): void => {
    const anchor = bounds(trigger)
    const bar = bounds(host)
    if (anchor === null || bar === null) return
    const width = menu.offsetWidth ?? 0
    const height = menu.offsetHeight ?? 0
    const below = anchor.bottom - bar.top + MENU_GAP
    const panel = bounds(host.parentElement)
    const room = panel === null ? Number.POSITIVE_INFINITY : panel.bottom - bar.top - below
    const top = height > room ? Math.max(0, anchor.top - bar.top - MENU_GAP - height) : below
    const rightAligned = anchor.right - bar.left - width
    const left = Math.max(MENU_EDGE, Math.min(rightAligned, bar.width - width - MENU_EDGE))
    menu.style.top = `${top}px`
    menu.style.left = `${left}px`
  }

  // The panel stub used by the unit tests has no view; a browser always has one.
  const watchResize = (listen: boolean): void => {
    const view = doc.defaultView as unknown as {
      addEventListener?: (type: string, handler: () => void) => void
      removeEventListener?: (type: string, handler: () => void) => void
    } | null
    if (listen) view?.addEventListener?.('resize', place)
    else view?.removeEventListener?.('resize', place)
  }

  const close = (): void => {
    watchResize(false)
    if (!open) return
    open = false
    menu.hidden = true
    menu.removeAttribute('role')
    trigger.setAttribute('aria-expanded', 'false')
    trigger.removeAttribute('aria-controls')
  }

  const run = (entry: MoreMenuEntry): void => {
    close()
    entry.run()
    options.onDone?.()
  }

  const build = (): void => {
    menu.setAttribute('aria-label', text.t('more.label'))
    menu.replaceChildren()
    const entries = options.entries()
    let currentGroup: MoreMenuEntry['group']
    let container = menu
    items = entries.map(entry => {
      if (entry.group !== currentGroup) {
        currentGroup = entry.group
        container = menu
        if (entry.group) {
          container = doc.createElement('div')
          container.setAttribute('role', 'group')
          container.setAttribute('aria-label', text.t('more.declutter.label'))
          menu.appendChild(container)
        }
      }
      const button = doc.createElement('button')
      button.type = 'button'
      button.dataset.entry = entry.id
      const checked = entry.checked?.() ?? false
      button.setAttribute('role', entry.role ?? (entry.checked ? 'menuitemcheckbox' : 'menuitem'))
      if (entry.checked) button.setAttribute('aria-checked', String(checked))
      button.disabled = entry.disabled?.() ?? false
      const check = doc.createElement('span')
      check.className = 'dsh-uml-viewer-more-check'
      check.setAttribute('aria-hidden', 'true')
      check.textContent = checked ? '✓' : ''
      const label = doc.createElement('span')
      label.className = 'dsh-uml-viewer-more-text'
      label.textContent = entry.label()
      const shortcut = doc.createElement('span')
      shortcut.className = 'dsh-uml-viewer-more-shortcut'
      shortcut.setAttribute('aria-hidden', 'true')
      shortcut.textContent = entry.shortcut ?? ''
      button.append(check, label, shortcut)
      if (entry.shortcut) button.setAttribute('aria-keyshortcuts', entry.shortcut)
      button.addEventListener('click', () => run(entry))
      container.appendChild(button)
      return button
    })
  }

  const show = (): void => {
    if (open) {
      close()
      return
    }
    build()
    open = true
    menu.hidden = false
    place()
    watchResize(true)
    menu.setAttribute('role', 'menu')
    trigger.setAttribute('aria-expanded', 'true')
    trigger.setAttribute('aria-controls', menuId)
    items[0]?.focus()
  }

  const onTrigger = (): void => show()
  const dismiss = (event: Event): void => {
    if (!open || event.target === trigger) return
    if (event.target !== menu && !menu.contains?.(event.target as Node)) close()
  }
  const key = (event: KeyboardEvent): void => {
    if (!open || event.isComposing) return
    if (event.key === 'Escape') {
      event.preventDefault()
      event.stopPropagation()
      close()
      trigger.focus()
      return
    }
    if (event.key === 'Tab') {
      close()
      return
    }
    if (event.key !== 'ArrowDown' && event.key !== 'ArrowUp') return
    event.preventDefault()
    const enabled = items.filter(item => !item.disabled)
    if (enabled.length === 0) return
    const index = enabled.indexOf(doc.activeElement as HTMLButtonElement)
    const step = event.key === 'ArrowDown' ? 1 : -1
    const next = index < 0 ? 0 : (index + step + enabled.length) % enabled.length
    enabled[next]?.focus()
  }
  trigger.addEventListener('click', onTrigger)
  host.addEventListener('pointerdown', dismiss)
  host.addEventListener('keydown', key, true)
  host.appendChild(menu)
  return {
    toggle: show,
    open: () => {
      if (!open) show()
    },
    close,
    isOpen: () => open,
    dispose() {
      close()
      trigger.removeEventListener('click', onTrigger)
      host.removeEventListener('pointerdown', dismiss)
      host.removeEventListener('keydown', key, true)
      menu.remove()
    },
  }
}
