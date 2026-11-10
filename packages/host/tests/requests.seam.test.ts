// #28 seam: right-click quality requests become visible queued user messages
// in the panel's bound session, through DSH's real `sessionController.prompt`
// shape, in click order, with the companion rules present only while a panel
// stream is open. A real Cordis Context hosts the built plugin; a real child
// speaks the resident protocol.
//
// What the target covers is the resident's answer (`quality-target`), so these
// cases never parse an IR and no diagram file exists in the project: the
// files in the delivered message can only come from the resident.
import { Context } from '@deepseek-ai/cordis'
import { spawn } from 'node:child_process'
import { existsSync } from 'node:fs'
import { mkdtemp, readFile, rm, writeFile } from 'node:fs/promises'
import os from 'node:os'
import path from 'node:path'
import { fileURLToPath } from 'node:url'
import { afterEach, expect, it } from 'vitest'
import type { PanelEvent, RequestStatusEntry } from '@dsh-uml-viewer/contracts'
import { parsePanelEvent } from '@dsh-uml-viewer/contracts'

const builtHost = new URL('../../../lib/host.js', import.meta.url).href
const plugin = await import(builtHost) as typeof import('../src/index.js')
import type { Agent, HostServices, SpawnSpec } from '../src/dsh.js'

const goldenFrame = fileURLToPath(new URL('../../../crates/frame/tests/fixtures/frames/root.frame.json', import.meta.url))
const fakeBinary = [process.execPath, fileURLToPath(new URL('./fixtures/uml-viewer.mjs', import.meta.url))]

/** What the fake resident answers for each element id; the host may only use this. */
const TARGETS: Record<string, Record<string, unknown>> = {
  'session-a': {
    ':core': { kind: 'component', files: ['src/from-resident/a.ts', 'src/from-resident/b.ts'] },
    ':core.order': { kind: 'class', files: ['src/from-resident/order.ts'] },
    ':api': { kind: 'class', files: ['src/from-resident/api.ts'] },
    ':quil': { error: 'no class or namespace component :quil in the shown diagram' },
    ':nowhere': { error: 'no class or namespace component :nowhere in the shown diagram' },
    ':sourceless': { error: 'class :sourceless has no source file; it cannot be refreshed' },
    ':bad-kind': { kind: 'layer', files: ['src/x.ts'] },
    ':empty': { kind: 'class', files: [] },
    ':unsorted': { kind: 'component', files: ['src/b.ts', 'src/a.ts'] },
    ':duplicate': { kind: 'component', files: ['src/a.ts', 'src/a.ts'] },
    ':escape': { kind: 'class', files: ['../outside.ts'] },
    ':mismatch': { id: ':other', kind: 'class', files: ['src/a.ts'] },
    ':huge': { kind: 'component', files: Array.from({ length: 4097 }, (_, index) => `src/bulk/f${String(index).padStart(4, '0')}.ts`) },
  },
  'session-b': { ':api': { kind: 'class', files: ['src/session-b/api.ts'] } },
}

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
  const reader = { readFrom: () => ({ text: '', lossy: false }) }
  return {
    stdin: child.stdin, stdout: child.stdout, stderr: child.stderr,
    collected: { stdout: reader, stderr: reader },
    done: new Promise<{ exitCode: number | null; signal: string | null }>((resolve, reject) => {
      child.on('error', reject)
      child.on('close', (exitCode, signal) => resolve({ exitCode, signal }))
    }),
    terminate: () => child.kill(),
    waitForExit: async () => true,
  }
}

interface PromptRequest { requestId: string; sessionId: string; mode: string; content: readonly { type: string; text: string }[] }
interface Section { name: string; text: string | ((context: unknown) => string); interpolate?: boolean }

