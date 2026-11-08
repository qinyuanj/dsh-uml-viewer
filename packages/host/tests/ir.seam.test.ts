import { Context } from '@deepseek-ai/cordis'
import { spawn } from 'node:child_process'
import { mkdtemp, mkdir, readFile, rm, writeFile, readdir, lstat, utimes } from 'node:fs/promises'
import os from 'node:os'
import path from 'node:path'
import { fileURLToPath } from 'node:url'
import { afterEach, expect, it } from 'vitest'
import * as plugin from '../src/index.js'
import type { HostServices, ToolDefinition, ToolRunContext, SpawnSpec } from '../src/dsh.js'

const roots: string[] = []
const contexts: Context[] = []
afterEach(async () => {
  for (const ctx of contexts.splice(0)) await ctx.fiber.dispose()
  for (const root of roots.splice(0)) await rm(root, { recursive: true, force: true })
})

// Narrow DSH subprocess adapter for the test: spawn a real fake binary and expose
// DSH's collected readers. The plugin never imports child_process.
function localSubprocess(spec: SpawnSpec) {
  const child = spawn(spec.argv[0]!, spec.argv.slice(1), {
    cwd: spec.cwd, stdio: ['ignore', 'pipe', 'pipe'], signal: spec.signal,
  })
  const chunks = { stdout: [] as Buffer[], stderr: [] as Buffer[] }
  child.stdout.on('data', b => chunks.stdout.push(b))
  child.stderr.on('data', b => chunks.stderr.push(b))
  const reader = (key: 'stdout' | 'stderr') => ({ readFrom: () => {
    const data = Buffer.concat(chunks[key])
    const mode = spec.stdio[key]
    const max = typeof mode === 'object' ? mode.maxBytes : data.length
    return { text: data.subarray(0, max).toString(), lossy: data.length > max }
  } })
  return {
    collected: { stdout: reader('stdout'), stderr: reader('stderr') },
    done: new Promise<{ exitCode: number | null; signal: string | null }>((resolve, reject) => {
      child.on('error', reject)
      child.on('close', (exitCode, signal) => resolve({ exitCode, signal }))
    }),
  }
}

async function setup(mode: 'read-only' | 'workspace-write' | 'danger-full-access' = 'danger-full-access') {
  const root = await mkdtemp(path.join(os.tmpdir(), 'uml project & '))
  roots.push(root)
  await writeFile(path.join(root, 'fake-binary.json'), JSON.stringify({ exitCode: 0 }))
  await mkdir(path.join(root, 'examples'))
  await writeFile(path.join(root, 'examples', 'sample.policy.edn'), '{:out "diagram.edn"}')
  const legacy = path.join(root, '.uml-viewer')
  await mkdir(path.join(legacy, 'nested'), { recursive: true })
  await writeFile(path.join(legacy, 'quality-result.json'), 'old result, deliberately not valid JSON\n')
  await writeFile(path.join(legacy, 'nested', 'companion.edn'), Buffer.from([0, 255, 8, 128]))
  const stamp = new Date('2001-01-01T00:00:00Z')
  for (const file of ['quality-result.json', 'nested/companion.edn', 'nested', '.']) {
    await utimes(path.join(legacy, file), stamp, stamp)
  }
  const legacyBefore = await legacySnapshot(legacy)
  const definitions = new Map<string, ToolDefinition>()
  const services: HostServices = {
    tools: { register(def) { definitions.set(def.name, def); return () => { definitions.delete(def.name) } } },
    sandboxPolicy: { resolve: () => ({ mode, workspaceRoot: root }) },
    workingDirectory: { get: () => root, ensure: async () => root },
    subprocess: { spawn: localSubprocess },
    sandbox: { confine: async argv => ({ argv: [...argv], enforcement: 'full', denialSignatures: [], runnerFailureRules: [] }) },
  }
  const ctx = new Context()
  contexts.push(ctx)
  for (const [key, value] of Object.entries(services)) ctx.provide(key, value)
  const fiber = ctx.plugin(plugin, { binary: [process.execPath, fileURLToPath(new URL('./fixtures/uml-viewer.mjs', import.meta.url))] })
  await fiber
  const call = async (args: unknown = {}) => {
    const def = definitions.get('uml_ir')
    expect(def, 'Cordis mounted uml_ir').toBeDefined()
    return def!.execute(args, {
      agent: { id: 'session-1', session: { id: 'session-1' } },
      signal: new AbortController().signal,
    } as ToolRunContext)
  }
  return { root, services, call, definitions, fiber, legacy, legacyBefore }
}

