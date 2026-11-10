import { Context } from '@deepseek-ai/cordis'
import { spawn } from 'node:child_process'
import { existsSync } from 'node:fs'
import { mkdir, mkdtemp, readFile, realpath, rm, writeFile } from 'node:fs/promises'
import os from 'node:os'
import path from 'node:path'
import { fileURLToPath } from 'node:url'
import { afterEach, expect, it } from 'vitest'
import { parsePanelEvent, UML_VIEWER_REMOTE } from '@dsh-uml-viewer/contracts'
import type { PanelEvent } from '@dsh-uml-viewer/contracts'

const builtHost = new URL('../../../lib/host.js', import.meta.url).href
const plugin = await import(builtHost) as typeof import('../src/index.js')
import type { Agent, HostServices, SpawnSpec, ToolDefinition, ToolRunContext } from '../src/dsh.js'

const goldenFrame = fileURLToPath(new URL('../../../crates/frame/tests/fixtures/frames/root.frame.json', import.meta.url))
const fakeBinary = [process.execPath, fileURLToPath(new URL('./fixtures/uml-viewer.mjs', import.meta.url))]

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
  const reader = (key: 'stdout' | 'stderr') => ({
    readFrom: () => {
      const data = Buffer.concat(chunks[key])
      return { text: data.toString(), lossy: false }
    },
  })
  return {
    stdin: child.stdin,
    stdout: child.stdout,
    stderr: child.stderr,
    collected: { stdout: reader('stdout'), stderr: reader('stderr') },
    done: new Promise<{ exitCode: number | null; signal: string | null }>((resolve, reject) => {
      child.on('error', reject)
      child.on('close', (exitCode, signal) => resolve({ exitCode, signal }))
    }),
    terminate: () => child.kill(),
    waitForExit: async () => true,
  }
}

interface PromptSection { name: string; order: number; text: string | ((context: unknown) => string) }
interface FakeSystemPrompt {
  sections: PromptSection[]
  section(section: PromptSection): () => void
  getSectionOrder(name: string): number
}

function fakeSystemPrompt(): FakeSystemPrompt {
  const sections: PromptSection[] = []
  return {
    sections,
    section(section) {
      sections.push(section)
      return () => {
        const index = sections.indexOf(section)
        if (index >= 0) sections.splice(index, 1)
      }
    },
    getSectionOrder: () => 100,
  }
}

type Route = { path: string; kind?: string; resource?: (request: Request) => Response | Promise<Response> }

