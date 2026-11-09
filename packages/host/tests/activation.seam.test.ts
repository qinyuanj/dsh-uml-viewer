import { Context } from '@deepseek-ai/cordis'
import { mkdir, mkdtemp, readFile, rm, writeFile } from 'node:fs/promises'
import os from 'node:os'
import path from 'node:path'
import { afterEach, expect, it, vi } from 'vitest'
import type { Config } from '../src/binary.js'
import type { HostServices, SpawnSpec, ToolDefinition } from '../src/dsh.js'
import * as plugin from '../src/index.js'

const contexts: Context[] = []
const roots: string[] = []
afterEach(async () => {
  for (const ctx of contexts.splice(0)) await ctx.fiber.dispose()
  vi.unstubAllEnvs()
  for (const root of roots.splice(0)) await rm(root, { recursive: true, force: true })
})

it.each([['freebsd', 'x64'], ['linux', 'arm64'], ['win32', 'arm64'], ['darwin', 'ia32']])(
  'rejects unsupported %s/%s before registering tools, routes or the Remote service', async (platform, arch) => {
  const ctx = new Context()
  contexts.push(ctx)
  const registrations: string[] = []
  ctx.provide('tools', { register: ({ name }: { name: string }) => { registrations.push(name); return () => {} } })
  ctx.provide('typert', { register() { registrations.push('Remote'); return () => {} } })
  ctx.provide('webServer', { register() { registrations.push('route'); return () => {} } })
  let reason = ''
  const fiber = ctx.plugin({
    name: 'unsupported-platform-acceptance',
    apply(scope: Context) {
      try {
        plugin.applyWithPlatform(scope, { binary: process.execPath }, { platform, arch })
      } catch (error) {
        reason = error instanceof Error ? error.message : String(error)
        throw error
      }
    },
  })
  await expect(Promise.resolve(fiber)).rejects.toThrow(/Unsupported/)
  expect(reason).toContain(`Unsupported uml-viewer platform: ${platform}/${arch}`)
  expect(registrations).toEqual([])
  expect(ctx.get('umlViewer')).toBeUndefined()
})

async function setup(platform = 'win32', arch = 'x64', installed = true) {
  vi.stubEnv('UML_VIEWER_BIN', undefined)
  const root = await mkdtemp(path.join(os.tmpdir(), 'uml activation '))
  roots.push(root)
  const packageName = `@dsh-uml-viewer/${platform}-${arch}`
  await writeFile(path.join(root, 'package.json'), JSON.stringify({ name: 'dsh-uml-viewer', version: '1.0.0',
    optionalDependencies: { [packageName]: '1.0.0' } }))
  if (installed) {
    const dependency = path.join(root, 'node_modules', packageName)
    await mkdir(path.join(dependency, 'bin'), { recursive: true })
    await writeFile(path.join(dependency, 'package.json'), JSON.stringify({ name: packageName, version: '1.0.0' }))
    await writeFile(path.join(dependency, 'bin', platform === 'win32' ? 'uml-viewer.exe' : 'uml-viewer'), 'packaged executable bytes')
  }
  const definitions = new Map<string, ToolDefinition>()
  const spawns: SpawnSpec[] = []
  const home = path.join(root, 'scratch-dsh-home')
  const services: HostServices = {
    tools: { register(def) { definitions.set(def.name, def); return () => { definitions.delete(def.name) } } },
    workingDirectory: { get: () => root, ensure: async () => root },
    sandboxPolicy: { resolve: () => ({ mode: 'danger-full-access', workspaceRoot: root }) },
    subprocess: { spawn(spec) {
      spawns.push(spec)
      return { collected: { stdout: { readFrom: () => ({ text: '', lossy: false }) },
        stderr: { readFrom: () => ({ text: '', lossy: false }) } },
      done: Promise.resolve({ exitCode: 0, signal: null }), terminate() {} }
    } },
    dshHomePath: (...segments) => path.join(home, ...segments),
  }
  const ctx = new Context()
  contexts.push(ctx)
  for (const [key, value] of Object.entries(services)) ctx.provide(key, value)
  const load = (config: Config = {}) => Promise.resolve(ctx.plugin({ name: 'activation-acceptance',
    apply: (scope: Context) => plugin.applyWithPlatform(scope, config, { platform, arch, root }) }))
  const call = () => definitions.get('uml_ir')!.execute({ policy: 'policy.edn' }, {
    agent: { id: 'activation-session', session: { id: 'activation-session' } }, signal: new AbortController().signal,
  })
  return { root, home, definitions, spawns, load, call }
}

