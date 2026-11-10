/**
 * The discussion-context switcher (#47, ADR 0015): the top bar's dropdown
 * that names the discussion context and changes it. It lists the source
 * diagram, then each proposal of the shown diagram (from the resident's
 * `state` event), and ends with 「新建提案」. It replaced the inspector's
 * proposal list, the canvas's proposal banner and the `Namespace tree` button.
 *
 * Choosing reuses the existing panel actions: `context` (the host switches the
 * view and tells the companion with a message in the conversation) and
 * `new-proposal` (the default name is still the local time). **P** is the
 * same as choosing the source diagram here: back to the namespace tree's root.
 *
 * Plain markup styled from host theme tokens (ADR 0013). Same rule as the
 * More menu: `role="menu"` only while open, because the host's shortcut
 * service treats any `[role="menu"]` in the document as an open modal.
 */
import type { DiscussionContext, ProposalEntry } from '@dsh-uml-viewer/contracts'
import { ENGLISH_TEXT, type PanelText } from './locale.js'

export const SWITCHER_STYLE = `
.dsh-uml-viewer-switcher { position: relative; flex: 0 1 auto; min-width: 0; max-width: 40%; display: flex; }
.dsh-uml-viewer-topbar .dsh-uml-viewer-switcher-button {
  display: flex; align-items: center; gap: 4px; min-width: 0; max-width: 100%;
  color: var(--dsw-alias-label-primary);
}
.dsh-uml-viewer-switcher-name { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.dsh-uml-viewer-switcher-caret { flex: 0 0 auto; color: var(--dsw-alias-label-tertiary); }
.dsh-uml-viewer-switcher-menu {
  position: absolute; z-index: 60; top: calc(100% + 4px); left: 0; min-width: 200px; max-width: min(360px, 90vw);
  max-height: 320px; overflow: auto; padding: 4px;
  border: 1px solid var(--dsw-alias-border-l2); border-radius: 6px;
  background: var(--dsw-alias-bg-overlay); color: var(--dsw-alias-label-primary);
  box-shadow: var(--dsw-shadow-lv2, 0 4px 16px rgb(0 0 0 / 20%));
  font: 12px/1.4 var(--dsw-font-family, system-ui);
}
.dsh-uml-viewer-switcher-menu[hidden] { display: none; }
.dsh-uml-viewer-topbar .dsh-uml-viewer-switcher-menu button {
  display: flex; align-items: center; gap: 8px; width: 100%; border: 0; border-radius: 3px;
  padding: 6px 10px; text-align: left; background: transparent; color: inherit; font: inherit; cursor: pointer;
  white-space: normal; overflow-wrap: anywhere;
}
.dsh-uml-viewer-topbar .dsh-uml-viewer-switcher-menu button:hover,
.dsh-uml-viewer-topbar .dsh-uml-viewer-switcher-menu button:focus-visible {
  background: var(--dsw-alias-interactive-bg-hover);
  outline: 1px solid var(--dsw-focus-ring-color, var(--dsw-alias-state-business-primary));
}
.dsh-uml-viewer-switcher-check { flex: 0 0 auto; width: 1em; text-align: center; }
.dsh-uml-viewer-switcher-heading { padding: 6px 10px 2px; color: var(--dsw-alias-label-tertiary); }
.dsh-uml-viewer-switcher-separator { height: 1px; margin: 4px 0; background: var(--dsw-alias-border-l2); }
`

/** What the switcher asks for; `index.ts` wires these to the overlay controller. */
export interface SwitcherActions {
  /** The `context` action: the source diagram (null) or a proposal id. */
  choose(proposalId: string | null): void
  /** The `namespace-tree` action: the source diagram's root, no context change. */
  namespaceTree(): void
  /** The `new-proposal` action, with the local time as the default name. */
  newProposal(): void
}

/** What the switcher shows: the panel's discussion context and proposal list. */
export interface SwitcherView {
  context: DiscussionContext | null
  proposals: readonly ProposalEntry[]
}

