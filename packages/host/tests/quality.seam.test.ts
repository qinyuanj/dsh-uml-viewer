import { Context } from '@deepseek-ai/cordis'
import { spawn } from 'node:child_process'
import { existsSync } from 'node:fs'
import { mkdir, mkdtemp, readFile, rm, writeFile } from 'node:fs/promises'
import os from 'node:os'
import path from 'node:path'
import { fileURLToPath } from 'node:url'
import { afterEach, expect, it } from 'vitest'
import type { Agent, HostServices, SpawnSpec, ToolDefinition, ToolRunContext } from '../src/dsh.js'

const builtHost = new URL('../../../lib/host.js', import.meta.url).href
const plugin = await import(builtHost) as typeof import('../src/index.js')
const fakeBinary = [process.execPath, fileURLToPath(new URL('./fixtures/quality.mjs', import.meta.url))]

const roots: string[] = []
const contexts: Context[] = []
const controllers: AbortController[] = []
afterEach(async () => {
  for (const controller of controllers.splice(0)) controller.abort()
  for (const ctx of contexts.splice(0)) await ctx.fiber.dispose()
  for (const root of roots.splice(0)) await rm(root, { recursive: true, force: true })
})

function localSubprocess(spec: SpawnSpec) {
  const child = spawn(spec.argv[0]!, spec.argv.slice(1), {
    cwd: spec.cwd, stdio: ['pipe', 'pipe', 'pipe'], signal: spec.signal,
  })
  const chunks = { stdout: [] as Buffer[], stderr: [] as Buffer[] }
  child.stdout.on('data', (chunk: Buffer) => chunks.stdout.push(chunk))
  child.stderr.on('data', (chunk: Buffer) => chunks.stderr.push(chunk))
  const reader = (key: 'stdout' | 'stderr') => ({
    readFrom: () => {
      const data = Buffer.concat(chunks[key])
      const max = typeof spec.stdio[key] === 'object'
        ? (spec.stdio[key] as { maxBytes: number }).maxBytes
        : data.length
      return { text: data.subarray(0, max).toString(), lossy: data.length > max }
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

async function setup(options: {
  mode?: 'read-only' | 'workspace-write' | 'danger-full-access'
  quality?: Record<string, unknown>
  resident?: Record<string, unknown>
} = {}) {
  const home = await mkdtemp(path.join(os.tmpdir(), 'uml quality home '))
  roots.push(home)
  const projects: Record<string, string> = {}
  const prompts: Record<string, FakeSystemPrompt> = {}
  const agents: Record<string, Agent> = {}
  for (const id of ['session-a', 'session-b']) {
    const root = await mkdtemp(path.join(os.tmpdir(), `uml quality ${id} `))
    roots.push(root)
    projects[id] = root
    await writeFile(path.join(root, 'quality-binary.json'), JSON.stringify(options.quality ?? {}))
    await writeFile(path.join(root, 'fake-binary.json'), JSON.stringify({ exitCode: 0 }))
    await mkdir(path.join(root, 'examples'))
    await writeFile(path.join(root, 'examples', 'sample.policy.edn'), '{:out "diagram.edn"}')
    if (options.resident) {
      await writeFile(path.join(root, 'resident.json'), JSON.stringify({
        framePath: fileURLToPath(new URL('../../../crates/frame/tests/fixtures/frames/root.frame.json', import.meta.url)),
        ...options.resident,
      }))
    }
    prompts[id] = fakeSystemPrompt()
    agents[id] = { id, session: { id }, ctx: { systemPrompt: prompts[id] } } as unknown as Agent
  }
  let spawns = 0
  const definitions = new Map<string, ToolDefinition>()
  const services: HostServices = {
    tools: { register(def) { definitions.set(def.name, def); return () => { definitions.delete(def.name) } } },
    sandboxPolicy: { resolve: ({ session }) => ({ mode: options.mode ?? 'danger-full-access', workspaceRoot: projects[session.id]! }) },
    workingDirectory: { get: session => projects[session.id]!, ensure: async agent => projects[agent.session.id]! },
    subprocess: { spawn(spec) { spawns += 1; return localSubprocess(spec) } },
    sandbox: { confine: async argv => ({ argv: [...argv], enforcement: 'full' }) },
    dshHomePath: (...segments: string[]) => path.join(home, ...segments),
  }
  const ctx = new Context()
  contexts.push(ctx)
  for (const [key, value] of Object.entries(services)) ctx.provide(key, value)
  const fiber = ctx.plugin(plugin, { binary: fakeBinary, home: path.join(home, 'dsh-uml-viewer') })
  await fiber
  const tool = async (name: string, args: unknown, session = 'session-a', signal?: AbortSignal) => {
    const definition = definitions.get(name)
    expect(definition, `Cordis mounted ${name}`).toBeDefined()
    return definition!.execute(args, {
      agent: agents[session], signal: signal ?? new AbortController().signal,
    } as ToolRunContext)
  }
  return { home, projects, definitions, services, ctx, fiber, tool, spawnCount: () => spawns, agents }
}

async function events(root: string): Promise<{ event: string; argv: string[]; cwd: string }[]> {
  const text = await readFile(path.join(root, 'quality-events.jsonl'), 'utf8')
  return text.trim().split('\n').map(line => JSON.parse(line) as { event: string; argv: string[]; cwd: string })
}

it('runs quality crap in the session cwd and returns its version-1 record', async () => {
  const { tool, projects, spawnCount } = await setup()
  const result = await tool('uml_crap', {}) as unknown as Record<string, unknown>
  expect(result).toEqual({
    version: 1, operation: 'crap', projectRoot: projects['session-a'], exitCode: 0,
    validResult: true, crapScope: 'whole-tree', configured: false, tool: 'crapper', reason: null,
  })
  expect(await events(projects['session-a']!)).toEqual([
    { event: 'start', argv: ['quality', 'crap'], cwd: projects['session-a'], pid: expect.any(Number) },
    { event: 'end', argv: ['quality', 'crap'], cwd: projects['session-a'], pid: expect.any(Number) },
  ])
  expect(spawnCount()).toBe(1)
})

it('forwards every mutation target and --mutate-all in one quality call', async () => {
  const { tool, projects } = await setup()
  const result = await tool('uml_mutate', {
    targets: ['crates/engine/src/a.rs', 'src/dir with spaces/b.rs'], mutateAll: true,
  }) as unknown as Record<string, unknown>
  expect(result).toEqual({
    version: 1, operation: 'mutate', projectRoot: projects['session-a'], exitCode: 0,
    validResult: true, crapScope: null, configured: false, tool: 'mutator', reason: null,
  })
  expect((await events(projects['session-a']!)).filter(entry => entry.event === 'start').map(entry => entry.argv))
    .toEqual([['quality', 'mutate', '--mutate-all', 'crates/engine/src/a.rs', 'src/dir with spaces/b.rs']])
})

it('runs dry without arguments and returns its record', async () => {
  const { tool, projects } = await setup()
  expect(await tool('uml_dry', {})).toMatchObject({
    operation: 'dry', exitCode: 0, validResult: true, crapScope: null, tool: 'dryer', reason: null,
  })
  expect((await events(projects['session-a']!)).find(entry => entry.event === 'start')?.argv)
    .toEqual(['quality', 'dry'])
})

it.each([
  { label: 'tool exit 0', operation: 'crap', args: {}, config: { exitCode: 0 }, exitCode: 0, valid: true },
  { label: 'tool failure exit 1', operation: 'crap', args: {}, config: { exitCode: 1 }, exitCode: 1, valid: false },
  { label: 'undeclared exit 3', operation: 'mutate', args: { targets: ['src/a.rs'] },
    config: { exitCode: 3, validResult: false }, exitCode: 3, valid: false },
  { label: 'declared mutation survivors at exit 3', operation: 'mutate', args: { targets: ['src/a.rs'] },
    config: { exitCode: 3, validResult: true }, exitCode: 3, valid: true },
] as const)('preserves $label in the returned record', async ({ operation, args, config, exitCode, valid }) => {
  const { tool } = await setup({ quality: config })
  expect(await tool(operation === 'crap' ? 'uml_crap' : 'uml_mutate', args)).toMatchObject({
    operation, exitCode, validResult: valid, reason: null,
  })
})

it('never turns signal termination into a valid quality result', async () => {
  const { tool, services } = await setup()
  services.subprocess.spawn = spec => {
    const child = localSubprocess(spec)
    return { ...child, done: child.done.then(() => ({ exitCode: 0, signal: 'SIGTERM' })) }
  }
  expect(await tool('uml_crap', {})).toMatchObject({
    operation: 'crap', exitCode: 0, validResult: false,
    reason: expect.stringMatching(/SIGTERM/),
  })
})

it.each(['missing stdout', 'missing stderr', 'stdout throws', 'stderr throws'] as const)(
  'keeps the real quality exit when an output reader fails: %s', async failure => {
    const { tool, services } = await setup({ quality: { exitCode: 1 } })
    services.subprocess.spawn = spec => {
      const child = localSubprocess(spec)
      const brokenReader = () => ({ readFrom: () => { throw new Error('reader failed') } })
      const stdout = failure === 'missing stdout' ? undefined
        : failure === 'stdout throws' ? brokenReader() : child.collected.stdout
      const stderr = failure === 'missing stderr' ? undefined
        : failure === 'stderr throws' ? brokenReader() : child.collected.stderr
      return { ...child, collected: { ...(stdout ? { stdout } : {}), ...(stderr ? { stderr } : {}) } }
    }
    expect(await tool('uml_crap', {})).toMatchObject({
      operation: 'crap', exitCode: 1, validResult: false, reason: expect.stringMatching(/reader/),
    })
  })

it.each([
  ['malformed JSON', { exitCode: 0, stdout: 'not a record\n' }, /valid version-1 record/],
  ['another operation', { exitCode: 0, stdout: JSON.stringify({
    version: 1, operation: 'mutate', projectRoot: '<root>', exitCode: 0, validResult: true,
    crapScope: null, configured: false, tool: 'mutator', reason: null,
  }) }, /record for mutate/],
  ['another project root', { exitCode: 0, stdout: JSON.stringify({
    version: 1, operation: 'crap', projectRoot: '<other>', exitCode: 0, validResult: true,
    crapScope: 'whole-tree', configured: false, tool: 'crapper', reason: null,
  }) }, /projectRoot/],
  ['a different exit', { exitCode: 0, stdout: JSON.stringify({
    version: 1, operation: 'crap', projectRoot: '<root>', exitCode: 7, validResult: true,
    crapScope: 'whole-tree', configured: false, tool: 'crapper', reason: null,
  }) }, /reported exit 7/],
] as const)('fails closed on %s in the quality record', async (_label, config, reason) => {
  const { tool, projects } = await setup()
  const root = projects['session-a']!
  const json = (value: string) => JSON.stringify(value).slice(1, -1)
  const stdout = config.stdout
    .replaceAll('<root>', json(root))
    .replaceAll('<other>', json(path.join(os.tmpdir(), 'other project')))
  await writeFile(path.join(root, 'quality-binary.json'), JSON.stringify({ ...config, stdout }))
  expect(await tool('uml_crap', {})).toMatchObject({
    operation: 'crap', exitCode: 0, validResult: false, reason: expect.stringMatching(reason),
  })
})

it('refuses each quality tool in a read-only sandbox without spawning', async () => {
  const { tool, projects, spawnCount } = await setup({ mode: 'read-only' })
  for (const [name, args] of [
    ['uml_crap', {}], ['uml_mutate', { targets: ['src/a.rs'] }], ['uml_dry', {}],
  ] as const) {
    expect(await tool(name, args)).toMatchObject({
      validResult: false, exitCode: null, reason: expect.stringMatching(/read-only/),
    })
  }
  expect(spawnCount()).toBe(0)
  expect(existsSync(path.join(projects['session-a']!, 'quality-events.jsonl'))).toBe(false)
})

it('serializes overlapping quality calls in one session FIFO', async () => {
  const { tool, projects } = await setup({ quality: { delay: 120 } })
  const first = tool('uml_crap', {})
  const second = tool('uml_dry', {})
  expect(await Promise.all([first, second])).toMatchObject([
    { operation: 'crap', validResult: true }, { operation: 'dry', validResult: true },
  ])
  const log = await events(projects['session-a']!)
  expect(log.map(entry => entry.event)).toEqual(['start', 'end', 'start', 'end'])
  expect(log.map(entry => entry.argv[1])).toEqual(['crap', 'crap', 'dry', 'dry'])
})

it('blocks uml_ir after an invalid quality result and re-enables display after a fresh successful IR', async () => {
  const { tool, projects } = await setup({
    quality: { exitCode: 1, stderr: 'tests failed\n' },
    resident: {},
  })
  const root = projects['session-a']!
  expect(await tool('uml_crap', {})).toMatchObject({ exitCode: 1, validResult: false })
  expect(await tool('uml_ir', {})).toMatchObject({
    operation: 'ir', status: 'refused', validResult: false, exitCode: null,
    reason: expect.stringMatching(/latest quality call.*crap.*exit 1/),
  })
  expect(existsSync(path.join(root, 'invocation.json'))).toBe(false)

  await writeFile(path.join(root, 'quality-binary.json'), JSON.stringify({ exitCode: 0 }))
  expect(await tool('uml_crap', {})).toMatchObject({ validResult: true })
  expect(await tool('uml_ir', {})).toMatchObject({ status: 'ok', validResult: true, exitCode: 0 })
  expect(await tool('uml_display', {})).toMatchObject({ status: 'ok', validResult: true })

  // A later malformed IR invocation revokes the display grant, as #21 repaired it.
  expect(await tool('uml_ir', { policy: '../outside.policy.edn' })).toMatchObject({
    operation: 'ir', status: 'refused', validResult: false,
  })
  expect(await tool('uml_display', {})).toMatchObject({ status: 'refused', validResult: false })
  // A new successful IR restores it.
  expect(await tool('uml_ir', {})).toMatchObject({ status: 'ok', validResult: true })
  expect(await tool('uml_display', {})).toMatchObject({ status: 'ok', validResult: true })
})

it('keeps quality gating per session', async () => {
  const { tool, projects } = await setup({ quality: { exitCode: 1 } })
  expect(await tool('uml_crap', {}, 'session-a')).toMatchObject({ validResult: false })
  expect(await tool('uml_ir', {}, 'session-a')).toMatchObject({ status: 'refused' })
  expect(await tool('uml_ir', {}, 'session-b')).toMatchObject({ status: 'ok', validResult: true })
  expect(projects['session-b']).toBeDefined()
})
