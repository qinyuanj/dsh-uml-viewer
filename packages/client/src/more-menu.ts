/**
 * The top bar's 「更多」 menu (#46, ADR 0015): fit the view and the Remove
 * arrows toggle, each with its shortcut beside it. The legend entry joins them
 * in #50; the request list of #51 lives in the status bar, not here.
 *
 * The menu is plain markup styled from host theme tokens (ADR 0013: the host's
 * UI primitives are never loaded). Same rule as the element menu (#43):
 * `role="menu"` only while open. The host's shortcut service treats any
 * `[role="menu"]` in the document as an open modal, even a hidden one, and a
 * modal blocks the tab's refresh command.
 */
import type { PanelController } from './panel.js'
import { ENGLISH_TEXT, type PanelText } from './locale.js'

/** One item of the More menu. */
export interface MoreMenuEntry {
  /** Stable key; later tickets add their entries beside these. */
  readonly id: string
  /** The item's text, read in the current language. */
  label(): string
  /** The shortcut shown beside the label (`Ctrl+0`), where the item has one. */
  readonly shortcut?: string
  /** A toggle's state, drawn as `aria-checked` on a checkbox item. */
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

/** The built-in entries: fit the view, and the Remove arrows toggle. */
export function moreMenuEntries(controller: PanelController, text: PanelText): MoreMenuEntry[] {
  return [
    {
      id: 'fit',
      label: () => text.t('more.fit'),
      shortcut: 'Ctrl+0',
      disabled: () => controller.current.frame === null,
      run: () => controller.fit(),
    },
    {
      id: 'remove-arrows',
      label: () => text.t('more.removeArrows'),
      // `triangles` is the arrow-less mode; any other mode (a restored view
      // included) first clears to it, so a saved mode cannot strand the user.
      checked: () => controller.declutterMode === 'triangles',
      run: () => {
        // The panel stream reports its own failures; the menu just asks.
        void controller
          .setDeclutter(controller.declutterMode === 'triangles' ? 'full' : 'triangles')
          .catch(() => {})
      },
    },
  ]
}

export interface MoreMenuOptions {
  text?: PanelText
  /** Read when the menu opens, so later tickets add entries without a remount. */
  entries: () => readonly MoreMenuEntry[]
  /** Put the keyboard back where it was after an entry ran. */
  onDone?: () => void
}

export interface MoreMenu {
  /** Open or close the menu; the top bar's button and Esc both do this. */
  toggle(): void
  open(): void
  close(): void
  isOpen(): boolean
  dispose(): void
}

let counter = 0

/**
 * Attach the More menu to its trigger button. The menu element is appended to
 * `host` (the top bar) so it is anchored under the button and stays inside the
 * panel's own box.
 */
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

  const close = (): void => {
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
    // A reopen rebuilds the items: labels and toggle states may have changed.
    menu.replaceChildren()
    const entries = options.entries()
    items = entries.map(entry => {
      const button = doc.createElement('button')
      button.type = 'button'
      button.dataset.entry = entry.id
      const checked = entry.checked?.() ?? false
      button.setAttribute('role', entry.checked ? 'menuitemcheckbox' : 'menuitem')
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
      menu.appendChild(button)
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
    menu.setAttribute('role', 'menu')
    trigger.setAttribute('aria-expanded', 'true')
    trigger.setAttribute('aria-controls', menuId)
    items[0]?.focus()
  }

  const onTrigger = (): void => show()
  const dismiss = (event: Event): void => {
    // A press on the trigger is the toggle, not a dismissal: closing here
    // would let the click that follows reopen the menu for good.
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
