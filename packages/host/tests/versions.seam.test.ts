import { Context } from '@deepseek-ai/cordis'
import { spawn } from 'node:child_process'
import { createHash } from 'node:crypto'
import { existsSync } from 'node:fs'
import { copyFile, mkdir, mkdtemp, readdir, readFile, rm, stat, utimes, writeFile } from 'node:fs/promises'
import os from 'node:os'
import path from 'node:path'
import { fileURLToPath, pathToFileURL } from 'node:url'
import { afterEach, expect, it, vi } from 'vitest'
import type { PanelEvent } from '@dsh-uml-viewer/contracts'
import type { Agent, HostServices, SpawnSpec } from '../src/dsh.js'
import * as plugin from '../src/index.js'

// #32: the packaged binary lifecycle across plugin versions sharing one DSH
// home. Each "installed version" is a scratch main package plus its exact
// platform dependency whose payload is a fake resident (a Node script), or on
// Windows optionally a copy of the running Node executable so the version
// directory really holds an exe that is in use.

const goldenFrame = fileURLToPath(new URL('../../../crates/frame/tests/fixtures/frames/root.frame.json', import.meta.url))
const residentFixture = pathToFileURL(fileURLToPath(new URL('./fixtures/resident.mjs', import.meta.url))).href
const target = `${process.platform}-${process.arch}`
const exe = process.platform === 'win32' ? 'uml-viewer.exe' : 'uml-viewer'

const contexts: Context[] = []
const controllers: AbortController[] = []
const roots: string[] = []
afterEach(async () => {
  for (const controller of controllers.splice(0)) controller.abort()
  for (const ctx of contexts.splice(0)) await ctx.fiber.dispose()
  vi.unstubAllEnvs()
  for (const root of roots.splice(0)) await rm(root, { recursive: true, force: true, maxRetries: 20, retryDelay: 100 })
})

async function scratch(label: string): Promise<string> {
  const root = await mkdtemp(path.join(os.tmpdir(), `uml versions ${label} `))
  roots.push(root)
  return root
}

/** The fake resident payload: records which version and file ran, then speaks the protocol. */
function payloadScript(version: string): string {
  return `require('node:fs').appendFileSync('resident-versions.log', JSON.stringify({ version: ${JSON.stringify(version)}, binary: __filename }) + '\\n')\n`
    + `import(${JSON.stringify(residentFixture)})\n`
}

/** One installed main package `version` with its platform payload. */
async function install(version: string, options: { native?: boolean } = {}): Promise<string> {
  const root = await scratch(`package ${version}`)
  const dependency = `@dsh-uml-viewer/${target}`
  await writeFile(path.join(root, 'package.json'), JSON.stringify({ name: 'dsh-uml-viewer', version,
    optionalDependencies: { [dependency]: version } }))
  const directory = path.join(root, 'node_modules', dependency)
  await mkdir(path.join(directory, 'bin'), { recursive: true })
  await writeFile(path.join(directory, 'package.json'), JSON.stringify({ name: dependency, version }))
  const payload = path.join(directory, 'bin', exe)
  if (options.native) await copyFile(process.execPath, payload)
  else await writeFile(payload, payloadScript(version))
  return root
}

function localSubprocess(spec: SpawnSpec, wrappers: Map<string, string>) {
  const [command, ...rest] = spec.argv
  // A native payload is a copy of node.exe: run the version's wrapper script with it.
  const wrapper = wrappers.get(command!)
  const child = wrapper
    ? spawn(command!, [wrapper, ...rest], { cwd: spec.cwd, stdio: ['pipe', 'pipe', 'pipe'] })
    : spawn(process.execPath, [command!, ...rest], { cwd: spec.cwd, stdio: ['pipe', 'pipe', 'pipe'] })
  const chunks: Buffer[] = []
  child.stderr.on('data', (chunk: Buffer) => chunks.push(chunk))
  const reader = { readFrom: () => ({ text: Buffer.concat(chunks).toString(), lossy: false }) }
  return {
    stdin: child.stdin,
    stdout: child.stdout,
    stderr: child.stderr,
    collected: { stdout: reader, stderr: reader },
    done: new Promise<{ exitCode: number | null; signal: string | null }>((resolve, reject) => {
      child.on('error', reject)
      child.on('close', (exitCode, signal) => resolve({ exitCode, signal }))
    }),
    terminate: () => child.kill(),
    waitForExit: async () => true,
  }
}