/** The proposal under discussion, or null for the source diagram. */
function discussed(context: DiscussionContext | null): string | null {
  return context?.kind === 'proposal' ? context.proposalId ?? null : null
}

/**
 * **P** and the switcher's source-diagram row: back to the namespace tree's
 * root of the source diagram. Leaving a proposal is a context switch the
 * companion is told about; already on the source diagram only the view returns
 * to the root, so the conversation gets no repeat message.
 */
export function returnToSourceTree(context: DiscussionContext | null, actions: SwitcherActions): void {
  if (context?.kind === 'proposal') actions.choose(null)
  else actions.namespaceTree()
}

/** The name the switcher's button shows for the current discussion context. */
export function contextName(view: SwitcherView, text: PanelText): string {
  const id = discussed(view.context)
  if (view.context?.kind !== 'proposal') return text.t('switcher.source')
  return view.context.name ?? view.proposals.find(entry => entry.id === id)?.name ?? id ?? text.t('summary.proposal')
}

export interface ContextSwitcherOptions {
  text?: PanelText
  actions: SwitcherActions
  /** Put the keyboard back where it was after a choice. */
  onDone?: () => void
}

export interface ContextSwitcher {
  readonly element: HTMLElement
  render(view: SwitcherView): void
  isOpen(): boolean
  close(): void
  dispose(): void
}

let counter = 0

