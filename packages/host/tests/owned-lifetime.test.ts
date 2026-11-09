import { afterEach, expect, it, vi } from 'vitest'
import { ResidentConnection, ResidentPool, ResidentSessions } from '../src/resident.js'
import { ViewerService } from '../src/viewer.js'
import type { Agent, HostServices, SubprocessHandle } from '../src/dsh.js'
import type { SessionStates } from '../src/state.js'

const cleanups: (() => Promise<void>)[] = []
afterEach(async () => {
  for (const cleanup of cleanups.splice(0)) await cleanup()
  vi.useRealTimers()
})
function setup() {
  const pool = new ResidentPool()
  const connection = new ResidentConnection('session', process.cwd(), {
    done: Promise.resolve({ exitCode: 0, signal: null }), terminate() {},
  } as unknown as SubprocessHandle, false)
  const requests: string[] = []
  connection.request = async action => { requests.push(action.action); return { version: 1, seq: null, event: 'error', message: 'memory fixture' } }
  pool.connections.set('session', connection)
  const sessions = new ResidentSessions({ sandboxPolicy: { resolve: () => ({ mode: 'danger-full-access', workspaceRoot: process.cwd() }) } } as unknown as HostServices,
    {}, { read: async () => null } as unknown as SessionStates, pool)
  const agent = { session: { id: 'session' } } as Agent
  const viewer = new ViewerService(sessions, { openPanel() {}, closePanel() {} } as any, {} as any)
  cleanups.push(() => sessions.dispose())
  return { pool, connection, sessions, viewer, agent, requests }
}
const owner = (holderId: string, pageId = 'page-a') => ({ pageId, holderId })
it('an action borrowed before the last withdrawal preserves the resident and reattach lease', async () => {
  const h = setup()
  const signal = new AbortController().signal
  const first = owner('first')
  await h.sessions.openOwned(h.agent, signal, first)
  const borrowed = await h.sessions.open(h.agent, signal, false)
  await h.sessions.releaseOwned(h.connection, first)
  await h.sessions.release(borrowed, 'borrowed')
  expect(h.requests).not.toContain('quit')
  expect(h.pool.graces.size).toBe(1)
  expect(await h.sessions.openOwned(h.agent, signal, owner('again'))).toBe(h.connection)
  expect(h.pool.graces.size).toBe(0)
  expect(h.connection.refs).toBe(1)
})

it('a later definite last close wins over an earlier withdrawal with a borrow in flight', async () => {
  const h = setup()
  const signal = new AbortController().signal
  await h.sessions.openOwned(h.agent, signal, owner('first'))
  await h.sessions.openOwned(h.agent, signal, owner('last'))
  const borrowed = await h.sessions.open(h.agent, signal, false)
  await h.sessions.releaseOwned(h.connection, owner('first'))
  expect(h.pool.graces.size).toBe(0)
  expect(await h.sessions.closeOwned(h.agent, owner('last'))).toBe(true)
  await h.sessions.releaseOwned(h.connection, owner('last'))
  await h.sessions.release(borrowed, 'borrowed')
  expect(h.requests.filter(action => action === 'quit')).toHaveLength(1)
  expect(h.pool.graces.size).toBe(0)
  expect(h.connection.refs).toBe(0)
})

it('last close acknowledges a stopped resident even when a previous action still borrows it', async () => {
  const h = setup()
  const signal = new AbortController().signal
  await h.sessions.openOwned(h.agent, signal, owner('first'))
  const borrowed = await h.sessions.open(h.agent, signal, false)
  expect(await h.sessions.closeOwned(h.agent, owner('first'))).toBe(true)
  // An immediate Open must select the new activation, never rejoin this child.
  expect(h.pool.connections.has('session')).toBe(false)
  expect(h.requests).toContain('quit')
  await h.sessions.release(borrowed, 'borrowed')
  await h.sessions.releaseOwned(h.connection, owner('first'))
  expect(h.requests.filter(action => action === 'quit')).toHaveLength(1)
  expect(h.connection.refs).toBe(0)
})