async function setup(options: {
  mode?: 'read-only' | 'workspace-write' | 'danger-full-access'
  resident?: Record<string, unknown>
  displayError?: string
  /** Provide the optional services only after the plugin is loaded. */
  late?: boolean
} = {}) {
  let mode = options.mode ?? 'danger-full-access'
  let spawns = 0
  // Match the canonical paths used by the host and resident child.
  const home = await realpath(await mkdtemp(path.join(os.tmpdir(), 'uml panel home ')))
  roots.push(home)
  const projects: Record<string, string> = {}
  for (const id of ['session-a', 'session-b']) {
    const root = await realpath(await mkdtemp(path.join(os.tmpdir(), `uml panel ${id} `)))
    roots.push(root)
    projects[id] = root
    await writeFile(path.join(root, 'fake-binary.json'), JSON.stringify({ exitCode: 0 }))
    await mkdir(path.join(root, 'examples'))
    await writeFile(path.join(root, 'examples', 'sample.policy.edn'), '{:out "diagram.edn"}')
    await writeFile(path.join(root, 'resident.json'), JSON.stringify({
      framePath: goldenFrame, ...options.resident, ...(options.displayError ? { displayError: options.displayError } : {}),
    }))
  }
  const definitions = new Map<string, ToolDefinition>()
  const prompts: Record<string, FakeSystemPrompt> = { 'session-a': fakeSystemPrompt(), 'session-b': fakeSystemPrompt() }
  const contributions: unknown[] = []
  const routes: Route[] = []
  const agents: Record<string, Agent> = {}
  for (const id of Object.keys(projects)) {
    agents[id] = { id, session: { id }, ctx: { systemPrompt: prompts[id] } } as unknown as Agent
  }
  const services: HostServices = {
    tools: { register(definition) { definitions.set(definition.name, definition); return () => { definitions.delete(definition.name) } } },
    sandboxPolicy: { resolve: ({ session }) => ({ mode, workspaceRoot: projects[session.id]! }) },
    workingDirectory: { get: session => projects[session.id]!, ensure: async agent => projects[agent.session.id]! },
    subprocess: { spawn(spec) { spawns += 1; return localSubprocess(spec) } },
    sandbox: { confine: async argv => ({ argv: [...argv], enforcement: 'full' }) },
    typert: { register(contribution: unknown) { contributions.push(contribution); return async () => { contributions.splice(contributions.indexOf(contribution), 1) } } },
    webServer: { register(route: Route) { routes.push(route); return () => { routes.splice(routes.indexOf(route), 1) } } },
    dshHomePath: (...segments: string[]) => path.join(home, ...segments),
  }
  const ctx = new Context()
  contexts.push(ctx)
  const optional = ['typert', 'webServer'] as const
  for (const [key, value] of Object.entries(services)) {
    if (value === undefined) continue
    if (options.late && (optional as readonly string[]).includes(key)) continue
    ctx.provide(key, value)
  }
  const fiber = ctx.plugin(plugin, { binary: fakeBinary, home: path.join(home, 'dsh-uml-viewer') })
  await fiber
  const tool = async (name: string, args: unknown, session: string) => {
    const definition = definitions.get(name)
    expect(definition, `Cordis mounted ${name}`).toBeDefined()
    return definition!.execute(args, {
      agent: agents[session], signal: new AbortController().signal,
    } as ToolRunContext)
  }
  const viewer = () => ctx.get('umlViewer') as unknown as {
    watch(agent: Agent, signal: AbortSignal): AsyncIterable<PanelEvent>
    action(agent: Agent, action: unknown): Promise<{ accepted: boolean; reason: string | null }>
    close(agent: Agent): Promise<{ closed: boolean }>
  }
  const stream = (session: string) => {
    const controller = new AbortController()
    controllers.push(controller)
    const iterator = viewer().watch(agents[session]!, controller.signal)[Symbol.asyncIterator]()
    return {
      controller,
      next: async () => (await iterator.next()) as IteratorResult<PanelEvent>,
      /** A user close: the explicit Remote call, then the stream's own teardown. */
      close: async () => {
        await viewer().close(agents[session]!)
        controller.abort()
        await iterator.return?.(undefined)
      },
      /** A carrier loss: abort the signal and return the iterator, as the gateway does. */
      withdraw: async () => {
        controller.abort()
        await iterator.return?.(undefined)
      },
    }
  }
  /** Provide an optional service after the plugin loaded (Cordis starts a late fiber). */
  const provideLate = (key: 'typert' | 'webServer'): void => {
    ctx.provide(key, services[key] as never)
  }
  return { home, projects, services, definitions, contributions, routes, agents, prompts, ctx, fiber, tool, viewer, stream, provideLate,
    setMode(value: typeof mode) { mode = value }, spawnCount: () => spawns }
}

async function actionLog(root: string): Promise<{ action: string }[]> {
  const text = await readFile(path.join(root, 'resident-actions.log'), 'utf8')
  return text.trim().split('\n').map(line => JSON.parse(line) as { action: string })
}

/** The actions the panel asked for, without the save/quit pair the panel's close always sends. */
async function contentActions(root: string): Promise<string[]> {
  return (await actionLog(root)).map(entry => entry.action).filter(action => action !== 'save' && action !== 'quit')
}

