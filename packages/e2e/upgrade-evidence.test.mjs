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
const manifest = (name, version, extra = {}) => JSON.stringify({ name, version, ...extra })
const BINARY = process.platform === 'win32' ? 'uml-viewer.exe' : 'uml-viewer'

test('installEvidence records every pinned version store, its leases and activations, the profile links and the virtual store copies', async () => {
  const home = await fs.mkdtemp(path.join(os.tmpdir(), 'upgrade-evidence-'))
  try {
    const store = path.join(home, 'plugin-state')
    await write(path.join(store, 'current.json'), '{"version":"1.0.0","size":1,"sha256":"a"}\n')
    await fs.mkdir(path.join(store, '1.0.0'))
    await write(path.join(store, 'leases', '1.0.0', '5944-018985d4.lease'), '{"pid":5944,"version":"1.0.0"}\n')
    await write(path.join(store, 'leases', '1.0.1', '5944-aa11bb22.lease'), '{"pid":5944,"version":"1.0.1"}\n')
    await write(path.join(store, 'activations.jsonl'), [
      '{"at":"2026-11-09T15:17:52.000Z","pid":5944,"version":"1.0.0","source":"pinned","root":"C:\\\\web\\\\dsh-uml-viewer","argv":["C:\\\\state\\\\1.0.0\\\\uml-viewer.exe"]}',
      'not json',
      '{"at":"2026-11-09T15:18:12.000Z","pid":5944,"version":"1.0.1","source":"pinned","root":"C:\\\\web\\\\dsh-uml-viewer","argv":["C:\\\\state\\\\1.0.1\\\\uml-viewer.exe"]}',
      '',
    ].join('\n'))
    // A default-home activation (config without `home`) would land here.
    await write(path.join(home, 'dsh-uml-viewer', 'current.json'), '{"version":"1.0.1"}\n')
    await fs.mkdir(path.join(home, 'dsh-uml-viewer', '1.0.1'))
    const modules = path.join(home, 'profiles', 'web', 'node_modules')
    const old = path.join(modules, '.pnpm', 'dsh-uml-viewer@file+old', 'node_modules', 'dsh-uml-viewer')
    const next = path.join(modules, '.pnpm', 'dsh-uml-viewer@file+new', 'node_modules', 'dsh-uml-viewer')
    const platform = path.join(modules, '.pnpm', '@dsh-uml-viewer+win32-x64@file+new', 'node_modules', '@dsh-uml-viewer', 'win32-x64')
    await write(path.join(old, 'package.json'), manifest('dsh-uml-viewer', '1.0.0'))
    await write(path.join(next, 'package.json'), manifest('dsh-uml-viewer', '1.0.1', {
      main: 'lib/host.js', private: true, optionalDependencies: { '@dsh-uml-viewer/win32-x64': '1.0.1' },
    }))
    await fs.mkdir(path.join(next, 'target', 'debug'), { recursive: true })
    await write(path.join(next, 'target', 'debug', BINARY), 'checkout executable')
    await write(path.join(platform, 'package.json'), manifest('@dsh-uml-viewer/win32-x64', '1.0.1'))
    await write(path.join(modules, '.pnpm', 'unrelated@1.0.0', 'node_modules', 'unrelated', 'package.json'), manifest('unrelated', '1.0.0'))
    await fs.symlink(next, path.join(modules, 'dsh-uml-viewer'), 'junction')
    await fs.mkdir(path.join(modules, '@dsh-uml-viewer'))
    await fs.symlink(platform, path.join(modules, '@dsh-uml-viewer', 'win32-x64'), 'junction')
    await write(path.join(home, 'cache', 'deep', 'current.json'), '{"version":"9.9.9"}\n')

    const evidence = await installEvidence(home, { platform: '@dsh-uml-viewer/win32-x64' })

    assert.deepEqual(evidence.stores, [
      {
        dir: 'dsh-uml-viewer', current: '1.0.1', versions: ['1.0.1'], leases: {}, activations: [],
      },
      {
        dir: 'plugin-state',
        current: '1.0.0',
        versions: ['1.0.0'],
        leases: { '1.0.0': ['5944-018985d4.lease'], '1.0.1': ['5944-aa11bb22.lease'] },
        activations: [
          {
            at: '2026-11-09T15:17:52.000Z', pid: 5944, version: '1.0.0', source: 'pinned',
            root: 'C:\\web\\dsh-uml-viewer', argv: ['C:\\state\\1.0.0\\uml-viewer.exe'],
          },
          { unreadable: 'not json' },
          {
            at: '2026-11-09T15:18:12.000Z', pid: 5944, version: '1.0.1', source: 'pinned',
            root: 'C:\\web\\dsh-uml-viewer', argv: ['C:\\state\\1.0.1\\uml-viewer.exe'],
          },
        ],
      },
    ])
    assert.deepEqual(evidence.links, [
      { name: 'dsh-uml-viewer', version: '1.0.1', target: path.relative(home, await fs.realpath(next)) },
      { name: '@dsh-uml-viewer/win32-x64', version: '1.0.1', target: path.relative(home, await fs.realpath(platform)) },
    ])
    // What the activation would have read from disk, and which build it would
    // fall back to if the installed manifest were a private checkout.
    assert.deepEqual(evidence.main, {
      version: '1.0.1',
      private: true,
      main: 'lib/host.js',
      optionalDependencies: { '@dsh-uml-viewer/win32-x64': '1.0.1' },
      devBinaries: ['debug'],
    })
    assert.deepEqual(evidence.virtualStore, [
      { entry: '@dsh-uml-viewer+win32-x64@file+new', version: '1.0.1' },
      { entry: 'dsh-uml-viewer@file+new', version: '1.0.1' },
      { entry: 'dsh-uml-viewer@file+old', version: '1.0.0' },
    ])
  } finally {
    await fs.rm(home, { recursive: true, force: true })
  }
})

