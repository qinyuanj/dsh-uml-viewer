// #review H3: one panel request must never hold its session's FIFO forever.
// The admission (the resident's answer plus DSH's prompt) runs under one
// deadline, and the signal it aborts is the one every step of that request
// receives, so a step that never settles fails this request with a reason and
// the requests queued behind it still run.
import { afterEach, expect, it, vi } from 'vitest'
import type { Agent, HostServices, SessionPromptRequest } from '../src/dsh.js'
import { RequestGrants } from '../src/gate.js'
import { ADMISSION_TIMEOUT_MS, PanelRequests } from '../src/requests.js'
import type { ResidentSessions } from '../src/resident.js'

afterEach(() => vi.useRealTimers())

const agent = { id: 'session-a', session: { id: 'session-a' } } as Agent
const target = { id: ':api', kind: 'class' as const, files: ['src/api.ts'] }

function setup(options: {
  prompt?: (request: SessionPromptRequest, signal: AbortSignal) => Promise<{ accepted: true }>
  answer?: (signal: AbortSignal) => Promise<typeof target>
} = {}) {
  const admitted: string[] = []
  const signals: AbortSignal[] = []
  const prompt = options.prompt ?? (async (request: SessionPromptRequest, signal: AbortSignal) => {
    signals.push(signal)
    admitted.push(request.content[0]!.text)
    return { accepted: true as const }
  })
  const services = { sessionController: { prompt } } as unknown as HostServices
  const sessions = {
    qualityTarget: async (_agent: Agent, signal: AbortSignal) => {
      signals.push(signal)
      return options.answer ? await options.answer(signal) : target
    },
  } as unknown as ResidentSessions
  return { requests: new PanelRequests(services, sessions, new RequestGrants(), ADMISSION_TIMEOUT_MS), admitted, signals }
}

it('bounds one admission at 20 s, like one resident action', () => {
  expect(ADMISSION_TIMEOUT_MS).toBe(20_000)
})

it('abandons a prompt that never resolves and admits the request queued behind it', async () => {
  vi.useFakeTimers()
  const s = setup({
    prompt: async request => request.content[0]!.text.includes(':refresh-crap')
      ? await new Promise<never>(() => {})
      : { accepted: true as const },
  })
  const first = s.requests.deliver(agent, 'refresh-crap', ':api')
  const second = s.requests.deliver(agent, 'refresh-mutate', ':api')
  await vi.advanceTimersByTimeAsync(ADMISSION_TIMEOUT_MS)
  expect(await first).toEqual({ accepted: false, reason: expect.stringMatching(/not admitted within 20s/) })
  // The FIFO moved on: the request queued behind the abandoned one still runs.
  expect(await second).toEqual({ accepted: true, reason: null })
})

it('passes the one abortable signal to the resident answer and aborts it at the deadline', async () => {
  vi.useFakeTimers()
  const s = setup({ answer: async () => await new Promise<never>(() => {}) })
  const answer = s.requests.deliver(agent, 'refresh-crap', ':api')
  await vi.advanceTimersByTimeAsync(ADMISSION_TIMEOUT_MS)
  expect(await answer).toEqual({ accepted: false, reason: expect.stringMatching(/not admitted within 20s/) })
  expect(s.signals).toHaveLength(1)
  expect(s.signals[0]!.aborted).toBe(true)
  // Nothing was queued: a request that never resolved names no target.
  expect(s.admitted).toEqual([])
})
