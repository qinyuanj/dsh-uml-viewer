// #29 seam: hiding an element, choosing the discussion context and adding a
// proposal become visible queued user messages in the panel's bound session,
// through DSH's `sessionController.prompt` shape, in click order. The built
// plugin runs in a real Cordis Context; a real child speaks the resident
// protocol (its frame is the producer golden) and serves IR and quality too.
import { Context } from '@deepseek-ai/cordis'
import { spawn } from 'node:child_process'
import { existsSync } from 'node:fs'
import { mkdir, mkdtemp, readFile, rm, writeFile } from 'node:fs/promises'
import os from 'node:os'
import path from 'node:path'
import { fileURLToPath } from 'node:url'
import { afterEach, expect, it } from 'vitest'
import type { PanelEvent } from '@dsh-uml-viewer/contracts'

const builtHost = new URL('../../../lib/host.js', import.meta.url).href
const plugin = await import(builtHost) as typeof import('../src/index.js')
import type { Agent, HostServices, SpawnSpec, ToolDefinition, ToolRunContext } from '../src/dsh.js'

const goldenFrame = fileURLToPath(new URL('../../../crates/frame/tests/fixtures/frames/root.frame.json', import.meta.url))
const fakeBinary = [process.execPath, fileURLToPath(new URL('./fixtures/quality.mjs', import.meta.url))]
const header = 'UML viewer panel request:\n'

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

async function setup(options: { resident?: Record<string, unknown>; controller?: boolean } = {}) {
  const projects: Record<string, string> = {}
  const agents: Record<string, Agent> = {}
  for (const id of ['session-a', 'session-b']) {
    const root = await mkdtemp(path.join(os.tmpdir(), `uml view requests ${id} `))
    roots.push(root)
    projects[id] = root
    await writeFile(path.join(root, 'resident.json'), JSON.stringify({ framePath: goldenFrame, openWaiting: false, ...options.resident }))
    await writeFile(path.join(root, 'fake-binary.json'), JSON.stringify({ exitCode: 0 }))
    await mkdir(path.join(root, 'examples'))
    await writeFile(path.join(root, 'examples', 'sample.policy.edn'), '{:out "diagram.edn"}')
    agents[id] = { id, session: { id }, ctx: { systemPrompt: { section: () => () => {}, getSectionOrder: () => 100 } } } as unknown as Agent
  }
  const home = await mkdtemp(path.join(os.tmpdir(), 'uml view requests home '))
  roots.push(home)
  const admitted: PromptRequest[] = []
  const started: PromptRequest[] = []
  let hold: ((request: PromptRequest) => Promise<void>) | null = null
  let reject: ((request: PromptRequest) => Error | null) | null = null
  const sessionController = {
    async prompt(request: PromptRequest) {
      started.push(request)
      if (hold) await hold(request)
      const failure = reject?.(request)
      if (failure) throw failure
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
  /** Open a panel stream and keep reading it, so the test sees every event. */
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
    return { events }
  }
  const act = (session: string, action: Record<string, unknown>) => viewer().action(agents[session]!, action)
  const tool = (name: string, args: unknown, session = 'session-a') =>
    definitions.get(name)!.execute(args, { agent: agents[session], signal: new AbortController().signal } as ToolRunContext) as unknown as
      Promise<Record<string, unknown>>
  /** The actions the fake resident received, in order. */
  const actions = async (session: string): Promise<{ action: string }[]> => {
    const file = path.join(projects[session]!, 'resident-actions.log')
    if (!existsSync(file)) return []
    return (await readFile(file, 'utf8')).trimEnd().split('\n').filter(Boolean).map(line => JSON.parse(line) as { action: string })
  }
  const texts = () => admitted.map(r => r.content[0]!.text)
  return { projects, agents, admitted, started, open, act, tool, actions, texts,
    hold(fn: typeof hold) { hold = fn }, reject(fn: typeof reject) { reject = fn } }
}

/** The request id a message carries, so a later tool call can present it. */
function requestIdOf(text: string): string {
  const match = /:request "([0-9a-f]+)"\}$/.exec(text)
  expect(match, text).not.toBeNull()
  return match![1]!
}

async function until(condition: () => boolean, ms = 5000): Promise<void> {
  const deadline = Date.now() + ms
  while (!condition()) {
    if (Date.now() > deadline) throw new Error('timed out waiting for the expected state')
    await new Promise(resolve => setTimeout(resolve, 10))
  }
}