it('closing the withdrawn last holder cancels grace even while an action borrows it', async () => {
  const h = setup()
  const signal = new AbortController().signal
  await h.sessions.openOwned(h.agent, signal, owner('first'))
  const borrowed = await h.sessions.open(h.agent, signal, false)
  await h.sessions.releaseOwned(h.connection, owner('first'))
  expect(await h.sessions.closeOwned(h.agent, owner('first'))).toBe(true)
  await h.sessions.release(borrowed, 'borrowed')
  expect(h.requests).toContain('quit')
  expect(h.pool.graces.size).toBe(0)
})

it('legacy watches and owned holders count separately from inflight borrows', async () => {
  const h = setup()
  const signal = new AbortController().signal
  await h.sessions.open(h.agent, signal)
  await h.sessions.openOwned(h.agent, signal, owner('owned'))
  const borrowed = await h.sessions.open(h.agent, signal, false)
  await h.sessions.releaseOwned(h.connection, owner('owned'))
  expect(h.pool.graces.size).toBe(0)
  await h.sessions.release(h.connection, 'withdrawn')
  expect(h.pool.graces.size).toBe(1)
  await h.sessions.release(borrowed, 'borrowed')
  expect(h.requests).not.toContain('quit')
  expect(h.connection.refs).toBe(0)
})

it('an inflight borrow never extends the withdrawal deadline and shutdown drains late releases', async () => {
  vi.useFakeTimers()
  const h = setup()
  const signal = new AbortController().signal
  await h.sessions.openOwned(h.agent, signal, owner('first'))
  const borrowed = await h.sessions.open(h.agent, signal, false)
  await h.sessions.releaseOwned(h.connection, owner('first'))
  await vi.advanceTimersByTimeAsync(h.pool.graceMs)
  expect(h.pool.graces.size).toBe(0)
  expect(h.requests).not.toContain('quit')
  await h.sessions.release(borrowed, 'borrowed')
  expect(h.requests).toContain('quit')
  await h.sessions.dispose()
  await h.sessions.releaseOwned(h.connection, owner('first'))
  await h.sessions.release(borrowed, 'borrowed')
  expect(h.connection.refs).toBe(0)
  expect(h.pool.graces.size).toBe(0)
  expect(h.pool.connections.size).toBe(0)
})

it('a late action from a withdrawn panel neither stops the resident nor cancels its grace', async () => {
  const h = setup()
  const abort = new AbortController()
  const stream = h.viewer.watchOwned(h.agent, owner('first'), abort.signal)[Symbol.asyncIterator]()
  await stream.next()
  abort.abort()
  await stream.return!()
  expect(h.pool.graces.size).toBe(1)
  await h.viewer.action(h.agent, { action: 'save' })
  await h.viewer.action(h.agent, { action: 'inspector' })
  expect(h.requests).not.toContain('quit')
  expect(h.pool.connections.get('session')).toBe(h.connection)
  expect(h.connection.refs).toBe(0)
  expect(h.pool.graces.size).toBe(1)
})

it('a delayed old holder close cannot poison the replacement holder withdrawal', async () => {
  const h = setup()
  const firstAbort = new AbortController()
  const first = h.viewer.watchOwned(h.agent, owner('first'), firstAbort.signal)[Symbol.asyncIterator]()
  await first.next()
  firstAbort.abort()
  await first.return!()
  const secondAbort = new AbortController()
  const second = h.viewer.watchOwned(h.agent, owner('second'), secondAbort.signal)[Symbol.asyncIterator]()
  await second.next()
  expect(await h.viewer.closeOwned(h.agent, owner('first'))).toEqual({ closed: false })
  secondAbort.abort()
  await second.return!()
  expect(h.pool.connections.get('session')).toBe(h.connection)
  expect(h.pool.graces.size).toBe(1)
  expect(h.requests).not.toContain('quit')
})
