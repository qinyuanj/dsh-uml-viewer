/**
 * Duplicate clusters (#91): the additive relationship view of dry snapshots.
 * The resident owns every number and the bounded cluster list; this module
 * only lays that data out as DOM beside the legend, with an entry in 更多.
 * A missing snapshot is an explicit unmeasured state, never an empty list of
 * “no duplicates”.
 */
import type { DrySummary, SceneFrame } from '@dsh-uml-viewer/contracts'
import { ENGLISH_TEXT, type PanelText } from './locale.js'
import type { MoreMenuEntry } from './more-menu.js'

export const DRY_CLUSTER_STYLE = `
.dsh-uml-dry { box-sizing: border-box; flex-direction: column; min-height: 0; overflow: auto; overscroll-behavior: contain; color: var(--dsw-alias-label-primary); font: 12px/1.5 var(--dsw-font-family, system-ui); }
.dsh-uml-dry[hidden] { display: none; }
.dsh-uml-dry[data-open=true] { display: flex; pointer-events: auto; position: absolute; z-index: 45; top: 8px; left: 8px; right: 8px; max-height: calc(100% - 16px); padding: 8px 12px 12px; border: 1px solid var(--dsw-alias-border-l2); border-radius: 6px; background: var(--dsw-alias-bg-overlay); box-shadow: var(--dsw-shadow-lv2, 0 4px 16px rgb(0 0 0 / 20%)); }
.dsh-uml-dry-header { flex: 0 0 auto; display: flex; align-items: center; gap: 8px; margin-bottom: 4px; }
.dsh-uml-dry-title { flex: 1 1 auto; margin: 0; font: inherit; font-weight: 600; }
.dsh-uml-dry-close { flex: 0 0 auto; appearance: none; border: 1px solid transparent; border-radius: 4px; padding: 0 6px; background: transparent; color: var(--dsw-alias-label-secondary); font: inherit; cursor: pointer; }
.dsh-uml-dry-close:hover { background: var(--dsw-alias-interactive-bg-hover); color: var(--dsw-alias-label-primary); }
.dsh-uml-dry-close:focus-visible { outline: var(--dsw-focus-ring-width, 2px) solid var(--dsw-focus-ring-color, var(--dsw-alias-state-business-primary)); outline-offset: 1px; }
.dsh-uml-dry-body { flex: 1 1 auto; min-height: 0; }
.dsh-uml-dry-state { margin: 2px 0; color: var(--dsw-alias-label-secondary); }
.dsh-uml-dry-list { margin: 2px 0; padding: 0; list-style: none; }
.dsh-uml-dry-cluster { margin: 0 0 6px; padding: 4px 6px; border-left: 2px solid var(--dsw-alias-state-business-primary); }
.dsh-uml-dry-cluster-label { margin: 0; font-weight: 600; }
.dsh-uml-dry-cluster button { display: block; max-width: 100%; border: 0; padding: 1px 0; background: transparent; color: var(--dsw-alias-label-secondary); font: inherit; text-align: left; overflow-wrap: anywhere; cursor: pointer; }
.dsh-uml-dry-cluster button:hover { color: var(--dsw-alias-label-primary); text-decoration: underline; }
`

export interface DryClusterOptions {
  text?: PanelText
  /** Navigate to an owning class's card, as a card relationship row does. */
  locate?(id: string): void
  /** Called when the list opens, so another popover can step aside. */
  onOpen?(): void
  onDone?(): void
}

export interface DryClusters {
  element: HTMLElement
  render(frame: SceneFrame | null): void
  show(): void
  close(): void
  isOpen(): boolean
  dispose(): void
}

const format = (n: number): string => n.toFixed(2)

function stateLine(dry: DrySummary | null, text: PanelText): string {
  if (dry === null) return text.t('dry.unmeasured')
  if (dry.clusterList.length === 0) return text.t('dry.none')
  return text.t('dry.clusters', { count: String(dry.clusterList.length) })
}

