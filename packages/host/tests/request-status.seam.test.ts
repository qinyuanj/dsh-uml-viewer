// #52 seam: request status B — FIFO attribution and the end of a turn. A real
// Cordis Context hosts the built plugin, a real child speaks the resident
// protocol and the quality/IR tools, and the fake DSH appends the session
// events a real one appends (`session/event`): `turn/start`, the admitted
// `user/message` echoing its prompt `rpcId`, and `turn/end`.
//
// The spec's transition table and the attribution rules are checked here rather
// than in the browser: each row needs a whole simulated conversation there
// (qinyuanj/dsh-uml-viewer#41, Testing Decisions).
import { Context } from '@deepseek-ai/cordis'
import { spawn } from 'node:child_process'
import { mkdir, mkdtemp, rm, writeFile } from 'node:fs/promises'
import os from 'node:os'
import path from 'node:path'
import { fileURLToPath } from 'node:url'
import { afterEach, expect, it } from 'vitest'
import type { PanelEvent, RequestStatusEntry } from '@dsh-uml-viewer/contracts'

const builtHost = new URL('../../../lib/host.js', import.meta.url).href
const plugin = await import(builtHost) as typeof import('../src/index.js')
import type { Agent, HostServices, SpawnSpec, ToolDefinition, ToolRunContext } from '../src/dsh.js'

const goldenFrame = fileURLToPath(new URL('../../../crates/frame/tests/fixtures/frames/root.frame.json', import.meta.url))
const fakeBinary = [process.execPath, fileURLToPath(new URL('./fixtures/quality.mjs', import.meta.url))]

/** What the fake resident says each element of the shown diagram covers. */
const TARGETS: Record<string, Record<string, unknown>> = {
  'session-a': {
    ':core': { kind: 'component', files: ['src/core/order.ts'] },
    ':api': { kind: 'class', files: ['src/api.ts'] },
  },
  'session-b': { ':api': { kind: 'class', files: ['src/session-b/api.ts'] } },
}

/** The spec's reason for a turn that ended while its request was unfinished. */
const TURN_ENDED = 'the companion ended this turn without completing the display'

const roots: string[] = []
const contexts: Context[] = []
const controllers: AbortController[] = []
afterEach(async () => {
  for (const controller of controllers.splice(0)) controller.abort()
  for (const ctx of contexts.splice(0)) await ctx.fiber.dispose()
  for (const root of roots.splice(0)) await rm(root, { recursive: true, force: true })
})

function localSubprocess(spec: SpawnSpec) {
  const child = spawn(spec.argv[0]!, spec.argv.slice(1), { cwd: spec.cwd, stdio: ['pipe', 'pipe', 'pipe'] })
  const chunks = { stdout: [] as Buffer[], stderr: [] as Buffer[] }
  child.stdout.on('data', (chunk: Buffer) => chunks.stdout.push(chunk))
  child.stderr.on('data', (chunk: Buffer) => chunks.stderr.push(chunk))
  const reader = (key: 'stdout' | 'stderr') => ({ readFrom: () => ({ text: Buffer.concat(chunks[key]).toString(), lossy: false }) })
  return {
    stdin: child.stdin, stdout: child.stdout, stderr: child.stderr,
    collected: { stdout: reader('stdout'), stderr: reader('stderr') },
    done: new Promise<{ exitCode: number | null; signal: string | null }>((resolve, reject) => {
      child.on('error', reject)
      child.on('close', (exitCode, signal) => resolve({ exitCode, signal }))
    }),
    terminate: () => child.kill(),
    waitForExit: async () => true,
  }
}

interface PromptRequest { requestId: string; sessionId: string; mode: string; content: readonly { type: string; text: string }[] }