async function setup(options: { shown?: boolean; controller?: boolean } = {}) {
  const projects: Record<string, string> = {}
  for (const id of ['session-a', 'session-b']) {
    const root = await mkdtemp(path.join(os.tmpdir(), `uml requests ${id} `))
    roots.push(root)
    projects[id] = root
    // No diagram file: only the resident's answer can name source files.
    await writeFile(path.join(root, 'resident.json'), JSON.stringify({
      framePath: goldenFrame,
      ...(options.shown === false ? {} : { openWaiting: false }),
      qualityTargets: TARGETS[id] ?? {},
    }))
  }
  const home = await mkdtemp(path.join(os.tmpdir(), 'uml requests home '))
  roots.push(home)
  const sections: Record<string, Section[]> = { 'session-a': [], 'session-b': [] }
  const agents: Record<string, Agent> = {}
  for (const id of Object.keys(projects)) {
    const own = sections[id]!
    agents[id] = { id, session: { id }, ctx: { systemPrompt: {
      section(section: Section) { own.push(section); return () => { own.splice(own.indexOf(section), 1) } },
      getSectionOrder: () => 100,
    } } } as unknown as Agent
  }
  /** Every prompt DSH admitted, in admission order; `hold` delays one admission. */
  const admitted: PromptRequest[] = []
  const started: PromptRequest[] = []
  let hold: ((request: PromptRequest) => Promise<void>) | null = null
  let reject: ((request: PromptRequest) => Error | null) | null = null
  const sessionController = {
    async prompt(request: PromptRequest, signal: AbortSignal) {
      expect(signal).toBeInstanceOf(AbortSignal)
      started.push(request)
      if (hold) await hold(request)
      const failure = reject?.(request)
      if (failure) throw failure
      admitted.push(request)
      return { accepted: true as const }
    },
  }
  const services: HostServices & Record<string, unknown> = {
    tools: { register: () => () => {} },
    sandboxPolicy: { resolve: ({ session }) => ({ mode: 'danger-full-access', workspaceRoot: projects[session.id]! }) },
    workingDirectory: { get: session => projects[session.id]!, ensure: async agent => projects[agent.session.id]! },
    subprocess: { spawn: spec => localSubprocess(spec) },
    typert: { register: () => async () => {} },
    dshHomePath: (...segments: string[]) => path.join(home, ...segments),
    ...(options.controller === false ? {} : { sessionController }),
  }
  const ctx = new Context()
  contexts.push(ctx)
  for (const [key, value] of Object.entries(services)) ctx.provide(key, value)
  await ctx.plugin(plugin, { binary: fakeBinary, home: path.join(home, 'dsh-uml-viewer') })
  const viewer = () => ctx.get('umlViewer') as unknown as {
    watch(agent: Agent, signal: AbortSignal): AsyncIterable<PanelEvent>
    action(agent: Agent, action: unknown): Promise<{ accepted: boolean; reason: string | null }>
  }
  /** Open a panel stream and keep reading it; `statuses` is every request-status list it pushed. */
  const open = async (session: string) => {
    const controller = new AbortController()
    controllers.push(controller)
    const iterator = viewer().watch(agents[session]!, controller.signal)[Symbol.asyncIterator]()
    expect((await iterator.next()).value).toMatchObject({ kind: 'ready' })
    const events: PanelEvent[] = []
    const reading = (async () => {
      for (;;) {
        const next = await iterator.next().catch(() => ({ done: true as const, value: undefined }))
        if (next.done) return
        events.push(next.value)
      }
    })()
    const statuses = () => events.flatMap(event => event.kind === 'request-status' ? [event.requests] : [])
    return {
      events,
      statuses,
      /** The latest list once `condition` holds for it. */
      async status(condition: (list: RequestStatusEntry[]) => boolean) {
        await until(() => { const last = statuses().at(-1); return last !== undefined && condition(last) })
        return statuses().at(-1)!
      },
      close: async () => { controller.abort(); await reading },
    }
  }
  const request = (session: string, request: string, id: string, extra: Record<string, unknown> = {}) =>
    viewer().action(agents[session]!, { action: 'quality-request', request, id, ...extra })
  /** The actions the fake resident actually received, in order. */
  const actions = (session: string): Promise<{ action: string; id?: string }[]> =>
    readFile(path.join(projects[session]!, 'resident-actions.log'), 'utf8')
      .then(text => text.trimEnd().split('\n').filter(Boolean).map(line => {
        const entry = JSON.parse(line) as { action: string; id?: string }
        return { action: entry.action, ...(entry.id === undefined ? {} : { id: entry.id }) }
      }))
  return { projects, agents, sections, admitted, started, open, request, viewer, actions,
    hold(fn: typeof hold) { hold = fn }, reject(fn: typeof reject) { reject = fn } }
}

const header = 'UML viewer panel request:\n'

/** Wait for a condition instead of sleeping a fixed time. */
async function until(condition: () => boolean, ms = 5000): Promise<void> {
  const deadline = Date.now() + ms
  while (!condition()) {
    if (Date.now() > deadline) throw new Error('timed out waiting for the expected state')
    await new Promise(resolve => setTimeout(resolve, 10))
  }
}

