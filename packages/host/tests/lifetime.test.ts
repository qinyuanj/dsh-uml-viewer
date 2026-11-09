import { Context } from '@deepseek-ai/cordis'
import { expect, it } from 'vitest'
import { panelLifetime } from '../src/lifetime.js'
import type { Agent, HostServices } from '../src/dsh.js'
import type { SessionStates } from '../src/state.js'

it('keeps incompatible v1 root state separate while new activations share the current pool', async () => {
  const root = new Context()
  // The pre-owned-holder activation had neither transitions nor holder records.
  const legacy = new Map([['checkout', { pool: { connections: new Map(), opening: new Map(), graces: new Map(), graceMs: 30_000 }, prompts: {} }]])
  let legacyDisposed = false
  root.effect(() => () => { legacyDisposed = true }, 'old activation cleanup')
  Object.defineProperty(root, Symbol.for('dsh-uml-viewer.panel-state.v1'), { value: legacy, configurable: true })
  const services = { sandboxPolicy: { resolve: () => ({ mode: 'read-only', workspaceRoot: process.cwd() }) } } as unknown as HostServices
  const states = {} as SessionStates
  try {
    const first = panelLifetime(root, undefined, services, {}, states)
    const next = panelLifetime(root, undefined, services, {}, states)
    // Owned open reaches the authoritative policy refusal through a valid current pool.
    const agent = { session: { id: 'session' } } as Agent
    await expect(first.sessions.openOwned(agent, new AbortController().signal, { pageId: 'page', holderId: 'holder' }))
      .rejects.toThrow(/read-only sandbox/)
    expect(first.prompts).not.toBe(legacy.get('checkout')!.prompts)
    expect(next.prompts).toBe(first.prompts)
    expect(Reflect.get(root, Symbol.for('dsh-uml-viewer.panel-state.v1'))).toBe(legacy)
  } finally {
    await root.fiber.dispose()
  }
  expect(legacyDisposed).toBe(true)
})