async function seedState(home: string, session: string, patch: Record<string, unknown>): Promise<string> {
  const states = path.join(home, 'dsh-uml-viewer', 'sessions')
  await mkdir(states, { recursive: true })
  const file = path.join(states, `${session}.json`)
  await writeFile(file, JSON.stringify({
    schema: 'dsh-uml-viewer/session-state', version: 1, diagram: null, context: null,
    focus: [], proposalId: null, openLayer: null, declutter: null,
    camera: { x: 0, y: 0, zoom: 1 }, selected: null, detail: null, ...patch,
  }))
  return file
}

/** Read a JSON file another process writes, once it exists and is complete. */
async function readJson(file: string): Promise<Record<string, unknown>> {
  return timeout(new Promise((resolve) => {
    const timer = setInterval(async () => {
      try { const value = JSON.parse(await readFile(file, 'utf8')); clearInterval(timer); resolve(value) } catch { /* not yet */ }
    }, 25)
  }))
}

async function timeout<T>(promise: Promise<T>, ms = 8000): Promise<T> {
  return Promise.race([promise, new Promise<T>((_resolve, reject) => setTimeout(() => reject(new Error('timed out')), ms))])
}

it('refuses uml_display until the latest uml_ir in that session succeeded', async () => {
  const { tool, projects } = await setup()
  const refused = await tool('uml_display', {}, 'session-a') as unknown as Record<string, unknown>
  expect(refused).toMatchObject({ version: 1, operation: 'display', status: 'refused', validResult: false, exitCode: null,
    reason: expect.stringMatching(/uml_ir/) })
  expect(existsSync(path.join(projects['session-a']!, 'resident-invocation.json'))).toBe(false)

  expect(await tool('uml_ir', {}, 'session-a')).toMatchObject({ validResult: true })
  const shown = await tool('uml_display', {}, 'session-a') as unknown as Record<string, unknown>
  expect(shown).toMatchObject({ operation: 'display', status: 'ok', validResult: true, reason: null,
    projectRoot: projects['session-a'], path: null })
  expect(await contentActions(projects['session-a']!)).toEqual(['open', 'reload'])
  // The transient resident is stopped again: the tool leaves no running panel.
  expect(existsSync(path.join(projects['session-a']!, 'resident-exit.json'))).toBe(true)
})

it('refuses uml_display after the latest uml_ir failed, and per session', async () => {
  const { tool, projects } = await setup()
  await writeFile(path.join(projects['session-a']!, 'fake-binary.json'), JSON.stringify({ exitCode: 1, stderr: 'bad policy\n' }))
  expect(await tool('uml_ir', {}, 'session-a')).toMatchObject({ validResult: false })
  expect(await tool('uml_display', {}, 'session-a')).toMatchObject({ status: 'refused',
    reason: expect.stringMatching(/uml_ir/) })
  // A successful IR elsewhere does not gate this session.
  expect(await tool('uml_ir', {}, 'session-b')).toMatchObject({ validResult: true })
  expect(await tool('uml_display', {}, 'session-a')).toMatchObject({ status: 'refused' })
  expect(await tool('uml_display', {}, 'session-b')).toMatchObject({ status: 'ok' })
})

it('revokes the gate when a later uml_ir fails', async () => {
  const { tool, projects } = await setup()
  expect(await tool('uml_ir', {}, 'session-a')).toMatchObject({ validResult: true })
  await writeFile(path.join(projects['session-a']!, 'fake-binary.json'), JSON.stringify({ exitCode: 3 }))
  expect(await tool('uml_ir', {}, 'session-a')).toMatchObject({ validResult: false, exitCode: 3 })
  expect(await tool('uml_display', {}, 'session-a')).toMatchObject({ status: 'refused' })
})

