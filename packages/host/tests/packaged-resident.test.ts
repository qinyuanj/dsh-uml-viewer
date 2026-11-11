import { mkdir, mkdtemp, readFile, rm, writeFile } from 'node:fs/promises'
import os from 'node:os'
import path from 'node:path'
import { afterEach, expect, it, vi } from 'vitest'
import { activationBinary } from '../src/binary.js'

const roots: string[] = []
afterEach(async () => {
  vi.unstubAllEnvs()
  for (const root of roots.splice(0)) await rm(root, { recursive: true, force: true })
})

it('pins the profile’s explicitly installed platform payload instead of a nested registry copy', async () => {
  vi.stubEnv('UML_VIEWER_BIN', undefined)
  const base = await mkdtemp(path.join(os.tmpdir(), 'uml packaged resident '))
  roots.push(base)
  const target = `${process.platform}-${process.arch}`
  const dependency = `@dsh-uml-viewer/${target}`
  const executable = process.platform === 'win32' ? 'uml-viewer.exe' : 'uml-viewer'
  const profileModules = path.join(base, 'profiles', 'web', 'node_modules')
  const root = path.join(profileModules, 'dsh-uml-viewer')
  await mkdir(root, { recursive: true })
  await writeFile(path.join(root, 'package.json'), JSON.stringify({
    name: 'dsh-uml-viewer', version: '1.0.0', optionalDependencies: { [dependency]: '1.0.0' },
  }))
  async function platformPackage(parent: string, payload: string): Promise<void> {
    const directory = path.join(parent, dependency)
    await mkdir(path.join(directory, 'bin'), { recursive: true })
    await writeFile(path.join(directory, 'package.json'), JSON.stringify({ name: dependency, version: '1.0.0' }))
    await writeFile(path.join(directory, 'bin', executable), payload)
  }
  // pnpm keeps the explicitly supplied tarball at the profile's top level, but
  // also installs the now-published semver optional dependency below the main
  // package. Node resolution from the main package sees the latter first.
  await platformPackage(profileModules, 'explicit local platform payload')
  await platformPackage(path.join(root, 'node_modules'), 'published registry platform payload')

  const activated = activationBinary({}, path.join(base, 'plugin-state'), {
    platform: process.platform, arch: process.arch, root,
  })
  expect(await readFile(activated.argv[0]!, 'utf8')).toBe('explicit local platform payload')
  activated.release()
})
