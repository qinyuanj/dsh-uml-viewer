import { describe, expect, it } from 'vitest'
import { mountStatusBar, statusCodeText } from '../src/statusbar.js'
import { translate } from '../src/locale.js'
import { Element, makeDocument } from './dom.js'

/** A real fake-DOM bar: the module's own rendering, localized from the locale files. */
function harness() {
  const doc = makeDocument()
  const host = new Element('div', doc)
  const bar = mountStatusBar(host as unknown as HTMLElement)
  const line = (): Element => bar.element.children[0] as unknown as Element
  return { bar, host, line }
}

describe('the panel status bar (#46)', () => {
  it('is a section of its own even when it has nothing to say', () => {
    const h = harness()
    expect(h.host.children).toContain(h.bar.element)
    expect(h.bar.element.className).toContain('dsh-uml-viewer-statusbar')
    expect(h.bar.element.getAttribute('role')).toBe('status')
    // The bar stays; only its text hides.
    expect(h.line().hidden).toBe(true)
    expect(h.bar.element.dataset.tone).toBe('none')
    h.bar.dispose()
  })

  it('localizes the canvas status code the resident sends (#45)', () => {
    const h = harness()
    h.bar.setStatus({ code: 'waiting-display' })
    expect(h.line().textContent).toBe('Waiting for :display to load the diagram…')
    expect(h.bar.element.dataset.code).toBe('waiting-display')
    expect(h.bar.element.dataset.tone).toBe('info')
    h.bar.setStatus({ code: 'missing-file', path: 'examples/shop.edn' })
    expect(h.line().textContent).toBe('The IR file is not there: examples/shop.edn')
    expect(h.bar.element.dataset.code).toBe('missing-file')
    h.bar.setStatus(null)
    expect(h.line().hidden).toBe(true)
    h.bar.dispose()
  })

  it('shows a refusal over the canvas status, because it explains a dead click', () => {
    const h = harness()
    h.bar.setStatus({ code: 'waiting-display' })
    h.bar.setMessage('The request was not accepted.')
    expect(h.line().textContent).toBe('The request was not accepted.')
    expect(h.bar.element.dataset.tone).toBe('error')
    // A thunk re-reads the language, so a switch relabels it in place.
    h.bar.setMessage(() => translate('en', 'request.refused'))
    expect(h.line().textContent).toBe('The request was not accepted.')
    h.bar.setMessage(null)
    expect(h.line().textContent).toBe('Waiting for :display to load the diagram…')
    h.bar.dispose()
  })

  it('removes its element on dispose', () => {
    const h = harness()
    h.bar.dispose()
    expect(h.host.children).not.toContain(h.bar.element)
  })

  it('has the same text for the same code in Chinese', () => {
    expect(statusCodeText({ language: 'zh', t: (key, params) => translate('zh', key, params), subscribe: () => () => {} },
      { code: 'missing-file', path: 'a.edn' })).toBe('找不到 IR 文件：a.edn')
  })
})
