import { existsSync } from 'node:fs'
import { mkdir, mkdtemp, readdir, readFile, rm, utimes, writeFile } from 'node:fs/promises'
import os from 'node:os'
import path from 'node:path'
import { afterEach, expect, it } from 'vitest'
import { cleanupVersions, pinVersion } from '../src/versions.js'

const roots: string[] = []
const releases: (() => void)[] = []
afterEach(async () => {
  for (const release of releases.splice(0)) release()
  for (const root of roots.splice(0)) await rm(root, { recursive: true, force: true })
})

async function fixture() {
  const root = await mkdtemp(path.join(os.tmpdir(), 'uml version store '))
  roots.push(root)
  const source = path.join(root, 'resident.cjs')
  await writeFile(source, "console.log('v1')\n")
  const home = path.join(root, 'dsh-home', 'dsh-uml-viewer')
  const pin = (version: string) => {
    const pinned = pinVersion(home, version, source, 'resident.cjs')
    releases.push(pinned.release)
    return pinned
  }
  return { root, home, source, pin }
}

it('retains a retired activation while a child holds its version, and cleans up on actual release', async () => {
  const { home, pin } = await fixture()
  const old = pin('1.0.0')
  const childExit = old.retain()
  releases.push(childExit)
  pin('1.1.0')
  old.release()
  cleanupVersions(home)
  expect(existsSync(path.join(home, '1.0.0', 'resident.cjs'))).toBe(true)
  childExit()
  expect(existsSync(path.join(home, '1.0.0'))).toBe(false)
  expect(existsSync(path.join(home, '1.1.0', 'resident.cjs'))).toBe(true)
  // Disposers are single-shot; an ended activation cannot start a new child.
  childExit()
  old.release()
  expect(() => old.retain()).toThrow(/activation has ended/)
})

it('keeps a live owner even when its heartbeat is old (e.g. a paused DSH host)', async () => {
  const { home, pin } = await fixture()
  const old = pin('1.0.0')
  const directory = path.join(home, 'leases', '1.0.0')
  const hourAgo = new Date(Date.now() - 60 * 60_000)
  for (const lease of await readdir(directory)) await utimes(path.join(directory, lease), hourAgo, hourAgo)
  pin('1.1.0')
  cleanupVersions(home)
  expect(existsSync(path.join(home, '1.0.0', 'resident.cjs'))).toBe(true)
  old.release()
  expect(existsSync(path.join(home, '1.0.0'))).toBe(false)
})

it('uses the hash to reject same-size corrupted bytes in a leased version', async () => {
  const { home, source, pin } = await fixture()
  const original = pin('1.0.0')
  const expected = await readFile(source, 'utf8')
  await writeFile(source, expected.replace('v1', 'v2'))
  expect(() => pin('1.0.0')).toThrow(/different bytes and is in use/)
  expect(await readFile(original.binary, 'utf8')).toBe("console.log('v1')\n")
  expect(await readdir(path.join(home, 'leases', '1.0.0'))).toHaveLength(1)
  expect((await readdir(home)).filter(name => name.startsWith('.'))).toEqual([])
})

it('replaces a same-size corrupted copy only after its owner releases it', async () => {
  const { source, pin } = await fixture()
  const old = pin('1.0.0')
  old.release()
  await writeFile(source, "console.log('v2')\n")
  const corrected = pin('1.0.0')
  expect(await readFile(corrected.binary, 'utf8')).toBe("console.log('v2')\n")
})

it.each(['../escape', '1/2/3', 'C:\\escape', '', '1.0.0/../2.0.0'])('refuses a non-version directory %j before any store write', async version => {
  const { home, source } = await fixture()
  expect(() => pinVersion(home, version, source, 'resident.cjs')).toThrow(/Invalid/)
  expect(existsSync(home)).toBe(false)
})

it('leaves unknown directories and the pre-lease layout alone', async () => {
  const { home, pin } = await fixture()
  await mkdir(path.join(home, 'versions', '0.9.0'), { recursive: true })
  await writeFile(path.join(home, 'versions', '0.9.0', 'resident.cjs'), 'legacy payload')
  pin('1.0.0')
  expect(await readFile(path.join(home, 'versions', '0.9.0', 'resident.cjs'), 'utf8')).toBe('legacy payload')
})