it.each([1, 3])('returns the actual nonzero binary exit %s and diagnostics', async exitCode => {
  const { root, call } = await setup()
  await writeFile(path.join(root, 'fake-binary.json'), JSON.stringify({ exitCode, stderr: 'bad policy\n' }))
  expect(await call()).toMatchObject({ exitCode, validResult: false, status: 'error', stderr: 'bad policy\n' })
})

it('refuses read-only without running a binary', async () => {
  const { root, call } = await setup('read-only')
  expect(await call()).toMatchObject({ exitCode: null, validResult: false, status: 'refused', reason: expect.stringMatching(/read-only/) })
  await expect(readFile(path.join(root, 'invocation.json'))).rejects.toMatchObject({ code: 'ENOENT' })
})

it('executes the sandbox provider’s confined argv in workspace-write', async () => {
  const { root, call, services } = await setup('workspace-write')
  services.sandbox!.confine = async argv => ({
    argv: [process.execPath, fileURLToPath(new URL('./confinement.mjs', import.meta.url)), ...argv],
    enforcement: 'partial', denialSignatures: [], runnerFailureRules: [],
  })
  expect(await call()).toMatchObject({ status: 'ok', validResult: true })
  expect(JSON.parse(await readFile(path.join(root, 'confined.json'), 'utf8'))[0]).toBe(process.execPath)
})

it('fails closed when confinement is unavailable', async () => {
  const { root, call, services } = await setup('workspace-write')
  services.sandbox!.confine = async () => { throw new Error('No confinement provider') }
  expect(await call()).toMatchObject({ exitCode: null, validResult: false, reason: expect.stringMatching(/confinement/i) })
  await expect(readFile(path.join(root, 'invocation.json'))).rejects.toMatchObject({ code: 'ENOENT' })
})

it('selects the project’s policy when arguments are omitted', async () => {
  const { root, call } = await setup()
  expect(await call()).toMatchObject({ status: 'ok' })
  expect(JSON.parse(await readFile(path.join(root, 'invocation.json'), 'utf8')).args)
    .toEqual(['ir', '--policy', 'examples/sample.policy.edn'])
})

it('refuses a mode change while the working directory is being validated', async () => {
  const { root, call, services } = await setup()
  services.workingDirectory.ensure = async () => {
    services.sandboxPolicy.resolve = () => ({ mode: 'read-only', workspaceRoot: root })
    return root
  }
  expect(await call()).toMatchObject({ status: 'refused', reason: expect.stringMatching(/policy changed/i) })
  await expect(readFile(path.join(root, 'invocation.json'))).rejects.toMatchObject({ code: 'ENOENT' })
})

async function legacySnapshot(dir: string): Promise<unknown[]> {
  const rows: unknown[] = []
  const visit = async (file: string) => {
    const info = await lstat(file, { bigint: true })
    rows.push({ path: path.relative(dir, file), mtime: String(info.mtimeNs),
      bytes: info.isDirectory() ? null : (await readFile(file)).toString('hex') })
    if (info.isDirectory()) for (const name of (await readdir(file)).sort()) await visit(path.join(file, name))
  }
  await visit(dir)
  return rows
}

it.each(['success', 'failure', 'read-only'] as const)('leaves legacy .uml-viewer bytes and nanosecond mtimes unchanged on %s', async scenario => {
  const { root, call, fiber, definitions, legacy, legacyBefore } = await setup(scenario === 'read-only' ? 'read-only' : 'danger-full-access')
  if (scenario === 'failure') await writeFile(path.join(root, 'fake-binary.json'), JSON.stringify({ exitCode: 1 }))
  expect(await call()).toMatchObject({ validResult: scenario === 'success' })
  await fiber.dispose()
  expect(definitions.has('uml_ir')).toBe(false)
  expect(await legacySnapshot(legacy)).toEqual(legacyBefore)
})

it('loads in Cordis and generates IR in the session cwd, returning a version-1 record', async () => {
  const { root, call } = await setup()
  const result = await call({ policy: 'examples/a & b.policy.edn', output: 'path with spaces.edn' })
  expect(result).toMatchObject({ version: 1, operation: 'ir', projectRoot: root, exitCode: 0,
    validResult: true, crapScope: null, configured: false, tool: 'uml-viewer', status: 'ok', reason: null })
  expect(JSON.parse(await readFile(path.join(root, 'invocation.json'), 'utf8'))).toEqual({ cwd: root,
    args: ['ir', '--policy', 'examples/a & b.policy.edn', '--output', 'path with spaces.edn'] })
  expect(await readFile(path.join(root, 'diagram.edn'), 'utf8')).toBe('{:classes []}\n')
})