it('hides a class or a component of the shown diagram with one queued message naming the source diagram', async () => {
  const s = await setup()
  const panel = await s.open('session-a')
  await until(() => panel.events.some(event => event.kind === 'frame'))
  const framesBefore = panel.events.filter(event => event.kind === 'frame').length
  const actionsBefore = (await s.actions('session-a')).length
  expect(await s.act('session-a', { action: 'omit', id: ':api' })).toEqual({ accepted: true, reason: null })
  expect(await s.act('session-a', { action: 'omit', id: ':view' })).toEqual({ accepted: true, reason: null })
  expect(s.admitted.map(r => [r.sessionId, r.mode])).toEqual([['session-a', 'queue'], ['session-a', 'queue']])
  const [api, view] = s.texts()
  expect(api).toMatch(new RegExp(`^${header}\\{:op :omit :target \\{:id :api :kind :class\\} :context \\{:context :real\\} :request "[0-9a-f]{32}"\\}$`))
  expect(view).toMatch(/^UML viewer panel request:\n\{:op :omit :target \{:id :view :kind :component\} :context \{:context :real\} :request "[0-9a-f]{32}"\}$/)
  expect(requestIdOf(api!)).not.toBe(requestIdOf(view!))
  // Hiding changes nothing on screen by itself: the panel refreshes only when
  // the companion's uml_ir succeeded and uml_display showed the new IR.
  await new Promise(resolve => setTimeout(resolve, 100))
  expect(panel.events.filter(event => event.kind === 'frame')).toHaveLength(framesBefore)
  // Nor is the resident asked to change anything: the companion edits the policy.
  expect((await s.actions('session-a')).slice(actionsBefore)).toEqual([])
})

it('refuses to hide what is not a policy element of the shown diagram, and sends nothing', async () => {
  const s = await setup()
  await s.open('session-a')
  expect(await s.act('session-a', { action: 'omit', id: ':nowhere' }))
    .toEqual({ accepted: false, reason: 'no class or namespace component :nowhere in the shown diagram' })
  expect(await s.act('session-a', { action: 'omit', id: ':serde' }))
    .toEqual({ accepted: false, reason: expect.stringMatching(/foreign library/) })
  expect(await s.act('session-a', { action: 'omit', id: 'serde' }))
    .toEqual({ accepted: false, reason: expect.stringMatching(/keyword/) })
  expect(s.admitted).toEqual([])
})

it('refuses to hide anything while the panel still waits for its first diagram', async () => {
  const s = await setup({ resident: { openWaiting: true } })
  await s.open('session-a')
  expect(await s.act('session-a', { action: 'omit', id: ':api' }))
    .toEqual({ accepted: false, reason: expect.stringMatching(/no diagram/) })
  expect(s.admitted).toEqual([])
})

it('tells the companion the chosen discussion context, after the resident switched to it', async () => {
  const s = await setup()
  const panel = await s.open('session-a')
  expect(await s.act('session-a', { action: 'context', proposalId: ':p-shop' })).toEqual({ accepted: true, reason: null })
  expect(s.texts()).toEqual([`${header}{:op :context :context {:context :proposal :proposal-id :p-shop}}`])
  // The view switched as before: the resident got the action, the panel the new context.
  expect((await s.actions('session-a')).filter(entry => entry.action === 'context')).toEqual([
    expect.objectContaining({ action: 'context', proposalId: ':p-shop' })])
  await until(() => panel.events.some(event => event.kind === 'context' && event.context.kind === 'proposal'))
  // Later requests are about that diagram until another context arrives.
  expect(await s.act('session-a', { action: 'omit', id: ':api' })).toEqual({ accepted: true, reason: null })
  expect(s.texts()[1]).toMatch(/^UML viewer panel request:\n\{:op :omit :target \{:id :api :kind :class\} :context \{:context :proposal :proposal-id :p-shop\} :request "[0-9a-f]{32}"\}$/)
  expect(await s.act('session-a', { action: 'context', proposalId: null })).toEqual({ accepted: true, reason: null })
  expect(await s.act('session-a', { action: 'omit', id: ':core' })).toEqual({ accepted: true, reason: null })
  expect(s.texts().slice(2)).toEqual([
    `${header}{:op :context :context {:context :real}}`,
    expect.stringMatching(/^UML viewer panel request:\n\{:op :omit :target \{:id :core :kind :class\} :context \{:context :real\} :request "[0-9a-f]{32}"\}$/),
  ])
})

it('sends no context message when the resident refuses the switch', async () => {
  const s = await setup({ resident: { unknownProposals: [':gone'] } })
  await s.open('session-a')
  expect(await s.act('session-a', { action: 'context', proposalId: ':gone' })).toEqual({ accepted: false, reason: 'unknown proposal :gone' })
  expect(s.admitted).toEqual([])
})