it('revokes a successful grant on refused, parse-failed and caught uml_ir invocations', async () => {
  const { tool, services } = await setup()
  expect(await tool('uml_ir', {}, 'session-a')).toMatchObject({ validResult: true })

  expect(await tool('uml_ir', { policy: '../outside.policy.edn' }, 'session-a'))
    .toMatchObject({ status: 'refused', validResult: false })
  expect(await tool('uml_display', {}, 'session-a')).toMatchObject({ status: 'refused', validResult: false })

  expect(await tool('uml_ir', {}, 'session-a')).toMatchObject({ validResult: true })
  services.workingDirectory.ensure = async () => { throw new Error('controlled preparation failure') }
  expect(await tool('uml_ir', {}, 'session-a')).toMatchObject({ status: 'error', validResult: false })
  expect(await tool('uml_display', {}, 'session-a')).toMatchObject({ status: 'refused', validResult: false })
})

it('reports a resident error instead of claiming the panel refreshed', async () => {
  const { tool, projects } = await setup({ displayError: 'unreadable IR' })
  expect(await tool('uml_ir', {}, 'session-a')).toMatchObject({ validResult: true })
  expect(await tool('uml_display', {}, 'session-a')).toMatchObject({ status: 'error',
    reason: expect.stringMatching(/unreadable IR/) })
  expect(await contentActions(projects['session-a']!)).toEqual(['open', 'reload'])
})

it('falls back to the source diagram when the persisted proposal no longer exists', async () => {
  const { home, projects, stream, tool } = await setup({ resident: { unknownProposals: [':p-stale'] } })
  const stored = await seedState(home, 'session-a', {
    context: { kind: 'proposal', proposalId: ':p-stale', name: 'Deleted proposal', edn: '{:context :proposal :proposal-id :p-stale}' },
    proposalId: ':p-stale',
  })
  const reader = stream('session-a')
  expect((await timeout(reader.next())).value).toMatchObject({ kind: 'ready' })
  expect((await timeout(reader.next())).value).toMatchObject({ kind: 'context',
    context: { kind: 'proposal', proposalId: ':p-stale' } })
  expect((await timeout(reader.next())).value).toMatchObject({ kind: 'frame' })

  expect(await tool('uml_ir', {}, 'session-a')).toMatchObject({ validResult: true })
  const shown = await tool('uml_display', {}, 'session-a') as unknown as Record<string, unknown>
  expect(shown).toMatchObject({ status: 'ok', validResult: true,
    reason: expect.stringMatching(/previous proposal no longer exists; showing source diagram/) })
  const refreshed = await timeout(reader.next())
  expect(refreshed.value).toMatchObject({ kind: 'frame' })
  if (refreshed.value?.kind === 'frame') expect(refreshed.value.frame.waiting).toBe(false)
  expect((await timeout(reader.next())).value).toEqual({ kind: 'context', context: { kind: 'real', edn: '{:context :real}' } })
  expect(await contentActions(projects['session-a']!)).toEqual(['open', 'reload', 'context', 'context'])
  expect(JSON.parse(await readFile(stored, 'utf8'))).toMatchObject({
    context: { kind: 'real', edn: '{:context :real}' }, proposalId: null,
  })
  await reader.close()
})

it('restores an existing persisted proposal after showing a diagram', async () => {
  const { home, projects, tool } = await setup()
  const stored = await seedState(home, 'session-a', {
    context: { kind: 'proposal', proposalId: ':p-ledger', name: 'Ledger proposal', edn: '{:context :proposal :proposal-id :p-ledger}' },
    proposalId: ':p-ledger',
  })
  expect(await tool('uml_ir', {}, 'session-a')).toMatchObject({ validResult: true })
  expect(await tool('uml_display', {}, 'session-a')).toMatchObject({ status: 'ok', validResult: true, reason: null })
  expect(await contentActions(projects['session-a']!)).toEqual(['open', 'reload', 'context', 'reload'])
  expect(JSON.parse(await readFile(stored, 'utf8'))).toMatchObject({
    context: { kind: 'proposal', proposalId: ':p-ledger' }, proposalId: ':p-ledger',
  })
})

