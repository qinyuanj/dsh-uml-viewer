import { EventEmitter } from 'node:events'
import { afterEach, expect, it, vi } from 'vitest'
import type { Agent, HostServices, SubprocessHandle } from '../src/dsh.js'
import { RESIDENT_LINE_MAX_BYTES } from '@dsh-uml-viewer/contracts'
import { ResidentConnection, ResidentSessions } from '../src/resident.js'
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

function connection(maxLineBytes?: number) {
  const writes: string[] = []
  let terminations = 0
  const child: SubprocessHandle = {
    stdin: { write: (line: string) => { writes.push(line); return true } },
    stdout: new EventEmitter() as unknown as SubprocessHandle['stdout'],
    collected: {},
    done: new Promise(() => undefined),
    terminate() { terminations += 1 },
  } as unknown as SubprocessHandle
  const resident = new ResidentConnection('s', process.cwd(), child, false, maxLineBytes)
  return { resident, writes, terminations: () => terminations }
}

const errorLine = (message: string) => `${JSON.stringify({ version: 1, seq: null, event: 'error', message })}\n`
const bytes = (text: string) => Buffer.byteLength(text, 'utf8')

it('caps resident JSONL records at the shared contracts transport limit by default', () => {
  expect(RESIDENT_LINE_MAX_BYTES).toBe(16 * 1024 * 1024)
  expect(connection().resident.maxLineBytes).toBe(RESIDENT_LINE_MAX_BYTES)
})

it('decodes a UTF-8 character split across stdout byte chunks', async () => {
  const h = connection()
  const queue = h.resident.attach()
  const raw = Buffer.from(errorLine('源码窗口'), 'utf8')
  const cut = raw.indexOf(Buffer.from('码', 'utf8')) + 1
  h.resident.ingest(raw.subarray(0, cut))
  h.resident.ingest(raw.subarray(cut, cut + 1))
  h.resident.ingest(raw.subarray(cut + 1))
  const next = await queue.next()
  expect(next.value).toMatchObject({ event: 'error', message: '源码窗口' })
})

it('accepts a record of exactly the byte limit, counting UTF-8 bytes and excluding CRLF', async () => {
  const line = errorLine('中文').slice(0, -1)
  const h = connection(bytes(line))
  const queue = h.resident.attach()
  h.resident.ingest(`${line}\r`)
  h.resident.ingest('\n')
  expect((await queue.next()).value).toMatchObject({ message: '中文' })
  expect(h.terminations()).toBe(0)
})

it('rejects pending requests, fails panels and stops the child on a record one byte over', async () => {
  const line = errorLine('中文').slice(0, -1)
  const h = connection(bytes(line) - 1)
  const queue = h.resident.attach()
  const pending = h.resident.request({ action: 'open-source', id: ':a' })
  h.resident.ingest(`${line}\n`)
  await expect(pending).rejects.toThrow(`JSONL record over ${bytes(line) - 1} bytes`)
  const failed = await queue.next()
  expect(failed.value).toMatchObject({ event: 'error', seq: null })
  expect((failed.value as { message: string }).message).toMatch(/resident was stopped/)
  expect((await queue.next()).done).toBe(true)
  expect(h.terminations()).toBe(1)
  // Settled: a new panel open starts a fresh resident instead of joining this one.
  expect(h.resident.isSettled).toBe(true)
  // The broken stream refuses later bytes and requests at once.
  h.resident.ingest(errorLine('late'))
  await expect(h.resident.request({ action: 'save' })).rejects.toThrow(/JSONL record over/)
  expect(h.writes).toHaveLength(1)
  expect(h.terminations()).toBe(1)
})

it('stops buffering an unterminated record as soon as its split chunks cross the limit', async () => {
  const h = connection(64)
  const pending = h.resident.request({ action: 'card' })
  for (let i = 0; i < 6; i += 1) h.resident.ingest('{"pad":"0123456789')
  await expect(pending).rejects.toThrow(/over 64 bytes \(at least 72\)/)
  expect(h.terminations()).toBe(1)
  h.resident.ingest('"}\n')
  expect(h.terminations()).toBe(1)
})

it('dispatches records before an oversized one in the same chunk, then stops', async () => {
  const h = connection(80)
  const queue = h.resident.attach()
  h.resident.ingest(`${errorLine('first')}${'x'.repeat(81)}\n${errorLine('never')}`)
  expect((await queue.next()).value).toMatchObject({ message: 'first' })
  expect((await queue.next()).value).toMatchObject({ message: expect.stringMatching(/over 80 bytes/) })
  expect((await queue.next()).done).toBe(true)
})

it('rejects requests still pending when the resident fails or ends', async () => {
  const h = connection()
  const failed = h.resident.request({ action: 'card' })
  h.resident.fail('uml-viewer resident exited (exit 1)')
  await expect(failed).rejects.toThrow(/exit 1/)
  const ended = h.resident.request({ action: 'inspector' })
  h.resident.end()
  await expect(ended).rejects.toThrow(/session ended/)
})
