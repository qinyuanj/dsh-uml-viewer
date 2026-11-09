import { mkdir, mkdtemp, rm, writeFile } from 'node:fs/promises'
import os from 'node:os'
import path from 'node:path'
import { afterEach, expect, it } from 'vitest'
import { packageRoot } from '../src/binary.js'

const roots: string[] = []
afterEach(async () => {
  for (const root of roots.splice(0)) await rm(root, { recursive: true, force: true })
})

async function tree(files: Record<string, unknown>): Promise<string> {
  const root = await mkdtemp(path.join(os.tmpdir(), 'uml package root '))
  roots.push(root)
  for (const [file, content] of Object.entries(files)) {
    await mkdir(path.dirname(path.join(root, file)), { recursive: true })
    await writeFile(path.join(root, file), typeof content === 'string' ? content : JSON.stringify(content))
  }
  return root
}

// CI14: the scratch DSH home lived inside a dsh-uml-viewer checkout, so a
// walk past the installed package's own directory found the checkout's
// manifest and activated its development build instead of the install.
const checkout = { name: 'dsh-uml-viewer', private: true, main: 'lib/host.js' }
const installed = 'run/profiles/web/node_modules/dsh-uml-viewer'

it('resolves an installed module to its own package directory', async () => {
  const root = await tree({
    'package.json': checkout,
    [`${installed}/package.json`]: { name: 'dsh-uml-viewer', version: '1.0.1', main: 'lib/host.js' },
    [`${installed}/lib/host.js`]: '',
  })
  expect(packageRoot(path.join(root, installed, 'lib', 'host.js'))).toBe(path.join(root, installed))
})

it('never adopts a dsh-uml-viewer manifest above the node_modules that holds the module', async () => {
  // The installed copy's manifest is absent (mid-install, or removed): the
  // checkout above the profile is not this module's package.
  const root = await tree({ 'package.json': checkout, [`${installed}/lib/host.js`]: '' })
  expect(() => packageRoot(path.join(root, installed, 'lib', 'host.js'))).toThrow(/Cannot locate the dsh-uml-viewer package root/)
})

it('resolves a checkout module through its workspace package to the checkout root', async () => {
  const root = await tree({
    'package.json': checkout,
    'packages/host/package.json': { name: '@dsh-uml-viewer/host', private: true },
    'packages/host/src/binary.ts': '',
  })
  expect(packageRoot(path.join(root, 'packages', 'host', 'src', 'binary.ts'))).toBe(root)
})
