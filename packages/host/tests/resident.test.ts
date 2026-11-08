import { EventEmitter } from 'node:events'
import { afterEach, expect, it, vi } from 'vitest'
import type { Agent, HostServices, SubprocessHandle } from '../src/dsh.js'
import { ResidentSessions } from '../src/resident.js'
import type { SessionStates } from '../src/state.js'

afterEach(() => vi.useRealTimers())

function fixture() {
  const stdout = new EventEmitter()
  let spawned!: (child: SubprocessHandle) => void
  const didSpawn = new Promise<SubprocessHandle>(resolve => { spawned = resolve })
  let exited!: (outcome: { exitCode: number | null; signal: string | null }) => void
  const done = new Promise<{ exitCode: number | null; signal: string | null }>(resolve => { exited = resolve })
  let terminations = 0
  const child: SubprocessHandle = {
    stdin: { write: () => true },
    stdout: stdout as unknown as SubprocessHandle['stdout'],
    collected: {},
    done,
    terminate() {
      terminations += 1
      exited({ exitCode: null, signal: 'SIGTERM' })
    },
  }
  const services = {
    sandboxPolicy: { resolve: () => ({ mode: 'workspace-write', workspaceRoot: process.cwd() }) },
    workingDirectory: { ensure: async () => process.cwd(), get: () => process.cwd() },
    subprocess: { spawn: () => { spawned(child); return child } },
    sandbox: { confine: async (argv: string[]) => ({ argv, enforcement: 'full' }) },
  } as unknown as HostServices
  const states = { read: async () => null, write: async () => undefined } as unknown as SessionStates
  const sessions = new ResidentSessions(services, { binary: [process.execPath] }, states)
  const agent = { id: 'session-a', session: { id: 'session-a' } } as Agent
  return { sessions, agent, stdout, didSpawn, terminations: () => terminations }
}

it('terminates and awaits a resident whose first line is malformed', async () => {
  const h = fixture()
  const opening = h.sessions.open(h.agent, new AbortController().signal)
  await h.didSpawn
  h.stdout.emit('data', 'not-json\n')
  await expect(opening).rejects.toThrow(/refused to start/)
  expect(h.terminations()).toBe(1)
  await h.sessions.dispose()
  expect(h.terminations()).toBe(1)
})

it('terminates and awaits a resident whose first event is not ready', async () => {
  const h = fixture()
  const opening = h.sessions.open(h.agent, new AbortController().signal)
  await h.didSpawn
  h.stdout.emit('data', '{"version":1,"seq":1,"event":"ack"}\n')
  await expect(opening).rejects.toThrow(/instead of ready/)
  expect(h.terminations()).toBe(1)
  await h.sessions.dispose()
  expect(h.terminations()).toBe(1)
})

it('terminates and awaits a resident whose readiness times out', async () => {
  vi.useFakeTimers()
  const h = fixture()
  const opening = h.sessions.open(h.agent, new AbortController().signal)
  await h.didSpawn
  const rejected = expect(opening).rejects.toThrow(/did not report ready/)
  await vi.advanceTimersByTimeAsync(20_000)
  await rejected
  expect(h.terminations()).toBe(1)
  await h.sessions.dispose()
  expect(h.terminations()).toBe(1)
})
