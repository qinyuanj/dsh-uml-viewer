import { readFileSync } from 'node:fs'
import { describe, expect, it } from 'vitest'
import type { PanelAction, PanelEvent, SceneFrame } from '@dsh-uml-viewer/contracts'
import { mountDeclutterControl } from '../src/declutter.js'
import { PanelController, type PanelStream } from '../src/panel.js'
import { fakeCanvas } from './fakes.js'

const frame = JSON.parse(readFileSync(new URL('../../../crates/frame/tests/fixtures/frames/root.frame.json', import.meta.url), 'utf8')) as SceneFrame

function deferred<T>() {
  let resolve!: (value: T) => void
  let reject!: (reason: unknown) => void
  const promise = new Promise<T>((yes, no) => { resolve = yes; reject = no })
  return { promise, resolve, reject }
}

// Only the button DOM is faked: clicks, controller bindings and acknowledgements
// go through the production modules, without a browser runtime.
async function harness(options: { rejectSave?: boolean } = {}) {
  const ack = deferred<unknown>()
  let mounted = false
  const actions: { sessionId: string; action: PanelAction }[] = []
  const button = {
    type: '', className: '', textContent: '', dataset: {}, disabled: false,
    listeners: new Set<() => void>(), removed: false,
    setAttribute() {},
    addEventListener(_name: string, listener: () => void) { this.listeners.add(listener) },
    removeEventListener(_name: string, listener: () => void) { this.listeners.delete(listener) },
    remove() { this.removed = true },
    click() { for (const listener of this.listeners) listener() },
  }
  const host = {
    ownerDocument: { createElement: () => button },
    append() {},
  } as unknown as HTMLElement
  const streams: { emit(event: PanelEvent): void }[] = []
  const controller = new PanelController({
    canvas: fakeCanvas(), paint() {},
    api: {
      watch() {
        const queue: PanelEvent[] = []
        let waiting: ReturnType<typeof deferred<IteratorResult<PanelEvent>>> | null = null
        let ended = false
        streams.push({ emit(event) {
          if (waiting) { waiting.resolve({ done: false, value: event }); waiting = null }
          else queue.push(event)
        } })
        return {
          [Symbol.asyncIterator]() { return { next: async () => {
            const value = queue.shift()
            if (value) return { done: false, value }
            if (ended) return { done: true, value: undefined }
            waiting = deferred<IteratorResult<PanelEvent>>()
            return waiting.promise
          } } },
          end() { ended = true; waiting?.resolve({ done: true, value: undefined }) },
        } satisfies PanelStream
      },
      action(sessionId, action) {
        actions.push({ sessionId, action })
        if (action.action === 'declutter') return ack.promise
        if (mounted && action.action === 'save' && options.rejectSave) return Promise.reject(new Error('save unavailable'))
        return Promise.resolve({ accepted: true, reason: null })
      },
    },
  })
  await controller.open('session-a')
  streams.at(-1)!.emit({ kind: 'frame', seq: 1, frame })
  await flush()
  const dispose = mountDeclutterControl(host, controller)
  actions.length = 0 // Opening/frame synchronization has completed before the click.
  mounted = true
  return { ack, actions, button, controller, dispose, emit: (event: PanelEvent) => streams.at(-1)!.emit(event) }
}

async function flush(): Promise<void> {
  await new Promise<void>(resolve => setImmediate(resolve))
}

const accepted = { ok: true, value: { accepted: true, reason: null } }

describe('declutter acknowledgement lifecycle', () => {
  it('saves exactly once after an active successful acknowledgement and ignores pending clicks', async () => {
    const h = await harness()
    h.button.click()
    h.button.click()
    expect(h.button.disabled).toBe(true)
    expect(h.actions.map(entry => entry.action.action)).toEqual(['declutter'])
    h.ack.resolve(accepted)
    await flush()
    expect(h.actions).toEqual([
      { sessionId: 'session-a', action: { action: 'declutter' } },
      { sessionId: 'session-a', action: { action: 'save' } },
    ])
    expect(h.button.disabled).toBe(false)
    h.dispose()
    h.controller.close()
  })

  it.each([
    { accepted: false, reason: 'refused' },
    { ok: false, error: { message: 'remote failure' } },
    null,
  ])('does not save after a non-success acknowledgement (%j)', async result => {
    const h = await harness()
    h.button.click()
    h.ack.resolve(result)
    await flush()
    expect(h.actions.map(entry => entry.action.action)).toEqual(['declutter'])
    expect(h.button.disabled).toBe(false)
    h.dispose()
    h.controller.close()
  })

  it.each([false, true])('handles a rejected declutter promise without saving (disposed=%s)', async disposed => {
    const h = await harness()
    h.button.click()
    if (disposed) h.dispose()
    h.ack.reject(new Error('declutter unavailable'))
    await flush()
    expect(h.actions.map(entry => entry.action.action)).toEqual(['declutter'])
    if (!disposed) expect(h.button.disabled).toBe(false)
    h.dispose()
    h.controller.close()
  })

  it('handles a rejected follow-up save and clears the pending button', async () => {
    const h = await harness({ rejectSave: true })
    h.button.click()
    h.ack.resolve(accepted)
    await flush()
    expect(h.actions.map(entry => entry.action.action)).toEqual(['declutter', 'save'])
    expect(h.button.disabled).toBe(false)
    h.dispose()
    h.controller.close()
  })

  it('keeps frame synchronization independent of a disposed control acknowledgement', async () => {
    const h = await harness()
    h.button.click()
    h.dispose()
    h.emit({ kind: 'frame', seq: 2, frame })
    await flush()
    expect(h.actions.map(entry => entry.action.action)).toEqual(['declutter', 'save'])
    h.ack.resolve(accepted)
    await flush()
    expect(h.actions.map(entry => entry.action.action)).toEqual(['declutter', 'save'])
    h.controller.close()
  })

  it('invalidates a pending acknowledgement and further sends when the controller closes', async () => {
    const h = await harness()
    h.button.click()
    h.controller.close()
    h.ack.resolve(accepted)
    await flush()
    expect(await h.controller.send({ action: 'save' })).toBeNull()
    expect(h.actions.map(entry => entry.action.action)).toEqual(['declutter'])
    h.dispose()
  })

  it('does not save when an accepted acknowledgement arrives after disposal', async () => {
    const h = await harness()
    h.button.click()
    h.dispose()
    h.ack.resolve(accepted)
    await flush()
    expect(h.actions.map(entry => entry.action.action)).toEqual(['declutter'])
    expect(h.button.removed).toBe(true)
    h.controller.close()
  })

  it.each(['session-b', 'session-a'])('does not save into a replacement binding (%s)', async sessionId => {
    const h = await harness()
    h.button.click()
    await h.controller.open(sessionId)
    h.actions.length = 0 // The replacement binding has its own viewport action.
    h.ack.resolve(accepted)
    await flush()
    expect(h.actions).toEqual([])
    h.dispose()
    h.controller.close()
  })
})