async function setup() {
  const projects: Record<string, string> = {}
  const agents: Record<string, Agent> = {}
  for (const id of ['session-a', 'session-b']) {
    const root = await mkdtemp(path.join(os.tmpdir(), `uml request status ${id} `))
    roots.push(root)
    projects[id] = root
    await writeFile(path.join(root, 'resident.json'), JSON.stringify({
      framePath: goldenFrame, openWaiting: false, qualityTargets: TARGETS[id] ?? {},
    }))
    await writeFile(path.join(root, 'fake-binary.json'), JSON.stringify({ exitCode: 0 }))
    await writeFile(path.join(root, 'quality-binary.json'), JSON.stringify({ exitCode: 0 }))
    await mkdir(path.join(root, 'examples'))
    await writeFile(path.join(root, 'examples', 'sample.policy.edn'), '{:out "diagram.edn"}')
    agents[id] = { id, session: { id }, ctx: { systemPrompt: { section: () => () => {}, getSectionOrder: () => 100 } } } as unknown as Agent
  }
  const home = await mkdtemp(path.join(os.tmpdir(), 'uml request status home '))
  roots.push(home)
  const admitted: PromptRequest[] = []
  const sessionController = {
    async prompt(request: PromptRequest) {
      admitted.push(request)
      return { accepted: true as const }
    },
  }
  const definitions = new Map<string, ToolDefinition>()
  const services: HostServices & Record<string, unknown> = {
    tools: { register(def) { definitions.set(def.name, def); return () => { definitions.delete(def.name) } } },
    sandboxPolicy: { resolve: ({ session }) => ({ mode: 'danger-full-access', workspaceRoot: projects[session.id]! }) },
    workingDirectory: { get: session => projects[session.id]!, ensure: async agent => projects[agent.session.id]! },
    subprocess: { spawn: spec => localSubprocess(spec) },
    sandbox: { confine: async argv => ({ argv: [...argv], enforcement: 'full' }) },
    typert: { register: () => async () => {} },
    dshHomePath: (...segments: string[]) => path.join(home, ...segments),
    sessionController,
  }
  const ctx = new Context()
  contexts.push(ctx)
  for (const [key, value] of Object.entries(services)) ctx.provide(key, value)
  await ctx.plugin(plugin, { binary: fakeBinary, home: path.join(home, 'dsh-uml-viewer') })
  const viewer = () => ctx.get('umlViewer') as unknown as {
    watch(agent: Agent, signal: AbortSignal): AsyncIterable<PanelEvent>
    action(agent: Agent, action: unknown): Promise<{ accepted: boolean; reason: string | null }>
  }
  const act = (session: string, action: Record<string, unknown>) => viewer().action(agents[session]!, action)
  const tool = (name: string, args: unknown, session = 'session-a') =>
    definitions.get(name)!.execute(args, { agent: agents[session], signal: new AbortController().signal } as ToolRunContext) as unknown as
      Promise<Record<string, unknown>>
  /** Open a panel stream and keep reading it, so every pushed list is seen. */
  const open = async (session: string) => {
    const controller = new AbortController()
    controllers.push(controller)
    const iterator = viewer().watch(agents[session]!, controller.signal)[Symbol.asyncIterator]()
    expect((await iterator.next()).value).toMatchObject({ kind: 'ready' })
    const events: PanelEvent[] = []
    void (async () => {
      for (;;) {
        const next = await iterator.next().catch(() => ({ done: true as const, value: undefined }))
        if (next.done) return
        events.push(next.value)
      }
    })()
    const lists = () => events.flatMap(event => event.kind === 'request-status' ? [event.requests] : [])
    return {
      events,
      lists,
      /** The latest list once `condition` holds for it. */
      async status(condition: (list: RequestStatusEntry[]) => boolean) {
        await until(() => { const last = lists().at(-1); return last !== undefined && condition(last) })
        return lists().at(-1)!
      },
    }
  }
  /** One session event, as DSH appends it: the fake DSH is the event's author. */
  const append = (session: string, event: Record<string, unknown>) =>
    (ctx as unknown as { emit(name: string, ...args: unknown[]): void }).emit('session/event', { id: session }, event)
  /** The durable user message an admitted prompt becomes: it echoes the prompt id. */
  const message = (session: string, prompt: PromptRequest | undefined) => append(session, {
    type: 'user/message', seq: 1, time: 0,
    data: { role: 'user', id: `m-${prompt?.requestId}`, content: prompt?.content ?? [],
      source: { kind: 'user', rpcId: prompt?.requestId } },
  })
  const turnStart = (session: string) => append(session, { type: 'turn/start', seq: 1, time: 0, data: { turn: 1 } })
  const turnEnd = (session: string) => append(session, { type: 'turn/end', seq: 2, time: 0, data: { turn: 1, reason: { kind: 'completed' } } })
  const write = (session: string, file: string, value: unknown) =>
    writeFile(path.join(projects[session]!, file), JSON.stringify(value))
  return { projects, agents, admitted, open, act, tool, append, message, turnStart, turnEnd, write, ctx }
}

async function until(condition: () => boolean, ms = 5000): Promise<void> {
  const deadline = Date.now() + ms
  while (!condition()) {
    if (Date.now() > deadline) throw new Error('timed out waiting for the expected state')
    await new Promise(resolve => setTimeout(resolve, 10))
  }
}

/** One request's entry as the panel sees it, in the order the table implies. */
const entry = (op: string, target: unknown, state: RequestStatusEntry['state'], extra: Partial<RequestStatusEntry> = {}) =>
  ({ id: expect.any(String), op, target, state, reason: null, exitCode: null, ...extra })

