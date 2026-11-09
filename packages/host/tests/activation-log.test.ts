import { mkdtemp, readFile, rm, writeFile } from 'node:fs/promises'
import os from 'node:os'
import path from 'node:path'
import { afterEach, expect, it } from 'vitest'
import { ACTIVATION_LOG, ACTIVATION_LOG_LIMIT, recordActivation } from '../src/activation.js'
import type { ActivationRecord } from '../src/activation.js'

const roots: string[] = []
afterEach(async () => {
  for (const root of roots.splice(0)) await rm(root, { recursive: true, force: true })
})

async function directory(): Promise<string> {
  const root = await mkdtemp(path.join(os.tmpdir(), 'uml activation log '))
  roots.push(root)
  return root
}

const pinned: ActivationRecord = {
  version: '1.0.0', source: 'pinned', root: 'C:\\plugin', argv: ['C:\\plugin\\uml-viewer.exe'],
}

async function lines(home: string): Promise<Record<string, unknown>[]> {
  const text = await readFile(path.join(home, ACTIVATION_LOG), 'utf8')
  return text.trim().split('\n').map(line => JSON.parse(line) as Record<string, unknown>)
}

it('appends one line per activation, naming the version and how it was chosen', async () => {
  const home = await directory()
  recordActivation(home, pinned, new Date('2026-11-09T15:18:12.000Z'))
  recordActivation(home, { version: null, source: 'unpinned', root: 'D:\\checkout', argv: ['D:\\checkout\\uml-viewer.exe'] },
    new Date('2026-11-09T15:18:20.000Z'))
  expect(await lines(home)).toEqual([
    { at: '2026-11-09T15:18:12.000Z', pid: process.pid, ...pinned },
    { at: '2026-11-09T15:18:20.000Z', pid: process.pid, version: null, source: 'unpinned', root: 'D:\\checkout',
      argv: ['D:\\checkout\\uml-viewer.exe'] },
  ])
})

it('keeps only the newest activations', async () => {
  const home = await directory()
  for (let index = 0; index < ACTIVATION_LOG_LIMIT + 5; index += 1) {
    recordActivation(home, { ...pinned, version: `1.0.${index}` }, new Date(Date.UTC(2026, 0, 1, 0, 0, index)))
  }
  const kept = await lines(home)
  expect(kept).toHaveLength(ACTIVATION_LOG_LIMIT)
  expect(kept[0]!.version).toBe('1.0.5')
  expect(kept.at(-1)!.version).toBe(`1.0.${ACTIVATION_LOG_LIMIT + 4}`)
})

it('never throws and never creates a home it cannot write', async () => {
  expect(() => recordActivation(undefined, pinned)).not.toThrow()
  const root = await directory()
  const blocker = path.join(root, 'blocker')
  await writeFile(blocker, 'not a directory')
  expect(() => recordActivation(path.join(blocker, 'home'), pinned)).not.toThrow()
})