it('still refuses uml_display after failed IR even when the persisted proposal is stale', async () => {
  const { home, projects, tool } = await setup({ resident: { unknownProposals: [':p-stale'] } })
  await seedState(home, 'session-a', {
    context: { kind: 'proposal', proposalId: ':p-stale', name: 'Deleted proposal', edn: '{:context :proposal :proposal-id :p-stale}' },
    proposalId: ':p-stale',
  })
  await writeFile(path.join(projects['session-a']!, 'fake-binary.json'), JSON.stringify({ exitCode: 1, stderr: 'bad policy\\n' }))
  expect(await tool('uml_ir', {}, 'session-a')).toMatchObject({ status: 'error', validResult: false })
  expect(await tool('uml_display', {}, 'session-a')).toMatchObject({ status: 'refused', validResult: false,
    reason: expect.stringMatching(/latest uml_ir in this session failed/) })
  expect(existsSync(path.join(projects['session-a']!, 'resident-invocation.json'))).toBe(false)
})

it('refuses the panel stream and uml_display in read-only, spawning nothing', async () => {
  const { tool, projects, stream } = await setup({ mode: 'read-only' })
  const events: PanelEvent[] = []
  const reader = stream('session-a')
  for (let step = 0; step < 1; step++) {
    const value = await timeout(reader.next())
    events.push(value.value!)
  }
  expect(events[0]).toMatchObject({ kind: 'refused', reason: expect.stringMatching(/read-only/) })
  expect(await timeout(reader.next()).then(value => value.done)).toBe(true)
  expect(await tool('uml_display', {}, 'session-a')).toMatchObject({ status: 'refused',
    reason: expect.stringMatching(/read-only/) })
  expect(existsSync(path.join(projects['session-a']!, 'resident-invocation.json'))).toBe(false)
})

it('refuses a new watcher after its existing resident session becomes read-only', async () => {
  const { stream, setMode, spawnCount, tool } = await setup()
  const first = stream('session-a')
  expect((await timeout(first.next())).value).toMatchObject({ kind: 'ready' })
  const before = spawnCount()

  setMode('read-only')
  const second = stream('session-a')
  expect((await timeout(second.next())).value).toMatchObject({ kind: 'refused', reason: expect.stringMatching(/read-only/) })
  expect((await timeout(second.next())).done).toBe(true)
  expect(spawnCount()).toBe(before)
  expect(await tool('uml_display', {}, 'session-a')).toMatchObject({ status: 'refused', validResult: false })
  setMode('danger-full-access')
  await first.close()
})

it('streams the resident scene frame to the panel bound to that session', async () => {
  const { stream, projects, tool, agents, viewer } = await setup()
  const reader = stream('session-a')
  const ready = await timeout(reader.next())
  expect(ready.value).toMatchObject({ kind: 'ready', actions: expect.arrayContaining(['display', 'reload']) })
  const first = await timeout(reader.next())
  expect(first.value?.kind).toBe('frame')
  if (first.value?.kind === 'frame') {
    expect(first.value.frame.waiting).toBe(true)
    expect(first.value.frame.schema).toBe('dsh-uml-viewer/scene-frame')
  }
  const invocation = JSON.parse(await readFile(path.join(projects['session-a']!, 'resident-invocation.json'), 'utf8'))
  expect(invocation).toEqual({ cwd: projects['session-a'], argv: ['resident'] })

  // The descriptor's result codec is the panel contract: every yielded item passes it.
  for (const event of [ready.value, first.value]) {
    expect(parsePanelEvent(event)).toEqual(event)
  }

  // A display from the agent reaches the open panel stream for the same session.
  expect(await tool('uml_ir', {}, 'session-a')).toMatchObject({ validResult: true })
  expect(await tool('uml_display', {}, 'session-a')).toMatchObject({ status: 'ok' })
  const shown = await timeout(reader.next())
  expect(shown.value?.kind).toBe('frame')
  if (shown.value?.kind === 'frame') expect(shown.value.frame.waiting).toBe(false)

  // The panel's own action call reaches the same resident.
  expect(await viewer().action(agents['session-a']!, { action: 'search', query: 'graph' }))
    .toEqual({ accepted: true, reason: null })
  const searched = await timeout(reader.next())
  expect(searched.value).toMatchObject({ kind: 'search', query: 'graph' })
  expect(await contentActions(projects['session-a']!)).toEqual(['open', 'reload', 'search'])
  await reader.close()
  expect((await actionLog(projects['session-a']!)).map(entry => entry.action))
    .toEqual(['open', 'reload', 'save', 'search', 'save', 'quit'])
})