it('queues one visible user message in the bound session for each refresh, naming the resident files', async () => {
  const s = await setup()
  await s.open('session-a')
  await s.open('session-b')
  for (const op of ['refresh-crap', 'refresh-mutate', 'refresh-mutate-all']) {
    expect(await s.request('session-a', op, ':core.order')).toEqual({ accepted: true, reason: null })
  }
  expect(s.admitted.map(r => [r.sessionId, r.mode, r.content])).toEqual(['refresh-crap', 'refresh-mutate', 'refresh-mutate-all']
    .map(op => ['session-a', 'queue', [{ type: 'text',
      text: `${header}{:op :${op} :target {:id :core.order :kind :class} :files ["src/from-resident/order.ts"]}` }]]))
  // Each message is its own admission with a fresh correlation id.
  expect(new Set(s.admitted.map(r => r.requestId)).size).toBe(3)
  expect(s.admitted.every(r => typeof r.requestId === 'string' && r.requestId.length > 0)).toBe(true)
  // Session B's panel is open too, but never receives A's request.
  expect(s.admitted.some(r => r.sessionId === 'session-b')).toBe(false)
  expect(await s.request('session-b', 'refresh-crap', ':api')).toEqual({ accepted: true, reason: null })
  expect(s.admitted.at(-1)).toMatchObject({ sessionId: 'session-b',
    content: [{ type: 'text', text: `${header}{:op :refresh-crap :target {:id :api :kind :class} :files ["src/session-b/api.ts"]}` }] })
})

it('uses only the resident answer: no IR file exists and a client-named file is ignored', async () => {
  const s = await setup()
  await s.open('session-a')
  expect(existsSync(path.join(s.projects['session-a']!, 'diagram.edn'))).toBe(false)
  // The panel sends an id and a file of its own; only the id is a field.
  expect(await s.request('session-a', 'refresh-mutate', ':core', { files: ['../outside.ts'] }))
    .toEqual({ accepted: true, reason: null })
  expect(s.admitted.map(r => r.content[0]!.text)).toEqual([
    `${header}{:op :refresh-mutate :target {:id :core :kind :component} :files ["src/from-resident/a.ts" "src/from-resident/b.ts"]}`])
  // What the host asked: the resident action for that element, never a `save`
  // read of the IR path and never a client file.
  const actions = await s.actions('session-a')
  expect(actions.filter(entry => entry.action === 'quality-target')).toEqual([{ action: 'quality-target', id: ':core' }])
  expect(actions.some(entry => entry.action === 'save')).toBe(false)
  expect(JSON.stringify(actions)).not.toContain('outside.ts')
})

it('refuses with the resident reason when the element has no target and still delivers later requests', async () => {
  const s = await setup()
  await s.open('session-a')
  for (const [id, reason] of [
    [':nowhere', 'no class or namespace component :nowhere in the shown diagram'],
    [':quil', 'no class or namespace component :quil in the shown diagram'],
    [':sourceless', 'class :sourceless has no source file; it cannot be refreshed'],
  ] as const) {
    expect(await s.request('session-a', 'refresh-mutate', id)).toEqual({ accepted: false, reason })
  }
  expect(s.admitted).toEqual([])
  expect(await s.request('session-a', 'refresh-crap', ':api')).toEqual({ accepted: true, reason: null })
  expect(s.admitted).toHaveLength(1)
})

it('refuses a malformed, reordered, oversized or mismatched resident answer instead of queuing it', async () => {
  const s = await setup()
  await s.open('session-a')
  const cases: [string, RegExp][] = [
    [':bad-kind', /kind must be class or component/],
    [':empty', /files must be a nonempty array/],
    [':unsorted', /ascending path order/],
    [':duplicate', /duplicate/],
    [':escape', /normalized relative path/],
    [':mismatch', /does not match the requested :mismatch/],
    [':huge', /the limit is 4096/],
  ]
  for (const [id, reason] of cases) {
    expect(await s.request('session-a', 'refresh-crap', id), id).toEqual({ accepted: false, reason: expect.stringMatching(reason) })
  }
  expect(s.admitted).toEqual([])
})

