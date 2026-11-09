import { mkdtemp, writeFile, rm } from 'node:fs/promises'
import os from 'node:os'
import path from 'node:path'
import { expect, it } from 'vitest'
import type { SessionState } from '@dsh-uml-viewer/contracts'
import { SessionStates } from '../src/state.js'

it('restores only the last displayed IR bytes; changed or missing IR reopens waiting', async () => {
  const home = await mkdtemp(path.join(os.tmpdir(), 'uml restore digest '))
  try {
    const diagram = path.join(home, 'diagram.edn')
    await writeFile(diagram, '{:title "Previously displayed"}')
    const state: SessionState = {
      schema: 'dsh-uml-viewer/session-state', version: 1, diagram, context: null,
      focus: [':core'], proposalId: null, openLayer: null, declutter: null,
      camera: { x: 12, y: 34, zoom: 0.6 }, selected: null, detail: null,
    }
    const states = new SessionStates(home)
    await states.write('session-a', state)
    // A new store instance models a DSH process restart.
    expect(await new SessionStates(home).read('session-a')).toEqual(state)
    await writeFile(diagram, '{:title "Not yet displayed"}')
    expect(await new SessionStates(home).read('session-a')).toEqual({ ...state, diagram: null })
    await rm(diagram)
    expect(await new SessionStates(home).read('session-a')).toEqual({ ...state, diagram: null })
  } finally {
    await rm(home, { recursive: true, force: true })
  }
})