it.each([['win32', 'x64', 'uml-viewer.exe'], ['linux', 'x64', 'uml-viewer'],
  ['darwin', 'arm64', 'uml-viewer'], ['darwin', 'x64', 'uml-viewer']])(
  'copies the %s/%s executable to the host plugin home before publishing tools', async (platform, arch, name) => {
  const { home, definitions, load, call, spawns } = await setup(platform, arch)
  await load()
  const binary = path.join(home, 'dsh-uml-viewer', 'versions', '1.0.0', name)
  expect(await readFile(binary, 'utf8')).toBe('packaged executable bytes')
  expect(definitions.size).toBe(5)
  expect(await call()).toMatchObject({ validResult: true })
  expect(spawns[0]!.argv[0]).toBe(binary)
})

it('refuses a packaged installation whose platform optional dependency is absent', async () => {
  const { definitions, load } = await setup('win32', 'x64', false)
  await expect(load()).rejects.toThrow(/Cannot load packaged binary @dsh-uml-viewer\/win32-x64@1.0.0.*optional dependencies enabled/s)
  expect(definitions.size).toBe(0)
})

it('pins the environment binary at activation for subsequent tool calls', async () => {
  const { root, load, call, spawns } = await setup()
  const initial = path.join(root, 'initial.exe')
  vi.stubEnv('UML_VIEWER_BIN', initial)
  await load()
  vi.stubEnv('UML_VIEWER_BIN', path.join(root, 'changed.exe'))
  expect(await call()).toMatchObject({ validResult: true })
  expect(spawns[0]!.argv[0]).toBe(initial)
})

it('pins a copy of trusted config argv and keeps it ahead of the environment', async () => {
  const { root, load, call, spawns } = await setup()
  const binary = [process.execPath, path.join(root, 'launcher.mjs')]
  const config: Config = { binary }
  await load(config)
  binary[0] = path.join(root, 'mutated.exe')
  config.binary = path.join(root, 'replacement.exe')
  vi.stubEnv('UML_VIEWER_BIN', path.join(root, 'environment.exe'))
  expect(await call()).toMatchObject({ validResult: true })
  expect(spawns[0]!.argv.slice(0, 2)).toEqual([process.execPath, path.join(root, 'launcher.mjs')])
})

it('keeps checkout local-build resolution and pins it at activation', async () => {
  const { root, load, call, spawns } = await setup(process.platform, process.arch)
  await writeFile(path.join(root, 'package.json'), JSON.stringify({ name: 'dsh-uml-viewer', version: '1.0.0', private: true }))
  const binary = path.join(root, 'target', 'debug', process.platform === 'win32' ? 'uml-viewer.exe' : 'uml-viewer')
  await mkdir(path.dirname(binary), { recursive: true })
  await writeFile(binary, 'checkout executable')
  await load()
  vi.stubEnv('UML_VIEWER_BIN', path.join(root, 'environment-changed-after-load'))
  expect(await call()).toMatchObject({ validResult: true })
  expect(spawns[0]!.argv[0]).toBe(binary)
})

it('uses an explicitly configured plugin home for the packaged executable', async () => {
  const { root, load, call, spawns } = await setup()
  const home = path.join(root, 'explicit-plugin-home')
  await load({ home })
  expect(await call()).toMatchObject({ validResult: true })
  expect(spawns[0]!.argv[0]).toBe(path.join(home, 'versions', '1.0.0', 'uml-viewer.exe'))
})

it('preserves a version already pinned by another host instead of overwriting it', async () => {
  const { home, load } = await setup()
  const directory = path.join(home, 'dsh-uml-viewer', 'versions', '1.0.0')
  await mkdir(directory, { recursive: true })
  const binary = path.join(directory, 'uml-viewer.exe')
  await writeFile(binary, 'already pinned executable')
  await load()
  expect(await readFile(binary, 'utf8')).toBe('already pinned executable')
})

it('refuses an installed platform dependency from a different version', async () => {
  const { root, load, definitions } = await setup()
  await writeFile(path.join(root, 'node_modules', '@dsh-uml-viewer', 'win32-x64', 'package.json'),
    JSON.stringify({ name: '@dsh-uml-viewer/win32-x64', version: '2.0.0' }))
  await expect(load()).rejects.toThrow(/installed version is 2.0.0/)
  expect(definitions.size).toBe(0)
})

it('reports a missing packaged manifest dependency before exposing tools', async () => {
  const { root, load, definitions } = await setup()
  await writeFile(path.join(root, 'package.json'), JSON.stringify({ name: 'dsh-uml-viewer', version: '1.0.0' }))
  await expect(load()).rejects.toThrow(/missing the exact @dsh-uml-viewer\/win32-x64@1.0.0 optional dependency declaration/)
  expect(definitions.size).toBe(0)
})