it('admits overlapping requests strictly in click order, one at a time', async () => {
  const s = await setup()
  await s.open('session-a')
  let release!: () => void
  const first = new Promise<void>(resolve => { release = resolve })
  s.hold(async request => { if (request.content[0]!.text.includes(':refresh-crap')) await first })
  const answers = [
    s.request('session-a', 'refresh-crap', ':core.order'),
    s.request('session-a', 'refresh-mutate', ':core'),
    s.request('session-a', 'refresh-mutate-all', ':api'),
  ]
  // The first admission is still pending: nothing behind it may overtake it.
  await until(() => s.started.length >= 1)
  await new Promise(resolve => setTimeout(resolve, 200))
  expect(s.started.map(r => r.content[0]!.text.split('\n')[1]!.split(' ')[1])).toEqual([':refresh-crap'])
  release()
  expect(await Promise.all(answers)).toEqual([0, 1, 2].map(() => ({ accepted: true, reason: null })))
  expect(s.admitted.map(r => r.content[0]!.text.split('\n')[1]!.split(' :target')[0]))
    .toEqual(['{:op :refresh-crap', '{:op :refresh-mutate', '{:op :refresh-mutate-all'])
})

it('reports a refused admission and still delivers the requests queued behind it', async () => {
  const s = await setup()
  await s.open('session-a')
  s.reject(request => request.content[0]!.text.includes(':refresh-crap')
    ? Object.assign(new Error('session is archived'), { code: 'session/not-found' }) : null)
  const answers = await Promise.all([
    s.request('session-a', 'refresh-crap', ':api'),
    s.request('session-a', 'refresh-mutate', ':api'),
  ])
  expect(answers[0]).toEqual({ accepted: false, reason: expect.stringMatching(/session is archived/) })
  expect(answers[1]).toEqual({ accepted: true, reason: null })
  expect(s.admitted.map(r => r.content[0]!.text)).toEqual([
    `${header}{:op :refresh-mutate :target {:id :api :kind :class} :files ["src/from-resident/api.ts"]}`])
})

it('refuses, without delivering, a panel whose resident shows no diagram yet', async () => {
  const s = await setup({ shown: false })
  await s.open('session-a')
  expect(await s.request('session-a', 'refresh-crap', ':api'))
    .toEqual({ accepted: false, reason: expect.stringMatching(/no diagram/) })
  expect(s.admitted).toEqual([])
})

it('refuses with a reason when this DSH offers no session prompt service', async () => {
  const s = await setup({ controller: false })
  await s.open('session-a')
  expect(await s.request('session-a', 'refresh-crap', ':api'))
    .toEqual({ accepted: false, reason: expect.stringMatching(/sessionController/) })
})

it('only queues: it never steers the running turn, so the composer and its draft are not involved', async () => {
  const s = await setup()
  await s.open('session-a')
  await s.request('session-a', 'refresh-crap', ':api')
  await s.request('session-a', 'refresh-mutate-all', ':core')
  // `queue` waits behind the running turn as a new user message; `steer` would
  // inject into it. No other part of the prompt request is sent.
  expect(s.admitted.map(r => r.mode)).toEqual(['queue', 'queue'])
  for (const r of s.admitted) expect(Object.keys(r).sort()).toEqual(['content', 'mode', 'requestId', 'sessionId'])
})

it('carries the companion rules only while that session has an open panel', async () => {
  const s = await setup()
  const rules = (await readFile(new URL('../../../docs/companion-rules.md', import.meta.url), 'utf8')).trimEnd()
  expect(s.sections['session-a']).toEqual([])
  const panel = await s.open('session-a')
  expect(s.sections['session-a']).toHaveLength(1)
  const text = s.sections['session-a']![0]!.text as string
  expect(text).toBe(rules)
  // The rules name the delivered request format and the one-call mutation rule.
  for (const phrase of [header.trim(), ':refresh-crap', ':refresh-mutate-all', ':files', 'uml_mutate', 'mutateAll', 'uml_ir', 'uml_display', 'validResult']) {
    expect(text).toContain(phrase)
  }
  expect(s.sections['session-b']).toEqual([])
  await panel.close()
  await until(() => s.sections['session-a']!.length === 0)
  expect(s.sections['session-a']).toEqual([])
})

// #51 request status A: the host keeps each session's request-status list in
// memory and pushes the whole list on the panel stream, when the stream opens
// and on every change.
const queued = (op: string, target: unknown, id: unknown = expect.any(String)) =>
  ({ id, op, target, state: 'queued', reason: null, exitCode: null })

it('pushes the session’s list when the stream opens, and an admitted refresh is queued at once', async () => {
  const s = await setup()
  const panel = await s.open('session-a')
  // The list arrives on subscribe, before any request, and is a valid event.
  expect(await panel.status(list => list.length === 0)).toEqual([])
  expect(await s.request('session-a', 'refresh-crap', ':core.order')).toEqual({ accepted: true, reason: null })
  const [entry] = await panel.status(list => list.length === 1)
  // Its id is the admitted message's correlation id.
  expect(entry).toEqual(queued('refresh-crap', { id: ':core.order', kind: 'class' }, s.admitted[0]!.requestId))
  await s.request('session-a', 'refresh-mutate', ':core')
  expect(await panel.status(list => list.length === 2)).toEqual([
    entry, queued('refresh-mutate', { id: ':core', kind: 'component' }, s.admitted[1]!.requestId)])
  for (const event of panel.events) expect(parsePanelEvent(event)).toEqual(event)
})