it('keeps one resident per session and stops it when the last panel closes', async () => {
  const { stream, projects } = await setup()
  const first = stream('session-a')
  await timeout(first.next())
  const second = stream('session-a')
  await timeout(second.next())
  const other = stream('session-b')
  await timeout(other.next())
  expect((await readJson(path.join(projects['session-b']!, 'resident-invocation.json'))).cwd)
    .toBe(projects['session-b'])
  expect(await contentActions(projects['session-a']!)).toEqual(['open'])

  await first.close()
  expect(existsSync(path.join(projects['session-a']!, 'resident-exit.json'))).toBe(false)
  await second.close()
  await timeout(new Promise<void>(resolve => {
    const timer = setInterval(() => {
      if (existsSync(path.join(projects['session-a']!, 'resident-exit.json'))) { clearInterval(timer); resolve() }
    }, 25)
  }))
  const exit = await readJson(path.join(projects['session-a']!, 'resident-exit.json'))
  expect(['quit', 'stdin']).toContain(exit.reason)
  expect(existsSync(path.join(projects['session-b']!, 'resident-exit.json'))).toBe(false)
  await other.close()
})

it('keeps the resident through a withdrawn stream and stops it only on an explicit close', async () => {
  const { stream, projects, prompts } = await setup()
  const reader = stream('session-a')
  await timeout(reader.next())
  await timeout(reader.next())
  // A carrier loss (a gateway reconnect, an HMR client swap) returns the
  // iterator: the panel is gone from the transport but was never closed.
  await reader.withdraw()
  expect(existsSync(path.join(projects['session-a']!, 'resident-exit.json'))).toBe(false)
  expect(prompts['session-a']!.sections).toEqual([])
  // The same session remounts and joins the waiting resident: no second child,
  // no second `open`, and the prompt section is back for the live stream.
  const remount = stream('session-a')
  await timeout(remount.next())
  expect(prompts['session-a']!.sections).toHaveLength(1)
  expect(await contentActions(projects['session-a']!)).toEqual(['open'])
  await remount.close()
  await timeout(new Promise<void>(resolve => {
    const timer = setInterval(() => {
      if (existsSync(path.join(projects['session-a']!, 'resident-exit.json'))) { clearInterval(timer); resolve() }
    }, 25)
  }))
  expect(['quit', 'stdin']).toContain((await readJson(path.join(projects['session-a']!, 'resident-exit.json'))).reason)
})

it('hands a session from a closing sidebar stream to an opening main-page stream', async () => {
  const { stream, projects } = await setup()
  const first = stream('session-a')
  expect((await timeout(first.next())).value).toMatchObject({ kind: 'ready' })
  // The main page mounts as the sidebar unmounts. Its watcher must either
  // join a still-live resident or wait for the close and start a fresh one,
  // never attach to the stream that is being stopped.
  const closing = first.close()
  const second = stream('session-a')
  expect((await timeout(second.next())).value).toMatchObject({ kind: 'ready' })
  expect((await timeout(second.next())).value?.kind).toBe('frame')
  await closing
  await second.close()
  expect(existsSync(path.join(projects['session-a']!, 'resident-invocation.json'))).toBe(true)
})

