import { test } from 'node:test'
import assert from 'node:assert/strict'
import fs from 'node:fs/promises'
import os from 'node:os'
import path from 'node:path'
import { installEvidence } from './upgrade-evidence.mjs'

async function write(file, text) {
  await fs.mkdir(path.dirname(file), { recursive: true })
  await fs.writeFile(file, text)
}
const manifest = (name, version) => JSON.stringify({ name, version })

test('installEvidence records every pinned version store, the profile links and the virtual store copies', async () => {
  const home = await fs.mkdtemp(path.join(os.tmpdir(), 'upgrade-evidence-'))
  try {
    await write(path.join(home, 'plugin-state', 'current.json'), '{"version":"1.0.0","size":1,"sha256":"a"}\n')
    await fs.mkdir(path.join(home, 'plugin-state', '1.0.0'))
    await fs.mkdir(path.join(home, 'plugin-state', 'leases'))
    // A default-home activation (config without `home`) would land here.
    await write(path.join(home, 'dsh-uml-viewer', 'current.json'), '{"version":"1.0.1"}\n')
    await fs.mkdir(path.join(home, 'dsh-uml-viewer', '1.0.1'))
    const modules = path.join(home, 'profiles', 'web', 'node_modules')
    const old = path.join(modules, '.pnpm', 'dsh-uml-viewer@file+old', 'node_modules', 'dsh-uml-viewer')
    const next = path.join(modules, '.pnpm', 'dsh-uml-viewer@file+new', 'node_modules', 'dsh-uml-viewer')
    const platform = path.join(modules, '.pnpm', '@dsh-uml-viewer+win32-x64@file+new', 'node_modules', '@dsh-uml-viewer', 'win32-x64')
    await write(path.join(old, 'package.json'), manifest('dsh-uml-viewer', '1.0.0'))
    await write(path.join(next, 'package.json'), manifest('dsh-uml-viewer', '1.0.1'))
    await write(path.join(platform, 'package.json'), manifest('@dsh-uml-viewer/win32-x64', '1.0.1'))
    await write(path.join(modules, '.pnpm', 'unrelated@1.0.0', 'node_modules', 'unrelated', 'package.json'), manifest('unrelated', '1.0.0'))
    await fs.symlink(next, path.join(modules, 'dsh-uml-viewer'), 'junction')
    await fs.mkdir(path.join(modules, '@dsh-uml-viewer'))
    await fs.symlink(platform, path.join(modules, '@dsh-uml-viewer', 'win32-x64'), 'junction')
    await write(path.join(home, 'cache', 'deep', 'current.json'), '{"version":"9.9.9"}\n')

    const evidence = await installEvidence(home, { platform: '@dsh-uml-viewer/win32-x64' })

    assert.deepEqual(evidence.stores, [
      { dir: 'dsh-uml-viewer', current: '1.0.1', versions: ['1.0.1'] },
      { dir: 'plugin-state', current: '1.0.0', versions: ['1.0.0'] },
    ])
    assert.deepEqual(evidence.links, [
      { name: 'dsh-uml-viewer', version: '1.0.1', target: path.relative(home, await fs.realpath(next)) },
      { name: '@dsh-uml-viewer/win32-x64', version: '1.0.1', target: path.relative(home, await fs.realpath(platform)) },
    ])
    assert.deepEqual(evidence.virtualStore, [
      { entry: '@dsh-uml-viewer+win32-x64@file+new', version: '1.0.1' },
      { entry: 'dsh-uml-viewer@file+new', version: '1.0.1' },
      { entry: 'dsh-uml-viewer@file+old', version: '1.0.0' },
    ])
  } finally {
    await fs.rm(home, { recursive: true, force: true })
  }
})

test('installEvidence reports absent links and stores instead of throwing', async () => {
  const home = await fs.mkdtemp(path.join(os.tmpdir(), 'upgrade-evidence-'))
  try {
    const evidence = await installEvidence(home, { platform: '@dsh-uml-viewer/win32-x64' })
    assert.deepEqual(evidence.stores, [])
    assert.deepEqual(evidence.virtualStore, [])
    assert.deepEqual(evidence.links.map(link => [link.name, link.error]), [
      ['dsh-uml-viewer', 'ENOENT'], ['@dsh-uml-viewer/win32-x64', 'ENOENT'],
    ])
  } finally {
    await fs.rm(home, { recursive: true, force: true })
  }
})