it('marks a request DSH did not admit as failed with the admission reason, and never queues it', async () => {
  const s = await setup()
  const panel = await s.open('session-a')
  // Refused before the message: the resident names no target for the element.
  expect(await s.request('session-a', 'refresh-mutate', ':nowhere')).toMatchObject({ accepted: false })
  const [unresolved] = await panel.status(list => list.length === 1)
  expect(unresolved).toEqual({ id: expect.any(String), op: 'refresh-mutate', target: null, state: 'failed',
    reason: 'no class or namespace component :nowhere in the shown diagram', exitCode: null })
  // Refused by DSH itself: the element was resolved, the session refused the message.
  s.reject(() => new Error('session is archived'))
  expect(await s.request('session-a', 'refresh-crap', ':api')).toMatchObject({ accepted: false })
  const list = await panel.status(list => list.length === 2)
  expect(list[1]).toEqual({ id: expect.any(String), op: 'refresh-crap', target: { id: ':api', kind: 'class' },
    state: 'failed', reason: expect.stringMatching(/session is archived/), exitCode: null })
  expect(list[0]!.id).not.toBe(list[1]!.id)
  expect(s.admitted).toEqual([])
  // Nothing was ever queued.
  expect(panel.statuses().flat().some(entry => entry.state === 'queued')).toBe(false)
})

it('fails a request with a reason when this DSH offers no session prompt service', async () => {
  const s = await setup({ controller: false })
  const panel = await s.open('session-a')
  await s.request('session-a', 'refresh-crap', ':api')
  expect(await panel.status(list => list.length === 1)).toEqual([expect.objectContaining({
    op: 'refresh-crap', state: 'failed', reason: expect.stringMatching(/sessionController/) })])
})

it('keeps a session’s entries for a stream opened later, as after a page refresh', async () => {
  const s = await setup()
  const first = await s.open('session-a')
  await s.request('session-a', 'refresh-crap', ':api')
  await s.request('session-a', 'refresh-mutate', ':nowhere')
  const before = await first.status(list => list.length === 2)
  await first.close()
  // The page refreshed: a new stream of the same session gets the list on subscribe.
  const second = await s.open('session-a')
  expect(await second.status(list => list.length === 2)).toEqual(before)
  expect(before.map(entry => entry.state)).toEqual(['queued', 'failed'])
})

it('never shows one session’s entries on another session’s panel', async () => {
  const s = await setup()
  const a = await s.open('session-a')
  const b = await s.open('session-b')
  await b.status(list => list.length === 0)
  await s.request('session-a', 'refresh-crap', ':api')
  await s.request('session-a', 'refresh-crap', ':nowhere')
  await a.status(list => list.length === 2)
  await s.request('session-b', 'refresh-crap', ':api')
  expect(await b.status(list => list.length === 1)).toEqual([
    queued('refresh-crap', { id: ':api', kind: 'class' }, s.admitted.at(-1)!.requestId)])
  // B saw only its own list; A's list never grew B's entry.
  expect(b.statuses().every(list => list.length <= 1)).toBe(true)
  await new Promise(resolve => setTimeout(resolve, 100))
  expect(a.statuses().at(-1)).toHaveLength(2)
  // A late B stream gets B's list alone.
  const late = await s.open('session-b')
  expect(await late.status(list => list.length === 1)).toEqual(b.statuses().at(-1))
})

it('keeps every resident event while request-status updates interleave with them', async () => {
  const s = await setup()
  const panel = await s.open('session-a')
  await until(() => panel.events.some(event => event.kind === 'frame'))
  // A burst of requests and resident answers at once: nothing of either is lost.
  await Promise.all([
    ...[':api', ':core', ':core.order'].map(id => s.request('session-a', 'refresh-crap', id)),
    ...['a', 'b', 'c'].map(query => s.viewer().action(s.agents['session-a']!, { action: 'search', query })),
  ])
  await panel.status(list => list.length === 3)
  await until(() => panel.events.filter(event => event.kind === 'search').length === 3)
  expect(panel.events.flatMap(event => event.kind === 'search' ? [event.query] : [])).toEqual(['a', 'b', 'c'])
})