it('sends the restored discussion context with the first panel stream', async () => {
  const { home, stream } = await setup()
  const states = path.join(home, 'dsh-uml-viewer', 'sessions')
  await mkdir(states, { recursive: true })
  await writeFile(path.join(states, 'session-a.json'), JSON.stringify({
    schema: 'dsh-uml-viewer/session-state', version: 1, diagram: null,
    context: { kind: 'proposal', proposalId: ':p-ledger', name: 'Ledger proposal', edn: '{:context :proposal}' },
    focus: [], proposalId: ':p-ledger', openLayer: null, declutter: null,
    camera: { x: 0, y: 0, zoom: 1 }, selected: null, detail: null,
  }))
  const reader = stream('session-a')
  expect((await timeout(reader.next())).value).toMatchObject({ kind: 'ready' })
  expect((await timeout(reader.next())).value).toEqual({
    kind: 'context',
    context: { kind: 'proposal', proposalId: ':p-ledger', name: 'Ledger proposal', edn: '{:context :proposal}' },
  })
  expect((await timeout(reader.next())).value?.kind).toBe('frame')
  await reader.close()
})

it('registers the companion prompt section only while that session’s panel stream is open', async () => {
  const { stream, prompts } = await setup()
  expect(prompts['session-a']!.sections).toEqual([])
  const reader = stream('session-a')
  await timeout(reader.next())
  expect(prompts['session-a']!.sections).toHaveLength(1)
  expect(prompts['session-a']!.sections[0]).toMatchObject({ name: 'dsh-uml-viewer:companion-rules', interpolate: false })
  const text = prompts['session-a']!.sections[0]!.text as string
  // One source: the section is the standalone rules document, verbatim.
  const rules = await readFile(new URL('../../../docs/companion-rules.md', import.meta.url), 'utf8')
  expect(text).toBe(rules.trimEnd())
  expect(text).toMatch(/uml_ir/)
  expect(text).toMatch(/uml_display/)
  expect(text).toMatch(/serial/i)
  expect(text).toMatch(/policy/i)
  expect(text).toMatch(/proposal/i)
  expect(text).toMatch(/discussion context/i)
  expect(prompts['session-b']!.sections).toEqual([])
  await reader.close()
  expect(prompts['session-a']!.sections).toEqual([])
})

it('publishes the hand-written typed Remote for the browser to mount', async () => {
  const { contributions, ctx, fiber } = await setup()
  expect(contributions).toHaveLength(1)
  const contribution = contributions[0] as { package: string; face: string; invocations: { id: string }[] }
  expect(contribution.package).toBe('dsh-uml-viewer')
  expect(contribution.face).toBe('host')
  expect(contribution.invocations.map(invocation => invocation.id).sort())
    .toEqual(UML_VIEWER_REMOTE.descriptors.map(descriptor => descriptor.id).sort())
  expect(ctx.get('umlViewer')).toMatchObject({ typertRemote: { serviceKey: 'umlViewer', namespace: 'umlViewer' } })
  await fiber.dispose()
  expect(contributions).toHaveLength(0)
  let after: unknown
  try { after = ctx.get('umlViewer') } catch { after = undefined }
  expect(after).toBeUndefined()
})