/** Build the switcher; the caller places `element` in the top bar. */
export function mountContextSwitcher(doc: Document, options: ContextSwitcherOptions): ContextSwitcher {
  const text = options.text ?? ENGLISH_TEXT
  const root = doc.createElement('div')
  root.className = 'dsh-uml-viewer-switcher'
  const trigger = doc.createElement('button')
  trigger.type = 'button'
  trigger.className = 'dsh-uml-viewer-switcher-button'
  trigger.setAttribute('aria-haspopup', 'menu')
  trigger.setAttribute('aria-expanded', 'false')
  const name = doc.createElement('span')
  name.className = 'dsh-uml-viewer-switcher-name'
  const caret = doc.createElement('span')
  caret.className = 'dsh-uml-viewer-switcher-caret'
  caret.setAttribute('aria-hidden', 'true')
  caret.textContent = '▾'
  trigger.append(name, caret)
  const menuId = `dsh-uml-viewer-switcher-${++counter}`
  const menu = doc.createElement('div')
  menu.className = 'dsh-uml-viewer-switcher-menu'
  menu.id = menuId
  menu.hidden = true
  root.append(trigger, menu)

  let view: SwitcherView = { context: null, proposals: [] }
  let items: HTMLButtonElement[] = []
  let open = false

  const label = (): void => {
    const current = contextName(view, text)
    name.textContent = current
    trigger.title = text.t('switcher.title', { name: current })
    trigger.setAttribute('aria-label', text.t('context.label', { name: current }))
    trigger.dataset.kind = view.context?.kind ?? 'real'
    menu.setAttribute('aria-label', text.t('switcher.label'))
  }

  // Declared below; a press anywhere outside closes the open menu.
  let dismiss: (event: Event) => void = () => {}
  const close = (): void => {
    if (!open) return
    open = false
    doc.removeEventListener?.('pointerdown', dismiss, true)
    menu.hidden = true
    menu.removeAttribute('role')
    trigger.setAttribute('aria-expanded', 'false')
    trigger.removeAttribute('aria-controls')
  }

  const run = (act: () => void): void => {
    close()
    act()
    options.onDone?.()
  }

  const item = (caption: string, role: string, act: () => void, checked?: boolean): HTMLButtonElement => {
    const button = doc.createElement('button')
    button.type = 'button'
    button.setAttribute('role', role)
    const check = doc.createElement('span')
    check.className = 'dsh-uml-viewer-switcher-check'
    check.setAttribute('aria-hidden', 'true')
    if (checked !== undefined) {
      button.setAttribute('aria-checked', String(checked))
      check.textContent = checked ? '✓' : ''
    }
    const words = doc.createElement('span')
    words.textContent = caption
    button.append(check, words)
    button.addEventListener('click', () => run(act))
    return button
  }

  const build = (): void => {
    menu.replaceChildren()
    const active = discussed(view.context)
    const source = item(text.t('switcher.source'), 'menuitemradio',
      () => returnToSourceTree(view.context, options.actions), active === null)
    source.dataset.entry = 'source'
    const rows: HTMLElement[] = [source]
    items = [source]
    if (view.proposals.length > 0) {
      const heading = doc.createElement('div')
      heading.className = 'dsh-uml-viewer-switcher-heading'
      heading.setAttribute('role', 'presentation')
      heading.textContent = text.t('switcher.proposals')
      rows.push(heading)
      for (const proposal of view.proposals) {
        // The proposal already under discussion stays as it is: choosing it
        // again would tell the companion nothing new.
        const choice = item(proposal.name, 'menuitemradio', () => {
          if (proposal.id !== discussed(view.context)) options.actions.choose(proposal.id)
        }, proposal.id === active)
        choice.dataset.entry = 'proposal'
        choice.dataset.proposalId = proposal.id
        rows.push(choice)
        items.push(choice)
      }
    }
    const separator = doc.createElement('div')
    separator.className = 'dsh-uml-viewer-switcher-separator'
    separator.setAttribute('role', 'separator')
    const create = item(text.t('switcher.newProposal'), 'menuitem', () => options.actions.newProposal())
    create.dataset.entry = 'new-proposal'
    items.push(create)
    rows.push(separator, create)
    menu.replaceChildren(...rows)
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
    doc.addEventListener?.('pointerdown', dismiss, true)
    const checked = items.find(entry => entry.getAttribute('aria-checked') === 'true')
    ;(checked ?? items[0])?.focus()
  }

  const onTrigger = (): void => show()
  // Any press outside the switcher closes it; the trigger's own click toggles.
  dismiss = (event: Event): void => {
    if (!open) return
    const target = event.target as Node | null
    if (target && (target === root || root.contains?.(target))) return
    close()
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
    if (event.key !== 'ArrowDown' && event.key !== 'ArrowUp' && event.key !== 'Home' && event.key !== 'End') return
    event.preventDefault()
    event.stopPropagation()
    const index = items.indexOf(doc.activeElement as HTMLButtonElement)
    const last = items.length - 1
    const next = event.key === 'Home' ? 0
      : event.key === 'End' ? last
        : index < 0 ? 0
          : (index + (event.key === 'ArrowDown' ? 1 : -1) + items.length) % items.length
    items[next]?.focus()
  }
  // The trigger opens the menu from the keyboard too (ArrowDown, like a select).
  const triggerKey = (event: KeyboardEvent): void => {
    if (open || event.isComposing || event.key !== 'ArrowDown') return
    event.preventDefault()
    event.stopPropagation()
    show()
  }
  trigger.addEventListener('click', onTrigger)
  trigger.addEventListener('keydown', triggerKey)
  root.addEventListener('keydown', key, true)
  label()
  return {
    element: root,
    render(next) {
      // Every frame publishes a status; only a changed list or context
      // rebuilds an open menu, so the keyboard focus in it survives.
      const changed = discussed(next.context) !== discussed(view.context)
        || JSON.stringify(next.proposals) !== JSON.stringify(view.proposals)
      view = next
      label()
      if (open && changed) {
        const focused = (doc.activeElement as HTMLElement | null)?.dataset?.proposalId
          ?? (doc.activeElement as HTMLElement | null)?.dataset?.entry
        build()
        const again = items.find(entry => entry.dataset.proposalId === focused || entry.dataset.entry === focused)
        ;(again ?? items[0])?.focus()
      }
    },
    isOpen: () => open,
    close,
    dispose() {
      close()
      trigger.removeEventListener('click', onTrigger)
      trigger.removeEventListener('keydown', triggerKey)
      root.removeEventListener('keydown', key, true)
      root.remove()
    },
  }
}