it('adds a proposal named with the local time to the second unless the user named it', async () => {
  const s = await setup()
  await s.open('session-a')
  const before = Date.now()
  expect(await s.act('session-a', { action: 'new-proposal' })).toEqual({ accepted: true, reason: null })
  const after = Date.now()
  expect(await s.act('session-a', { action: 'new-proposal', name: '  Split "core"  ' })).toEqual({ accepted: true, reason: null })
  const [timestamped, named] = s.texts()
  const match = /^UML viewer panel request:\n\{:op :new-proposal :name "(\d{4})-(\d{2})-(\d{2}) (\d{2}):(\d{2}):(\d{2})" :request "[0-9a-f]{32}"\}$/.exec(timestamped!)
  expect(match, timestamped).not.toBeNull()
  const [, y, mo, d, h, mi, sec] = match!.map(Number)
  const stamp = new Date(y!, mo! - 1, d!, h!, mi!, sec!).getTime()
  expect(stamp).toBeGreaterThanOrEqual(Math.floor(before / 1000) * 1000)
  expect(stamp).toBeLessThanOrEqual(after)
  expect(named).toMatch(/^UML viewer panel request:\n\{:op :new-proposal :name "Split \\"core\\"" :request "[0-9a-f]{32}"\}$/)
  expect(await s.act('session-a', { action: 'new-proposal', name: 'a\nb' }))
    .toEqual({ accepted: false, reason: expect.stringMatching(/one line/) })
  expect(s.admitted).toHaveLength(2)
})

it('admits hide, context and new-proposal requests strictly in click order, one at a time', async () => {
  const s = await setup()
  await s.open('session-a')
  let release!: () => void
  const first = new Promise<void>(resolve => { release = resolve })
  s.hold(async request => { if (request.content[0]!.text.includes(':op :omit')) await first })
  const answers = [
    s.act('session-a', { action: 'omit', id: ':api' }),
    s.act('session-a', { action: 'context', proposalId: ':p-shop' }),
    s.act('session-a', { action: 'new-proposal', name: 'Next' }),
    s.act('session-a', { action: 'quality-request', request: 'refresh-crap', id: ':api' }),
  ]
  await until(() => s.started.length >= 1)
  await new Promise(resolve => setTimeout(resolve, 200))
  expect(s.started).toHaveLength(1)
  // The switch behind the held hide has not reached the resident either, so
  // the hide still names the source diagram.
  expect((await s.actions('session-a')).some(entry => entry.action === 'context')).toBe(false)
  release()
  const acks = await Promise.all(answers)
  expect(acks.slice(0, 3)).toEqual([0, 1, 2].map(() => ({ accepted: true, reason: null })))
  expect(s.texts().slice(0, 3).map(text => text.split('\n')[1]!.split(' ').slice(0, 2).join(' '))).toEqual([
    '{:op :omit', '{:op :context', '{:op :new-proposal'])
  expect(s.texts()[0]).toContain(':context {:context :real}')
  expect(s.admitted.every(r => r.mode === 'queue' && r.sessionId === 'session-a')).toBe(true)
  for (const r of s.admitted) expect(Object.keys(r).sort()).toEqual(['content', 'mode', 'requestId', 'sessionId'])
})

it('runs uml_ir for a granted request after a failed quality call; display still waits for that IR', async () => {
  const s = await setup()
  const root = s.projects['session-a']!
  const panel = await s.open('session-a')
  await writeFile(path.join(root, 'quality-binary.json'), JSON.stringify({ exitCode: 1 }))
  expect(await s.tool('uml_crap', {})).toMatchObject({ validResult: false })
  // Without the id nothing changed: the failed quality call blocks IR.
  expect(await s.tool('uml_ir', {})).toMatchObject({ status: 'refused', reason: expect.stringMatching(/latest quality call/) })
  expect(await s.act('session-a', { action: 'omit', id: ':api' })).toEqual({ accepted: true, reason: null })
  const request = requestIdOf(s.texts()[0]!)
  // A failing IR for the request: it consumed the id, display is refused, the panel is unchanged.
  await writeFile(path.join(root, 'fake-binary.json'), JSON.stringify({ exitCode: 1, stderr: 'policy error\n' }))
  expect(await s.tool('uml_ir', { request })).toMatchObject({ status: 'error', validResult: false, exitCode: 1 })
  expect(JSON.parse(await readFile(path.join(root, 'invocation.json'), 'utf8')).args.join(' ')).not.toContain(request)
  const frames = panel.events.filter(event => event.kind === 'frame').length
  expect(await s.tool('uml_display', {})).toMatchObject({ status: 'refused', validResult: false })
  await new Promise(resolve => setTimeout(resolve, 100))
  expect(panel.events.filter(event => event.kind === 'frame')).toHaveLength(frames)
  // The id is single-use, success or failure.
  await writeFile(path.join(root, 'fake-binary.json'), JSON.stringify({ exitCode: 0 }))
  expect(await s.tool('uml_ir', { request })).toMatchObject({ status: 'refused', reason: expect.stringMatching(/unknown or was already used/) })
  expect(await s.tool('uml_display', {})).toMatchObject({ status: 'refused' })
  // A fresh request id: IR succeeds, then display refreshes the panel.
  expect(await s.act('session-a', { action: 'new-proposal', name: 'Next' })).toEqual({ accepted: true, reason: null })
  expect(await s.tool('uml_ir', { request: requestIdOf(s.texts()[1]!) })).toMatchObject({ status: 'ok', validResult: true })
  expect(await s.tool('uml_display', {})).toMatchObject({ status: 'ok', validResult: true })
  await until(() => panel.events.filter(event => event.kind === 'frame').length > frames)
})

