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

  // #51: the bar shows the current request, localized.
  it('shows the request that changed last, and on a fresh list the oldest unfinished one', () => {
    const h = harness()
    const request = (): Element => h.bar.element.children[1] as unknown as Element
    expect(request().hidden).toBe(true)
    const crap = { id: 'a', op: 'refresh-crap', target: { id: ':core.order', kind: 'class' }, state: 'queued', reason: null, exitCode: null } as const
    h.bar.setRequests([crap])
    expect(request().textContent).toBe('Refresh CRAP :core.order: Queued')
    expect(request().hidden).toBe(false)
    expect(h.bar.element.dataset.request).toBe('queued')
    // A refusal right after the click is what the user needs to see, even
    // while an earlier request still waits.
    const failed = { id: 'b', op: 'refresh-mutate', target: null, state: 'failed', reason: 'mutate failed', exitCode: 2 } as const
    h.bar.setRequests([crap, failed])
    expect(request().textContent).toBe('Differential mutation: Failed — mutate failed (exit code 2)')
    expect(h.bar.element.dataset.request).toBe('failed')
    // The same list again (a re-push) keeps the line.
    h.bar.setRequests([crap, failed])
    expect(request().textContent).toBe('Differential mutation: Failed — mutate failed (exit code 2)')
    const context = { id: 'c', op: 'context', target: null, state: 'done', reason: null, exitCode: null } as const
    h.bar.setRequests([crap, failed, context])
    expect(request().textContent).toBe('Switch discussion context: Done')
    // A stream opened again (a page refresh) has no history: the oldest
    // unfinished request is the current one, else the newest.
    const fresh = harness()
    const line = (): Element => fresh.bar.element.children[1] as unknown as Element
    fresh.bar.setRequests([failed, crap, context])
    expect(line().textContent).toBe('Refresh CRAP :core.order: Queued')
    fresh.bar.setRequests([])
    expect(line().hidden).toBe(true)
    fresh.bar.setRequests([failed, context])
    expect(line().textContent).toBe('Switch discussion context: Done')
    h.bar.dispose()
    fresh.bar.dispose()
  })

  it('says a refusal once when the request status already carries its reason', () => {
    const h = harness()
    const request = (): Element => h.bar.element.children[1] as unknown as Element
    h.bar.setMessage('session is archived')
    h.bar.setRequests([{ id: 'a', op: 'refresh-crap', target: null, state: 'failed', reason: 'session is archived', exitCode: null }])
    expect(h.line().hidden).toBe(true)
    expect(request().textContent).toBe('Refresh CRAP: Failed — session is archived')
    // A refusal the request status does not carry is still the panel's own line.
    h.bar.setMessage('network down')
    expect(h.line().textContent).toBe('network down')
    h.bar.dispose()
  })

  it('words the request line in Chinese with the glossary terms', () => {
    const doc = makeDocument()
    const host = new Element('div', doc)
    const bar = mountStatusBar(host as unknown as HTMLElement,
      { text: { language: 'zh', t: (key, params) => translate('zh', key, params), subscribe: () => () => {} } })
    bar.setRequests([{ id: 'a', op: 'omit', target: { id: ':api', kind: 'class' }, state: 'queued', reason: null, exitCode: null }])
    expect((bar.element.children[1] as unknown as Element).textContent).toBe('隐藏 :api：已排队')
    bar.setRequests([{ id: 'b', op: 'refresh-crap', target: null, state: 'failed', reason: '会话已归档', exitCode: null }])
    expect((bar.element.children[1] as unknown as Element).textContent).toBe('刷新 CRAP：失败——会话已归档')
    bar.dispose()
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
