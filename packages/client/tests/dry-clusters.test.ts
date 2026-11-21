import { describe, expect, it, vi } from 'vitest'
import type { SceneFrame } from '@dsh-uml-viewer/contracts'
import { dryClusterEntries, mountDryClusters } from '../src/dry-clusters.js'
import { ENGLISH_TEXT } from '../src/locale.js'
import { Element, makeDocument } from './dom.js'

function harness() {
  const doc = makeDocument()
  const panel = new Element('div', doc)
  const locate = vi.fn()
  const list = mountDryClusters(panel as unknown as HTMLElement, { text: ENGLISH_TEXT, locate })
  const root = list.element as unknown as Element
  const text = (): string => root.descendants().map(element => element.textContent).join(' ')
  return { list, root, locate, text }
}

const summary: NonNullable<SceneFrame['dry']> = {
  candidates: 2,
  attributed: 2,
  unattributed: 3,
  clusters: 1,
  shownClusters: 1,
  truncated: false,
  weakest: 0.83,
  clusterList: [{
    members: [{ file: 'src/a.rs', startLine: 1, endLine: 5, name: 'f1', class: ':scene.a' }],
    classes: [':scene.a'],
    weakest: 0.83,
  }],
}

function frame(dry: SceneFrame['dry']): SceneFrame {
  return { dry } as SceneFrame
}

describe('duplicate structure cluster list (#91)', () => {
  it('names all three states instead of hiding an empty measurement', () => {
    const { list, root, locate, text } = harness()
    expect(list.element.hidden).toBe(true)
    list.render(frame(undefined))
    list.show()
    expect(list.isOpen()).toBe(true)
    expect(text()).toContain('Unmeasured')
    list.render(frame({ ...summary, clusterList: [], clusters: 0, shownClusters: 0 }))
    expect(text()).toContain('Measured: no duplicate clusters')
    list.render(frame(summary))
    expect(text()).toContain('Duplicate clusters: 1')
    expect(text()).toContain('Candidates outside the diagram: 3')
    expect(text()).toContain('src/a.rs')
    expect(text()).toContain('0.83')
    const button = root.descendants().find(element => element.tagName === 'button' && element.className !== 'dsh-uml-dry-close')!
    button.fire('click')
    expect(locate).toHaveBeenCalledWith(':scene.a')
  })

  it('keeps the count of unmatched candidates even when there is no cluster', () => {
    const { list, text } = harness()
    list.render(frame({ ...summary, clusterList: [], clusters: 0, shownClusters: 0, unattributed: 5 }))
    expect(text()).toContain('Candidates outside the diagram: 5')
  })

  it('offers a member that joins no class as a name, not a link', () => {
    const { list, root, locate, text } = harness()
    list.render(frame({
      ...summary,
      clusterList: [{
        members: [{ file: 'src/b.rs', startLine: 2, endLine: 6, name: 'g', class: null }],
        classes: [],
        weakest: 0.9,
      }],
    }))
    const button = root.descendants().find(element => element.tagName === 'button' && element.className !== 'dsh-uml-dry-close')!
    expect(button.disabled).toBe(true)
    expect(button.getAttribute('aria-label')).toBe('Not on the diagram')
    button.fire('click')
    expect(locate).not.toHaveBeenCalled()
  })

  it('says when the list is truncated instead of hiding the rest', () => {
    const { list, text } = harness()
    list.render(frame({ ...summary, clusters: 90, shownClusters: 1, truncated: true }))
    expect(text()).toContain('Showing 1 of 90 clusters')
  })

  it('offers the list in the more menu', () => {
    const { list } = harness()
    const entries = dryClusterEntries(list, ENGLISH_TEXT)
    expect(entries.map(entry => entry.id)).toEqual(['dry-clusters'])
    expect(entries[0]!.label()).toBe('Duplicate clusters')
    entries[0]!.run()
    expect(list.isOpen()).toBe(true)
  })
})
