import { EventEmitter } from 'node:events'
import { readFileSync } from 'node:fs'
import { afterEach, expect, it, vi } from 'vitest'
import type { Agent, HostServices, SubprocessHandle } from '../src/dsh.js'
import { RESIDENT_LINE_MAX_BYTES } from '@dsh-uml-viewer/contracts'
import { PANEL_QUEUE_MAX_BYTES, PANEL_QUEUE_MAX_ITEMS, ResidentConnection, ResidentSessions } from '../src/resident.js'
import type { QueueSize } from '../src/resident.js'
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

function connection(maxLineBytes?: number, queueMax?: QueueSize) {
  const writes: string[] = []
  let terminations = 0
  const child: SubprocessHandle = {
    stdin: { write: (line: string) => { writes.push(line); return true } },
    stdout: new EventEmitter() as unknown as SubprocessHandle['stdout'],
    collected: {},
    done: new Promise(() => undefined),
    terminate() { terminations += 1 },
  } as unknown as SubprocessHandle
  const resident = new ResidentConnection('s', process.cwd(), child, false, maxLineBytes, queueMax)
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
  const ended = h.resident.request({ action: 'search', query: 'x' })
  h.resident.end()
  await expect(ended).rejects.toThrow(/session ended/)
})

// #review H2: one stalled panel subscriber must not grow host memory without
// bound. Each panel stream gets its own queue, and a queue that falls more than
// a few frames behind keeps the newest frames only.
const goldenFrame = readFileSync(
  new URL('../../../crates/frame/tests/fixtures/frames/root.frame.json', import.meta.url), 'utf8',
)
const frameLine = (seq: number) => `${JSON.stringify({ version: 1, seq, event: 'frame', frame: JSON.parse(goldenFrame) })}\n`

it('caps every panel queue at 64 unread events or 8 MiB by default', () => {
  expect(connection().resident.queueMax).toEqual({ items: PANEL_QUEUE_MAX_ITEMS, bytes: PANEL_QUEUE_MAX_BYTES })
  expect(PANEL_QUEUE_MAX_ITEMS).toBe(64)
  expect(PANEL_QUEUE_MAX_BYTES).toBe(8 * 1024 * 1024)
})

it('delivers the latest frame and critical context after repeated drain and refill', async () => {
  const h = connection(undefined, { items: 4, bytes: bytes(frameLine(1)) * 2 })
  const queue = h.resident.attach()
  for (let seq = 1; seq <= 8; seq += 1) {
    h.resident.ingest(frameLine(seq))
    h.resident.ingest(`${JSON.stringify({ version: 1, seq: null, event: 'context', context: { kind: 'real', edn: '{:context :real}' } })}\n`)
    // End makes a lost frame fail immediately instead of leaving next() blocked.
    if (seq === 8) queue.end()
    expect((await queue.next(AbortSignal.timeout(100))).value).toMatchObject({ event: 'frame', seq })
    expect((await queue.next(AbortSignal.timeout(100))).value).toMatchObject({ event: 'context', context: { kind: 'real' } })
  }
  expect((await queue.next()).done).toBe(true)
})

it('drops the oldest unread frames of a stalled panel, keeping the newest', async () => {
  const h = connection(undefined, { items: 3, bytes: 1 << 24 })
  const queue = h.resident.attach()
  for (let seq = 1; seq <= 5; seq += 1) h.resident.ingest(frameLine(seq))
  const seen: number[] = []
  for (let read = 0; read < 3; read += 1) {
    seen.push(((await queue.next()).value as unknown as { seq: number }).seq)
  }
  expect(seen).toEqual([3, 4, 5])
  // Nothing else was buffered: the three oldest frames are gone.
  h.resident.detach(queue)
  expect((await queue.next()).done).toBe(true)
  expect(h.terminations()).toBe(0)
})

it('reports overflow when the sole latest frame exceeds the byte cap', async () => {
  const h = connection(undefined, { items: 64, bytes: bytes(frameLine(1)) - 2 })
  const queue = h.resident.attach()
  h.resident.ingest(frameLine(1))
  queue.end()
  // The newest frame is not superseded: preserve it and explain why the stream ended.
  expect((await queue.next()).value).toMatchObject({ event: 'frame', seq: 1 })
  expect((await queue.next()).value).toMatchObject({ event: 'error', message: expect.stringMatching(/fell behind/) })
  expect((await queue.next()).done).toBe(true)
  expect(h.terminations()).toBe(0)
})

it('drops unread frames by bytes as well, oldest first', async () => {
  const line = frameLine(1)
  const h = connection(undefined, { items: 1024, bytes: Buffer.byteLength(line, 'utf8') * 2 })
  const queue = h.resident.attach()
  for (let seq = 1; seq <= 3; seq += 1) h.resident.ingest(frameLine(seq))
  expect((await queue.next()).value).toMatchObject({ event: 'frame', seq: 2 })
  expect((await queue.next()).value).toMatchObject({ event: 'frame', seq: 3 })
  h.resident.detach(queue)
  expect((await queue.next()).done).toBe(true)
})

it('never truncates ready, refusal or error events: an over-full panel stream ends with a reason', async () => {
  const h = connection(undefined, { items: 2, bytes: 1 << 24 })
  const queue = h.resident.attach()
  for (const message of ['one', 'two', 'three', 'four']) h.resident.ingest(errorLine(message))
  expect((await queue.next()).value).toMatchObject({ event: 'error', message: 'one' })
  expect((await queue.next()).value).toMatchObject({ event: 'error', message: 'two' })
  expect((await queue.next()).value).toMatchObject({ event: 'error', message: 'three' })
  const last = await queue.next()
  expect(last.done).toBe(false)
  expect(last.value).toMatchObject({ event: 'error', seq: null })
  expect((last.value as unknown as { message: string }).message).toMatch(/fell behind: 3 unread events/)
  expect((await queue.next()).done).toBe(true)
  // That panel stream ended; the resident itself is untouched.
  expect(h.terminations()).toBe(0)
})