export function mountDryClusters(host: HTMLElement, options: DryClusterOptions = {}): DryClusters {
  const text = options.text ?? ENGLISH_TEXT
  const doc = host.ownerDocument
  const element = doc.createElement('section')
  element.className = 'dsh-uml-dry'
  element.hidden = true
  element.setAttribute('role', 'dialog')
  const title = doc.createElement('h2')
  title.className = 'dsh-uml-dry-title'
  const closeButton = doc.createElement('button')
  closeButton.type = 'button'
  closeButton.className = 'dsh-uml-dry-close'
  const header = doc.createElement('div')
  header.className = 'dsh-uml-dry-header'
  header.append(title, closeButton)
  const body = doc.createElement('div')
  body.className = 'dsh-uml-dry-body'
  element.append(header, body)
  host.appendChild(element)

  let frame: SceneFrame | null = null
  let open = false
  let signature = ''
  let stopText = (): void => {}

  const label = (): void => {
    title.textContent = text.t('dry.title')
    closeButton.textContent = '×'
    closeButton.setAttribute('aria-label', text.t('dry.close'))
    closeButton.title = text.t('dry.close')
  }

  const paint = (): void => {
    body.replaceChildren()
    const dry = frame?.dry ?? null
    const state = doc.createElement('p')
    state.className = 'dsh-uml-dry-state'
    state.textContent = stateLine(dry, text)
    body.appendChild(state)
    if (dry) {
      // Candidates that join no class are counted, never dropped.
      const outside = doc.createElement('p')
      outside.className = 'dsh-uml-dry-state'
      outside.textContent = text.t('dry.unattributed', { count: String(dry.unattributed) })
      body.appendChild(outside)
    }
    if (!dry || dry.clusterList.length === 0) return
    const list = doc.createElement('ul')
    list.className = 'dsh-uml-dry-list'
    for (const cluster of dry.clusterList) {
      const item = doc.createElement('li')
      item.className = 'dsh-uml-dry-cluster'
      const heading = doc.createElement('p')
      heading.className = 'dsh-uml-dry-cluster-label'
      heading.textContent = text.t('dry.weakest', { value: format(cluster.weakest) })
      item.appendChild(heading)
      for (const member of cluster.members) {
        const parts = [member.file]
        if (member.name) parts.push(member.name)
        parts.push(`${member.startLine}-${member.endLine}`)
        const button = doc.createElement('button')
        button.type = 'button'
        button.textContent = text.t('dry.members', { value: parts.join(' ') })
        const owner = member.class
        if (owner !== null) {
          button.title = text.t('dry.class', { value: owner })
          button.setAttribute('aria-label', button.title)
          button.addEventListener('click', () => {
            options.locate?.(owner)
            options.onDone?.()
          })
        } else {
          // A member that joins no class is named, never linked: there is
          // nothing to locate.
          button.disabled = true
          button.title = text.t('dry.unattributed.member')
          button.setAttribute('aria-label', button.title)
        }
        item.appendChild(button)
      }
      list.appendChild(item)
    }
    body.appendChild(list)
    if (dry.truncated) {
      const more = doc.createElement('p')
      more.className = 'dsh-uml-dry-state'
      more.textContent = text.t('dry.truncated', { shown: String(dry.shownClusters), total: String(dry.clusters) })
      body.appendChild(more)
    }
  }

  const apply = (): void => {
    element.dataset.open = String(open)
    element.hidden = !open
  }
  const close = (): void => {
    if (!open) return
    open = false
    apply()
    options.onDone?.()
  }
  const onKey = (event: KeyboardEvent): void => {
    if (event.key !== 'Escape' || !open) return
    event.preventDefault()
    event.stopPropagation()
    close()
  }
  closeButton.addEventListener('click', close)
  element.addEventListener('keydown', onKey)
  label()
  paint()
  apply()
  stopText = text.subscribe(() => { label(); paint() })

  return {
    element,
    render(next) {
      const nextSignature = JSON.stringify(next?.dry ?? null)
      frame = next
      if (nextSignature !== signature) paint()
      signature = nextSignature
      apply()
    },
    show() {
      if (open) return
      open = true
      apply()
      options.onOpen?.()
    },
    close,
    isOpen: () => open,
    dispose() {
      stopText()
      closeButton.removeEventListener('click', close)
      element.removeEventListener('keydown', onKey)
      element.remove()
    },
  }
}

/** The 更多 entry that opens the cluster list, beside the legend. */
export function dryClusterEntries(list: DryClusters, text: PanelText): MoreMenuEntry[] {
  return [{ id: 'dry-clusters', label: () => text.t('dry.title'), run: () => list.show() }]
}