test('installEvidence reports absent links, stores and activation records instead of throwing', async () => {
  const home = await fs.mkdtemp(path.join(os.tmpdir(), 'upgrade-evidence-'))
  try {
    const evidence = await installEvidence(home, { platform: '@dsh-uml-viewer/win32-x64' })
    assert.deepEqual(evidence.stores, [])
    assert.deepEqual(evidence.virtualStore, [])
    assert.deepEqual(evidence.links.map(link => [link.name, link.error]), [
      ['dsh-uml-viewer', 'ENOENT'], ['@dsh-uml-viewer/win32-x64', 'ENOENT'],
    ])
    assert.deepEqual(evidence.main, { error: 'ENOENT' })
  } finally {
    await fs.rm(home, { recursive: true, force: true })
  }
})

test('installEvidence keeps the newest activation records and leases one level only', async () => {
  const home = await fs.mkdtemp(path.join(os.tmpdir(), 'upgrade-evidence-'))
  try {
    const store = path.join(home, 'plugin-state')
    await write(path.join(store, 'current.json'), '{"version":"2.0.0"}\n')
    await fs.mkdir(path.join(store, '2.0.0'))
    const lines = Array.from({ length: 40 }, (_, index) => JSON.stringify({
      at: `2026-11-09T15:18:${String(index).padStart(2, '0')}.000Z`, pid: 1, version: '2.0.0', source: 'pinned',
      root: '/web', argv: ['/state/2.0.0/uml-viewer'],
    }))
    await write(path.join(store, 'activations.jsonl'), `${lines.join('\n')}\n`)
    // A lease directory without files is a version that leased nothing.
    await fs.mkdir(path.join(store, 'leases', '1.0.0'), { recursive: true })

    const evidence = await installEvidence(home, { platform: '@dsh-uml-viewer/win32-x64' })
    assert.deepEqual(evidence.stores, [{
      dir: 'plugin-state', current: '2.0.0', versions: ['2.0.0'], leases: { '1.0.0': [] },
      activations: lines.slice(-20).map(line => JSON.parse(line)),
    }])
  } finally {
    await fs.rm(home, { recursive: true, force: true })
  }
})