it('marks the head in progress on the companion’s first uml_* call, and keeps it there while the call succeeds', async () => {
  const s = await setup()
  const panel = await s.open('session-a')
  await s.act('session-a', { action: 'quality-request', request: 'refresh-crap', id: ':api' })
  expect(await panel.status(list => list.length === 1)).toEqual([entry('refresh-crap', { id: ':api', kind: 'class' }, 'queued')])
  expect(await s.tool('uml_crap', {})).toMatchObject({ validResult: true })
  expect(await panel.status(list => list[0]?.state === 'running')).toEqual([
    entry('refresh-crap', { id: ':api', kind: 'class' }, 'running')])
  // A later call of the same turn belongs to the same running request: no new state.
  const pushed = panel.lists().length
  expect(await s.tool('uml_ir', {})).toMatchObject({ validResult: true })
  await new Promise(resolve => setTimeout(resolve, 100))
  expect(panel.lists()).toHaveLength(pushed)
  expect(panel.lists().at(-1)).toEqual([entry('refresh-crap', { id: ':api', kind: 'class' }, 'running')])
})

it('fails the head with the quality record’s own reason and exit code when validResult is false', async () => {
  const s = await setup()
  const panel = await s.open('session-a')
  await s.write('session-a', 'quality-binary.json', { exitCode: 1, reason: 'no coverage data for the configured scope' })
  await s.act('session-a', { action: 'quality-request', request: 'refresh-mutate', id: ':core' })
  await panel.status(list => list.length === 1)
  expect(await s.tool('uml_mutate', { targets: ['src/core/order.ts'] })).toMatchObject({
    validResult: false, exitCode: 1, reason: 'no coverage data for the configured scope' })
  expect(await panel.status(list => list[0]?.state === 'failed')).toEqual([
    entry('refresh-mutate', { id: ':core', kind: 'component' }, 'failed',
      { reason: 'no coverage data for the configured scope', exitCode: 1 })])
})

it('fails the head when uml_ir fails, with the IR record’s reason and exit code', async () => {
  const s = await setup()
  const panel = await s.open('session-a')
  await s.act('session-a', { action: 'quality-request', request: 'refresh-crap', id: ':api' })
  await panel.status(list => list.length === 1)
  await s.write('session-a', 'fake-binary.json', { exitCode: 1, stderr: 'bad policy\n' })
  expect(await s.tool('uml_ir', {})).toMatchObject({ status: 'error', validResult: false, exitCode: 1 })
  expect(await panel.status(list => list[0]?.state === 'failed')).toEqual([
    entry('refresh-crap', { id: ':api', kind: 'class' }, 'failed',
      { reason: 'IR generation failed (exit 1)', exitCode: 1 })])
})

it('completes the head on a successful display, and the display names the element that request named', async () => {
  const s = await setup()
  const panel = await s.open('session-a')
  await s.act('session-a', { action: 'quality-request', request: 'refresh-crap', id: ':core' })
  await panel.status(list => list.length === 1)
  expect(await s.tool('uml_ir', {})).toMatchObject({ validResult: true })
  expect(await s.tool('uml_display', {})).toMatchObject({ status: 'ok', target: { id: ':core', kind: 'component' } })
  expect(await panel.status(list => list[0]?.state === 'done')).toEqual([
    entry('refresh-crap', { id: ':core', kind: 'component' }, 'done')])
  // The next display answers no request: the target belonged to the one completed.
  expect(await s.tool('uml_display', {})).toMatchObject({ status: 'ok', target: null })
})

it('attributes a granted uml_ir to the request its id names, exactly, not to the head', async () => {
  const s = await setup()
  const panel = await s.open('session-a')
  await s.act('session-a', { action: 'omit', id: ':api' })
  await s.act('session-a', { action: 'new-proposal', name: 'Next' })
  const [omit, proposal] = await panel.status(list => list.length === 2)
  expect([omit!.state, proposal!.state]).toEqual(['queued', 'queued'])
  // The companion answers the *second* request first: its id says so.
  const ir = s.tool('uml_ir', { request: proposal!.id })
  expect(await panel.status(list => list[1]?.state === 'running')).toEqual([
    omit!, { ...proposal!, state: 'running' }])
  expect(await ir).toMatchObject({ status: 'ok', validResult: true })
  // A failing IR that presents that id fails that request, not the head.
  expect(await s.tool('uml_ir', { request: proposal!.id })).toMatchObject({ validResult: false,
    reason: expect.stringMatching(/unknown or was already used/) })
  expect(await panel.status(list => list[1]?.state === 'failed')).toEqual([
    omit!, { ...proposal!, state: 'failed', reason: expect.stringMatching(/already used/), exitCode: null }])
})

