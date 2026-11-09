import { afterEach, expect, it } from 'vitest'
import { ResidentConnection, ResidentPool, ResidentSessions } from '../src/resident.js'
import { ViewerService } from '../src/viewer.js'
import type { Agent, HostServices, SubprocessHandle } from '../src/dsh.js'
import type { SessionStates } from '../src/state.js'

const cleanups: (() => Promise<void>)[] = []
afterEach(async () => { for (const cleanup of cleanups.splice(0)) await cleanup() })
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