it('persists the panel view state per session and restores it when the panel reopens', async () => {
  const { stream, home, projects } = await setup()
  const first = stream('session-a')
  await timeout(first.next())
  await timeout(first.next())
  await first.close()
  const stored = path.join(home, 'dsh-uml-viewer', 'sessions', 'session-a.json')
  await timeout(new Promise<void>(resolve => {
    const timer = setInterval(() => { if (existsSync(stored)) { clearInterval(timer); resolve() } }, 25)
  }))
  const state = await readJson(stored)
  expect(state).toMatchObject({ schema: 'dsh-uml-viewer/session-state', version: 1, declutter: ':triangles',
    camera: { x: 4, y: 5, zoom: 0.75 } })
  const reopen = stream('session-a')
  await timeout(reopen.next())
  expect((await actionLog(projects['session-a']!)).map(entry => entry.action))
    .toEqual(['open', 'save', 'quit', 'open'])
  const openAction = JSON.parse((await readFile(path.join(projects['session-a']!, 'resident-actions.log'), 'utf8'))
    .trim().split('\n')[3]!) as { state?: unknown }
  expect(openAction.state).toMatchObject({ declutter: ':triangles' })
  await reopen.close()
})

it('forwards the state event’s breadcrumb path and status code to the panel (#45)', async () => {
  const { stream, viewer, agents } = await setup()
  const reader = stream('session-a')
  // ready, then the frame the resident opened with.
  await timeout(reader.next())
  await timeout(reader.next())
  expect(await viewer().action(agents['session-a']!, { action: 'save' })).toEqual({ accepted: true, reason: null })
  const next = await timeout(reader.next())
  expect(next.done).toBe(false)
  expect(next.value).toEqual({
    kind: 'state',
    state: expect.objectContaining({ schema: 'dsh-uml-viewer/session-state', version: 1 }),
    breadcrumbs: [{ id: null, name: 'Shop' }, { id: ':core', name: 'core' }],
    status: { code: 'waiting-display' },
  })
  await reader.close()
})

it('persists the view as soon as a panel action changes it, not only when the panel closes', async () => {
  const { stream, viewer, agents, home, projects } = await setup()
  const reader = stream('session-a')
  await timeout(reader.next())
  await timeout(reader.next())
  expect(await viewer().action(agents['session-a']!, { action: 'drill-in', id: ':core' }))
    .toEqual({ accepted: true, reason: null })
  const stored = path.join(home, 'dsh-uml-viewer', 'sessions', 'session-a.json')
  // The file is already there while the panel is still open: a DSH shutdown
  // cannot lose the last drill-in, pan or zoom.
  expect(await readJson(stored)).toMatchObject({
    schema: 'dsh-uml-viewer/session-state', camera: { x: 4, y: 5, zoom: 0.75 },
  })
  expect((await actionLog(projects['session-a']!)).map(entry => entry.action))
    .toEqual(['open', 'drill-in', 'save'])
  await reader.close()
})

it('saves the open panel view when the plugin unloads, as on a DSH shutdown', async () => {
  const { stream, home, fiber } = await setup()
  const reader = stream('session-a')
  await timeout(reader.next())
  await timeout(reader.next())
  const stored = path.join(home, 'dsh-uml-viewer', 'sessions', 'session-a.json')
  expect(existsSync(stored)).toBe(false)
  await fiber.dispose()
  expect(JSON.parse(await readFile(stored, 'utf8'))).toMatchObject({
    schema: 'dsh-uml-viewer/session-state', focus: [':core'], camera: { x: 4, y: 5, zoom: 0.75 },
  })
})

it('publishes the Remote and the font route when the optional services arrive after the plugin', async () => {
  // A profile may activate typert and webServer rows after this plugin; the
  // panel must still get its Remote and the Sarasa route instead of silently
  // degrading (a snapshot of the services at apply time would miss them).
  const { contributions, routes, provideLate } = await setup({ late: true })
  expect(contributions).toEqual([])
  expect(routes).toEqual([])
  provideLate('typert')
  await new Promise(resolve => setTimeout(resolve, 0))
  expect(contributions).toHaveLength(1)
  provideLate('webServer')
  await new Promise(resolve => setTimeout(resolve, 0))
  expect(routes).toHaveLength(1)
  expect(routes[0]).toMatchObject({ kind: 'exact', path: '/dsh-uml-viewer/font/SarasaMonoSC-Regular.ttf' })
})