it('refuses a request id from another session, an unknown one, and context messages carry none', async () => {
  const s = await setup()
  await s.open('session-a')
  await s.open('session-b')
  expect(await s.act('session-a', { action: 'omit', id: ':api' })).toEqual({ accepted: true, reason: null })
  const request = requestIdOf(s.texts()[0]!)
  expect(await s.tool('uml_ir', { request }, 'session-b')).toMatchObject({ status: 'refused', validResult: false,
    reason: expect.stringMatching(/was not sent to this session/) })
  expect(existsSync(path.join(s.projects['session-b']!, 'invocation.json'))).toBe(false)
  expect(await s.tool('uml_ir', { request: 'f'.repeat(32) })).toMatchObject({ status: 'refused',
    reason: expect.stringMatching(/unknown or was already used/) })
  expect(await s.tool('uml_ir', { request: 'nope' })).toMatchObject({ validResult: false, reason: expect.stringMatching(/request id/) })
  // The foreign presentation did not burn session A's id.
  expect(await s.tool('uml_ir', { request })).toMatchObject({ status: 'ok', validResult: true })
  expect(await s.act('session-a', { action: 'context', proposalId: null })).toEqual({ accepted: true, reason: null })
  expect(s.texts()[1]).not.toContain(':request')
})

it('keeps a granted uml_ir behind an in-flight quality call', async () => {
  const s = await setup()
  const root = s.projects['session-a']!
  await s.open('session-a')
  expect(await s.act('session-a', { action: 'omit', id: ':api' })).toEqual({ accepted: true, reason: null })
  const request = requestIdOf(s.texts()[0]!)
  await writeFile(path.join(root, 'quality-binary.json'), JSON.stringify({ exitCode: 1, delay: 300 }))
  const crap = s.tool('uml_crap', {})
  await until(() => existsSync(path.join(root, 'quality-events.jsonl')))
  const ir = s.tool('uml_ir', { request })
  await new Promise(resolve => setTimeout(resolve, 100))
  expect(existsSync(path.join(root, 'invocation.json'))).toBe(false)
  expect(await crap).toMatchObject({ validResult: false })
  // It ran after the failed call ended, and the grant let it run anyway.
  expect(await ir).toMatchObject({ status: 'ok', validResult: true })
  expect((await readFile(path.join(root, 'quality-events.jsonl'), 'utf8')).trim().split('\n').map(line => JSON.parse(line).event))
    .toEqual(['start', 'end'])
})

it('withdraws the id of a request DSH did not admit', async () => {
  const s = await setup()
  await s.open('session-a')
  let seen = ''
  s.reject(request => { seen = request.content[0]!.text; return new Error('session is archived') })
  expect(await s.act('session-a', { action: 'omit', id: ':api' })).toEqual({ accepted: false, reason: 'session is archived' })
  expect(await s.tool('uml_ir', { request: requestIdOf(seen) })).toMatchObject({ status: 'refused',
    reason: expect.stringMatching(/unknown or was already used/) })
})

it('carries the #29 request formats and the request-id rule in the embedded companion rules', async () => {
  const rules = await readFile(new URL('../../../docs/companion-rules.md', import.meta.url), 'utf8')
  expect(rules).not.toContain('\r')
  for (const phrase of ['{:op :omit :target', '{:op :new-proposal :name', '{:op :context :context', 'uml_ir({request: "<id>"})',
    'Each id works once', 'in place', 'keep its comments']) {
    expect(rules).toContain(phrase)
  }
})
