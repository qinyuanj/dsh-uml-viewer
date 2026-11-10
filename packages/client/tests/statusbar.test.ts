import { afterEach, describe, expect, it, vi } from 'vitest'
import type { RequestStatusEntry } from '@dsh-uml-viewer/contracts'
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

  // #51/#53/F23: the current request is the list's own choice — an unseen
  // failure, else the running head, else the oldest queued, else the newest.
  it('shows an unseen failure, else the running head, else the oldest queued, else the newest', () => {
    const h = harness()
    const request = (): Element => h.bar.element.children[1] as unknown as Element
    expect(request().hidden).toBe(true)
    const crap = { id: 'a', op: 'refresh-crap', target: { id: ':core.order', kind: 'class' }, state: 'queued', reason: null, exitCode: null } as const
    const mutate = { id: 'd', op: 'refresh-mutate', target: { id: ':core', kind: 'component' }, state: 'queued', reason: null, exitCode: null } as const
    h.bar.setRequests([crap])
    expect(request().textContent).toBe('Refresh CRAP :core.order: Queued')
    expect(request().hidden).toBe(false)
    expect(h.bar.element.dataset.request).toBe('queued')
    // A refusal right after the click is what the user must not miss, even
    // while an earlier request still waits.
    const failed = { id: 'b', op: 'refresh-mutate', target: null, state: 'failed', reason: 'mutate failed', exitCode: 2 } as const
    h.bar.setRequests([crap, failed])
    expect(request().textContent).toBe('Differential mutation: Failed — mutate failed (exit code 2)')
    expect(h.bar.element.dataset.request).toBe('failed')
    // The same list again (a re-push on the stream) keeps what the user sees.
    h.bar.setRequests([crap, failed])
    expect(request().textContent).toBe('Differential mutation: Failed — mutate failed (exit code 2)')
    // Once shown, a failure stops jumping the queue: the next change shows the
    // requests that are still moving.
    h.bar.setRequests([crap, failed, { ...crap, id: 'e' }])
    expect(request().textContent).toBe('Refresh CRAP :core.order: Queued')
    // A request in progress wins over the requests waiting behind it.
    const running = { ...mutate, state: 'running' } as const
    h.bar.setRequests([crap, failed, running, { ...crap, id: 'e' }])
    expect(request().textContent).toBe('Differential mutation :core: In progress')
    expect(h.bar.element.dataset.request).toBe('running')
    // An unseen failure still wins over the running head.
    const second = { ...failed, id: 'f', reason: 'ir failed', exitCode: null } as const
    h.bar.setRequests([crap, failed, running, second])
    expect(request().textContent).toBe('Differential mutation: Failed — ir failed')
    // Nothing unfinished: the newest entry.
    const done = { id: 'c', op: 'context', target: null, state: 'done', reason: null, exitCode: null } as const
    const fresh = harness()
    const line = (): Element => fresh.bar.element.children[1] as unknown as Element
    fresh.bar.setRequests([done, { ...done, id: 'g' }])
    expect(line().textContent).toBe('Switch discussion context: Done')
    fresh.bar.setRequests([])
    expect(line().hidden).toBe(true)
    h.bar.dispose()
    fresh.bar.dispose()
  })

  // F23: the same list shows the same entry however the panel got there, so a
  // refreshed page and an open page agree.
  it('picks the same entry for the same list whether or not the bar saw it change', () => {
    const entries = [
      { id: 'a', op: 'refresh-crap', target: { id: ':core.order', kind: 'class' }, state: 'done', reason: null, exitCode: null },
      { id: 'b', op: 'refresh-mutate', target: { id: ':core', kind: 'component' }, state: 'queued', reason: null, exitCode: null },
      { id: 'c', op: 'refresh-crap', target: { id: ':api', kind: 'class' }, state: 'running', reason: null, exitCode: null },
    ] as const
    const open = harness()
    for (const entry of entries) open.bar.setRequests(entries.slice(0, entries.indexOf(entry) + 1))
    const fresh = harness()
    fresh.bar.setRequests(entries)
    const current = (h: ReturnType<typeof harness>): string => (h.bar.element.children[1] as unknown as Element).textContent
    expect(current(open)).toBe('Refresh CRAP :api: In progress')
    expect(current(fresh)).toBe(current(open))
    open.bar.dispose()
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

// #53: the queue behind the current request. The bar shows the current entry
// plus 「还有 N 条」, and the list opens on a click.
const href = (id: string, op: RequestStatusEntry['op'] = 'refresh-crap',
  state: RequestStatusEntry['state'] = 'queued'): RequestStatusEntry =>
  ({ id, op, target: { id: `:${id}`, kind: 'class' }, state, reason: state === 'failed' ? 'why' : null, exitCode: null })

function queueHarness() {
  const doc = makeDocument()
  const host = new Element('div', doc)
  const dismissed: string[] = []
  const bar = mountStatusBar(host as unknown as HTMLElement, { onDismiss: id => dismissed.push(id) })
  const request = (): Element => bar.element.children[1] as unknown as Element
  const more = (): Element => bar.element.children[2] as unknown as Element
  const list = (): Element | null => (bar.element.children[3] as unknown as Element | undefined) ?? null
  return { bar, host, request, more, list, dismissed }
}

describe('the request queue (#53)', () => {
  it('says how many more requests wait behind the current one', () => {
    const h = queueHarness()
    h.bar.setRequests([href('a'), href('b'), href('c')])
    expect(h.request().textContent).toBe('Refresh CRAP :a: Queued')
    expect(h.more().hidden).toBe(false)
    expect(h.more().textContent).toBe('2 more')
    expect(h.more().getAttribute('aria-expanded')).toBe('false')
    expect(h.more().getAttribute('aria-label')).toBe('Show the other 2 requests')
    h.bar.setRequests([href('a'), href('b')])
    expect(h.more().textContent).toBe('1 more')
    // One request is the whole queue: there is nothing to open.
    h.bar.setRequests([href('a')])
    expect(h.more().hidden).toBe(true)
    expect(h.list()).toBeNull()
    h.bar.setRequests([])
    expect(h.more().hidden).toBe(true)
    h.bar.dispose()
  })

  it('opens the whole queue as a list, and closes it again', () => {
    const h = queueHarness()
    h.bar.setRequests([href('a'), href('b', 'omit'), href('c', 'refresh-mutate', 'failed')])
    expect(h.list()).toBeNull()
    h.more().fire('click')
    const shown = h.list()!
    expect(shown.getAttribute('role')).toBe('list')
    expect(shown.getAttribute('aria-label')).toBe('Request queue')
    const items = (): Element[] => shown.children.filter(item => item.getAttribute('role') === 'listitem')
    // Each item is its line plus, for a failure, the dismissal.
    expect(items().map(item => item.children[0]!.textContent)).toEqual([
      'Refresh CRAP :a: Queued', 'Hide :b: Queued', 'Differential mutation :c: Failed — why'])
    expect(h.more().getAttribute('aria-expanded')).toBe('true')
    h.more().fire('click')
    expect(h.list()).toBeNull()
    expect(h.more().getAttribute('aria-expanded')).toBe('false')
    h.bar.dispose()
  })

  it('keeps the list open across a status change and relabels it in the other language', () => {
    const doc = makeDocument()
    const host = new Element('div', doc)
    const language = { active: 'en' }
    const text = { get language() { return language.active as 'en' | 'zh' },
      t: (key: Parameters<typeof translate>[1], params?: Record<string, string>) => translate(language.active as 'en' | 'zh', key, params),
      subscribe: () => () => {} }
    const bar = mountStatusBar(host as unknown as HTMLElement, { text })
    const more = (): Element => bar.element.children[2] as unknown as Element
    bar.setRequests([href('a'), href('b')])
    more().fire('click')
    const open = (): Element => bar.element.children[3] as unknown as Element
    expect(open().children.map(item => item.children[0]!.textContent)).toEqual(['Refresh CRAP :a: Queued', 'Refresh CRAP :b: Queued'])
    bar.setRequests([href('a'), href('b'), href('c')])
    expect(more().textContent).toBe('2 more')
    expect(open().children).toHaveLength(3)
    language.active = 'zh'
    bar.setRequests([href('a'), href('b', 'omit'), href('c')])
    expect(more().textContent).toBe('还有 2 条')
    expect(more().getAttribute('aria-label')).toBe('查看其余 2 条请求')
    expect(open().getAttribute('aria-label')).toBe('请求队列')
    bar.dispose()
  })
})

describe('the completed fade (#53)', () => {
  const crap = { id: 'a', op: 'refresh-crap', target: { id: ':a', kind: 'class' }, state: 'queued', reason: null, exitCode: null } as const
  const done = { ...crap, state: 'done' } as const
  const next = { id: 'b', op: 'omit', target: { id: ':b', kind: 'class' }, state: 'queued', reason: null, exitCode: null } as const

  afterEach(() => { vi.useRealTimers() })

  it('fades a completed entry ten seconds after it finishes', () => {
    vi.useFakeTimers()
    const h = queueHarness()
    h.bar.setRequests([done])
    expect(h.request().textContent).toBe('Refresh CRAP :a: Done')
    vi.advanceTimersByTime(9_999)
    expect(h.request().textContent).toBe('Refresh CRAP :a: Done')
    vi.advanceTimersByTime(1)
    expect(h.request().hidden).toBe(true)
    expect(h.request().textContent).toBe('')
    expect(h.bar.element.dataset.request).toBe('')
    expect(h.more().hidden).toBe(true)
    // The host still lists it; the fade is this bar's own.
    expect(h.bar.element.dataset.requestCount).toBe('0')
    h.bar.setRequests([done])
    expect(h.request().hidden).toBe(true)
    h.bar.dispose()
  })

  it('fades a completed entry as soon as the next request starts', () => {
    vi.useFakeTimers()
    const h = queueHarness()
    h.bar.setRequests([done])
    expect(h.request().textContent).toBe('Refresh CRAP :a: Done')
    h.bar.setRequests([done, next])
    expect(h.request().textContent).toBe('Hide :b: Queued')
    expect(h.more().hidden).toBe(true)
    // Gone for good, even when the list arrives again.
    h.bar.setRequests([done])
    expect(h.request().hidden).toBe(true)
    h.bar.dispose()
  })

  it('keeps a failed entry until the user dismisses it, which is a panel action', () => {
    vi.useFakeTimers()
    const h = queueHarness()
    const failed = { id: 'f', op: 'refresh-mutate', target: null, state: 'failed', reason: 'mutate failed', exitCode: 2 } as const
    h.bar.setRequests([crap, failed])
    expect(h.request().textContent).toBe('Differential mutation: Failed — mutate failed (exit code 2)')
    // Ten seconds, and the failure is still there.
    vi.advanceTimersByTime(30_000)
    expect(h.request().textContent).toBe('Differential mutation: Failed — mutate failed (exit code 2)')
    h.more().fire('click')
    const items = h.list()!.children
    expect(items.map(item => item.dataset.id)).toEqual(['a', 'f'])
    // Only the failure can be dismissed: the queued one is still coming.
    expect(items[0]!.children).toHaveLength(1)
    expect(items[1]!.children).toHaveLength(2)
    const dismiss = items[1]!.children[1] as Element
    expect(dismiss.tagName).toBe('button')
    expect(dismiss.getAttribute('aria-label')).toBe('Dismiss Differential mutation: Failed — mutate failed (exit code 2)')
    dismiss.fire('click')
    expect(h.dismissed).toEqual(['f'])
    // The host answers with the list without the dismissed entry.
    h.bar.setRequests([crap])
    expect(h.list()).toBeNull()
    expect(h.request().textContent).toBe('Refresh CRAP :a: Queued')
    h.bar.dispose()
  })
})
