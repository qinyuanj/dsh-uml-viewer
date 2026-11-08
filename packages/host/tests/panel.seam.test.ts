import { Context } from '@deepseek-ai/cordis'
import { spawn } from 'node:child_process'
import { existsSync } from 'node:fs'
import { mkdir, mkdtemp, readFile, rm, writeFile } from 'node:fs/promises'
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
} = {}) {
  const mode = options.mode ?? 'danger-full-access'
  const home = await mkdtemp(path.join(os.tmpdir(), 'uml panel home '))
  roots.push(home)
  const projects: Record<string, string> = {}
  for (const id of ['session-a', 'session-b']) {
    const root = await mkdtemp(path.join(os.tmpdir(), `uml panel ${id} `))
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
    subprocess: { spawn: localSubprocess },
    sandbox: { confine: async argv => ({ argv: [...argv], enforcement: 'full' }) },
    typert: { register(contribution: unknown) { contributions.push(contribution); return async () => { contributions.splice(contributions.indexOf(contribution), 1) } } },
    webServer: { register(route: Route) { routes.push(route); return () => { routes.splice(routes.indexOf(route), 1) } } },
    dshHomePath: (...segments: string[]) => path.join(home, ...segments),
  }
  const ctx = new Context()
  contexts.push(ctx)
  for (const [key, value] of Object.entries(services)) if (value !== undefined) ctx.provide(key, value)
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
  }
  const stream = (session: string) => {
    const controller = new AbortController()
    controllers.push(controller)
    const iterator = viewer().watch(agents[session]!, controller.signal)[Symbol.asyncIterator]()
    return {
      controller,
      next: async () => (await iterator.next()) as IteratorResult<PanelEvent>,
      close: async () => { controller.abort(); await iterator.return?.(undefined) },
    }
  }
  return { home, projects, services, definitions, contributions, routes, agents, prompts, ctx, fiber, tool, viewer, stream }
}

async function actionLog(root: string): Promise<{ action: string }[]> {
  const text = await readFile(path.join(root, 'resident-actions.log'), 'utf8')
  return text.trim().split('\n').map(line => JSON.parse(line) as { action: string })
}

/** The actions the panel asked for, without the save/quit pair the panel's close always sends. */
async function contentActions(root: string): Promise<string[]> {
  return (await actionLog(root)).map(entry => entry.action).filter(action => action !== 'save' && action !== 'quit')
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

it('reports a resident error instead of claiming the panel refreshed', async () => {
  const { tool, projects } = await setup({ displayError: 'unreadable IR' })
  expect(await tool('uml_ir', {}, 'session-a')).toMatchObject({ validResult: true })
  expect(await tool('uml_display', {}, 'session-a')).toMatchObject({ status: 'error',
    reason: expect.stringMatching(/unreadable IR/) })
  expect(await contentActions(projects['session-a']!)).toEqual(['open', 'reload'])
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
    .toEqual(['open', 'reload', 'search', 'save', 'quit'])
})

it('keeps one resident per session and stops it when the last panel stream closes', async () => {
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

it('registers the companion prompt section only while that session’s panel stream is open', async () => {
  const { stream, prompts } = await setup()
  expect(prompts['session-a']!.sections).toEqual([])
  const reader = stream('session-a')
  await timeout(reader.next())
  expect(prompts['session-a']!.sections).toHaveLength(1)
  expect(prompts['session-a']!.sections[0]).toMatchObject({ name: 'dsh-uml-viewer:companion-rules', interpolate: false })
  const text = prompts['session-a']!.sections[0]!.text as string
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