/** A DSH host process: its own Cordis context and sessions, sharing the DSH home. */
async function host(dshHome: string, packageRoot: string, wrappers = new Map<string, string>()) {
  vi.stubEnv('UML_VIEWER_BIN', undefined)
  const projects: Record<string, string> = {}
  const agents: Record<string, Agent> = {}
  const services: HostServices = {
    tools: { register: () => () => {} },
    sandboxPolicy: { resolve: ({ session }) => ({ mode: 'danger-full-access', workspaceRoot: projects[session.id]! }) },
    workingDirectory: { get: session => projects[session.id]!, ensure: async agent => projects[agent.session.id]! },
    subprocess: { spawn: spec => localSubprocess(spec, wrappers) },
    typert: { register: () => async () => {} },
    dshHomePath: (...segments: string[]) => path.join(dshHome, ...segments),
  }
  const ctx = new Context()
  contexts.push(ctx)
  for (const [key, value] of Object.entries(services)) ctx.provide(key, value)
  const fiber = ctx.plugin({ name: 'uml-viewer-version-host',
    apply: (scope: Context) => plugin.applyWithPlatform(scope, {}, { platform: process.platform, arch: process.arch, root: packageRoot }) })
  await fiber
  const panel = async (session: string) => {
    const project = await scratch(`project ${session}`)
    projects[session] = project
    agents[session] = { id: session, session: { id: session } }
    await writeFile(path.join(project, 'resident.json'), JSON.stringify({ framePath: goldenFrame }))
    const controller = new AbortController()
    controllers.push(controller)
    const viewer = ctx.get('umlViewer') as unknown as {
      watch(agent: Agent, signal: AbortSignal): AsyncIterable<PanelEvent>
      action(agent: Agent, action: unknown): Promise<{ accepted: boolean; reason: string | null }>
    }
    const iterator = viewer.watch(agents[session]!, controller.signal)[Symbol.asyncIterator]()
    const ready = (await iterator.next()).value as PanelEvent
    expect(ready).toMatchObject({ kind: 'ready' })
    return {
      project,
      action: (value: unknown) => viewer.action(agents[session]!, value),
      /** Every resident start in this project: which version ran, from which file. */
      runs: async () => (await readFile(path.join(project, 'resident-versions.log'), 'utf8'))
        .trim().split('\n').map(line => JSON.parse(line) as { version: string; binary: string }),
      close: async () => {
        controller.abort()
        await iterator.return?.(undefined)
      },
    }
  }
  return { ctx, fiber, panel, dispose: () => ctx.fiber.dispose() }
}

const sha256 = async (file: string) => createHash('sha256').update(await readFile(file)).digest('hex')

it('pins the platform payload under <DSH home>/dsh-uml-viewer/<version>/ and records it as current', async () => {
  const dshHome = await scratch('home')
  const root = await install('1.0.0')
  await host(dshHome, root)
  const home = path.join(dshHome, 'dsh-uml-viewer')
  const pinned = path.join(home, '1.0.0', exe)
  const source = path.join(root, 'node_modules', '@dsh-uml-viewer', target, 'bin', exe)
  expect(await readFile(pinned, 'utf8')).toBe(payloadScript('1.0.0'))
  expect(JSON.parse(await readFile(path.join(home, 'current.json'), 'utf8'))).toEqual({
    version: '1.0.0', size: (await stat(source)).size, sha256: await sha256(source),
  })
  // Only the published version directory, never a staging copy, remains.
  expect((await readdir(home)).filter(name => name.startsWith('.'))).toEqual([])
})

it('keeps an old panel on vN after updating, while a fresh open runs vN+1', async () => {
  const dshHome = await scratch('home')
  const home = path.join(dshHome, 'dsh-uml-viewer')
  const old = await host(dshHome, await install('1.0.0'))
  const before = await old.panel('session-old')

  const updated = await host(dshHome, await install('1.1.0'))
  const fresh = await updated.panel('session-new')

  // The running panel still answers from the same vN resident process.
  expect(await before.action({ action: 'viewport', width: 640, height: 480 })).toMatchObject({ accepted: true })
  expect(await before.runs()).toEqual([{ version: '1.0.0', binary: path.join(home, '1.0.0', exe) }])
  expect(await fresh.runs()).toEqual([{ version: '1.1.0', binary: path.join(home, '1.1.0', exe) }])
  expect(existsSync(path.join(home, '1.0.0', exe))).toBe(true)
  expect(JSON.parse(await readFile(path.join(home, 'current.json'), 'utf8'))).toMatchObject({ version: '1.1.0' })
})

it('removes the old version directory once the host still using it has stopped', async () => {
  const dshHome = await scratch('home')
  const home = path.join(dshHome, 'dsh-uml-viewer')
  const old = await host(dshHome, await install('1.0.0'))
  const before = await old.panel('session-old')
  const updated = await host(dshHome, await install('1.1.0'))
  expect(existsSync(path.join(home, '1.0.0'))).toBe(true)

  await before.close()
  await old.dispose()
  expect(existsSync(path.join(home, '1.0.0'))).toBe(false)
  expect(existsSync(path.join(home, 'leases', '1.0.0'))).toBe(false)
  // The current version stays for the next fresh start, even with no host.
  await updated.dispose()
  expect(await readFile(path.join(home, '1.1.0', exe), 'utf8')).toBe(payloadScript('1.1.0'))
})
