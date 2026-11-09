import { mkdir, mkdtemp, writeFile, rm } from 'node:fs/promises'
import { createHash } from 'node:crypto'
import os from 'node:os'
import path from 'node:path'
import { expect, it } from 'vitest'
import type { SessionState } from '@dsh-uml-viewer/contracts'
import { parseResidentLine } from '@dsh-uml-viewer/contracts'
import { SessionStates } from '../src/state.js'

const sha256 = (text: string): string => createHash('sha256').update(text).digest('hex')

it('restores only the last displayed IR bytes; changed or missing IR reopens waiting', async () => {
  const home = await mkdtemp(path.join(os.tmpdir(), 'uml restore digest '))
  try {
    const diagram = path.join(home, 'diagram.edn')
    const displayed = '{:title "Previously displayed"}'
    await writeFile(diagram, displayed)
    const state: SessionState = {
      schema: 'dsh-uml-viewer/session-state', version: 1, diagram,
      loadedDiagram: { version: 1, sha256: sha256(displayed) },
      context: null, focus: [':core'], proposalId: null, openLayer: null, declutter: null,
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

it('saves the resident-loaded fingerprint, so a file replaced after the load is not trusted', async () => {
  // The resident parsed A; the host saves while B is already on disk (the
  // panel can close inside the 250 ms reload cadence). The stored state must
  // still describe A, so reopening over B fails closed.
  const home = await mkdtemp(path.join(os.tmpdir(), 'uml save fingerprint '))
  try {
    const diagram = path.join(home, 'diagram.edn')
    const loaded = '{:title "A"}'
    const residentState: SessionState = {
      schema: 'dsh-uml-viewer/session-state', version: 1, diagram,
      loadedDiagram: { version: 1, sha256: sha256(loaded) },
      context: null, focus: [], proposalId: null, openLayer: null, declutter: null,
      camera: { x: 0, y: 0, zoom: 1 }, selected: null, detail: null,
    }
    await writeFile(diagram, '{:title "B"}')
    // A fake resident emits the protocol response to save; validate and persist
    // exactly what crosses that boundary, including its loaded fingerprint.
    const event = parseResidentLine(JSON.stringify({ version: 1, seq: 9, event: 'state', state: residentState }))
    if (event.event !== 'state') throw new Error('expected state response')
    await new SessionStates(home).write('session-b', event.state)
    expect(await new SessionStates(home).read('session-b')).toEqual({ ...residentState, diagram: null })
  } finally {
    await rm(home, { recursive: true, force: true })
  }
})

it('reopens waiting for a state saved by a resident without a loaded fingerprint', async () => {
  const home = await mkdtemp(path.join(os.tmpdir(), 'uml legacy state '))
  try {
    const diagram = path.join(home, 'diagram.edn')
    const displayed = '{:title "Legacy"}'
    await writeFile(diagram, displayed)
    const legacy = {
      schema: 'dsh-uml-viewer/session-state', version: 1, diagram,
      diagramDigest: sha256(displayed),
      context: null, focus: [], proposalId: null, openLayer: null, declutter: null,
      camera: { x: 0, y: 0, zoom: 1 }, selected: null, detail: null,
    }
    const states = new SessionStates(home)
    await mkdir(path.join(home, 'sessions'), { recursive: true })
    await writeFile(path.join(home, 'sessions', 'legacy.json'), `${JSON.stringify(legacy)}\n`)
    expect(await states.read('legacy')).toEqual({
      schema: 'dsh-uml-viewer/session-state', version: 1, diagram: null,
      context: null, focus: [], proposalId: null, openLayer: null, declutter: null,
      camera: { x: 0, y: 0, zoom: 1 }, selected: null, detail: null,
    })
  } finally {
    await rm(home, { recursive: true, force: true })
  }
})
