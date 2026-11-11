import { mkdir, mkdtemp, rm, writeFile } from 'node:fs/promises'
import os from 'node:os'
import path from 'node:path'
import { afterEach, beforeEach, expect, it, vi } from 'vitest'
import { binaryArgv } from '../src/binary.js'

const roots: string[] = []
// The e2e lane exports UML_VIEWER_BIN; these locator cases must not read that
// ambient override (F47). Each test stubs/unstubs its own value from here.
beforeEach(() => { vi.stubEnv('UML_VIEWER_BIN', undefined) })
afterEach(async () => {
  vi.unstubAllEnvs()
  for (const root of roots.splice(0)) await rm(root, { recursive: true, force: true })
})

async function packageRoot(profiles: string[] = []): Promise<string> {
  const root = await mkdtemp(path.join(os.tmpdir(), 'uml binary locator '))
  roots.push(root)
  const name = process.platform === 'win32' ? 'uml-viewer.exe' : 'uml-viewer'
  for (const profile of profiles) {
    const binary = path.join(root, 'target', profile, name)
    await mkdir(path.dirname(binary), { recursive: true })
    await writeFile(binary, '')
  }
  return root
}

it('prefers trusted config over the environment and local build artifacts', async () => {
  const root = await packageRoot(['debug', 'release'])
  const environment = path.join(root, 'environment-uml-viewer')
  vi.stubEnv('UML_VIEWER_BIN', environment)
  expect(binaryArgv({ binary: [process.execPath, 'fixture'] }, root)).toEqual([process.execPath, 'fixture'])
  expect(binaryArgv({}, root)).toEqual([environment])
})

it('prefers a debug binary when both local profiles exist', async () => {
  const root = await packageRoot(['debug', 'release'])
  const name = process.platform === 'win32' ? 'uml-viewer.exe' : 'uml-viewer'
  expect(binaryArgv({}, root)).toEqual([path.join(root, 'target', 'debug', name)])
})

it('falls back to a release binary when debug is absent', async () => {
  const root = await packageRoot(['release'])
  const name = process.platform === 'win32' ? 'uml-viewer.exe' : 'uml-viewer'
  expect(binaryArgv({}, root)).toEqual([path.join(root, 'target', 'release', name)])
})

it('fails with an actionable reason when no binary exists', async () => {
  const root = await packageRoot()
  expect(() => binaryArgv({}, root)).toThrow(/No uml-viewer binary.*UML_VIEWER_BIN.*absolute/)
})

it('rejects a relative environment executable instead of using PATH', async () => {
  vi.stubEnv('UML_VIEWER_BIN', 'uml-viewer')
  expect(() => binaryArgv({}, '')).toThrow(/absolute executable/)
})