it('fails the head when the companion’s turn ends without completing the display', async () => {
  const s = await setup()
  const panel = await s.open('session-a')
  await s.act('session-a', { action: 'quality-request', request: 'refresh-crap', id: ':api' })
  await panel.status(list => list.length === 1)
  // The companion's turn claims the admitted message, then ends: the display it
  // did not run is what the request still waited for.
  s.turnStart('session-a')
  s.message('session-a', s.admitted[0])
  s.turnEnd('session-a')
  expect(await panel.status(list => list[0]?.state === 'failed')).toEqual([
    entry('refresh-crap', { id: ':api', kind: 'class' }, 'failed', { reason: TURN_ENDED })])
})

it('fails the running head when a turn ends, with the turn-end reason', async () => {
  const s = await setup()
  const panel = await s.open('session-a')
  await s.act('session-a', { action: 'quality-request', request: 'refresh-crap', id: ':api' })
  await panel.status(list => list.length === 1)
  s.turnStart('session-a')
  s.message('session-a', s.admitted[0])
  expect(await s.tool('uml_crap', {})).toMatchObject({ validResult: true })
  await panel.status(list => list[0]?.state === 'running')
  s.turnEnd('session-a')
  expect(await panel.status(list => list[0]?.state === 'failed')).toEqual([
    entry('refresh-crap', { id: ':api', kind: 'class' }, 'failed', { reason: TURN_ENDED })])
})

it('leaves the head queued when a turn that never consumed it ends', async () => {
  const s = await setup()
  const panel = await s.open('session-a')
  await s.act('session-a', { action: 'quality-request', request: 'refresh-crap', id: ':api' })
  await panel.status(list => list.length === 1)
  // A turn of the user's own, with the request still waiting in DSH's queue:
  // the companion did not end a turn for this request.
  s.turnStart('session-a')
  s.message('session-a', { requestId: 'not-a-panel-request', sessionId: 'session-a', mode: 'queue', content: [] })
  s.turnEnd('session-a')
  await new Promise(resolve => setTimeout(resolve, 100))
  expect(panel.lists().at(-1)![0]!.state).toBe('queued')
  // The request's own turn then ends without a display: now it fails.
  s.turnStart('session-a')
  s.message('session-a', s.admitted[0])
  s.turnEnd('session-a')
  expect(await panel.status(list => list[0]?.state === 'failed')).toEqual([
    entry('refresh-crap', { id: ':api', kind: 'class' }, 'failed', { reason: TURN_ENDED })])
})

it('attributes a quality call the user runs by hand to the head of the queue, and moves on afterwards', async () => {
  const s = await setup()
  const panel = await s.open('session-a')
  await s.act('session-a', { action: 'quality-request', request: 'refresh-crap', id: ':core' })
  await s.act('session-a', { action: 'quality-request', request: 'refresh-mutate', id: ':core' })
  const [first, second] = await panel.status(list => list.length === 2)
  // Not the companion's call and not asked for by the panel: the head takes it.
  await s.write('session-a', 'quality-binary.json', { exitCode: 1, reason: 'the user asked for this run' })
  expect(await s.tool('uml_crap', {})).toMatchObject({ validResult: false })
  const failed = { ...first!, state: 'failed', reason: 'the user asked for this run', exitCode: 1 }
  expect(await panel.status(list => list[0]?.state === 'failed')).toEqual([failed, second!])
  // FIFO moves on: the next successful quality run, IR and display answer the
  // second request.
  await s.write('session-a', 'quality-binary.json', { exitCode: 0 })
  expect(await s.tool('uml_crap', {})).toMatchObject({ validResult: true })
  expect(await panel.status(list => list[1]?.state === 'running')).toEqual([failed, { ...second!, state: 'running' }])
  expect(await s.tool('uml_ir', {})).toMatchObject({ validResult: true })
  expect(await s.tool('uml_display', {})).toMatchObject({ status: 'ok' })
  expect(await panel.status(list => list[1]?.state === 'done')).toEqual([failed, { ...second!, state: 'done' }])
})

it('never attributes one session’s calls to another session’s requests', async () => {
  const s = await setup()
  const a = await s.open('session-a')
  const b = await s.open('session-b')
  await s.act('session-a', { action: 'quality-request', request: 'refresh-crap', id: ':api' })
  await a.status(list => list.length === 1)
  // Session B runs the same tools; A's request stays queued.
  expect(await s.tool('uml_crap', {}, 'session-b')).toMatchObject({ validResult: true })
  await s.turnEnd('session-b')
  await new Promise(resolve => setTimeout(resolve, 100))
  expect(a.lists().at(-1)![0]!.state).toBe('queued')
  expect(b.lists().at(-1) ?? []).toEqual([])
  // A's own call is what moves A's request.
  s.turnStart('session-a')
  s.message('session-a', s.admitted[0])
  expect(await s.tool('uml_crap', {})).toMatchObject({ validResult: true })
  expect(await a.status(list => list[0]?.state === 'running')).toEqual([
    entry('refresh-crap', { id: ':api', kind: 'class' }, 'running')])
})
