import { readFileSync, readdirSync } from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'
import { describe, expect, it } from 'vitest'
import {
  RESIDENT_SCHEMA, RESIDENT_VERSION, SESSION_STATE_SCHEMA, parseResidentLine, residentAction,
  validateResidentEvent, validateSessionState,
} from '../src/index.js'

const frames = path.resolve(fileURLToPath(new URL('../../../crates/frame/tests/fixtures/frames', import.meta.url)))
const goldenFiles = readdirSync(frames).filter(name => name.endsWith('.frame.json')).sort()

function golden(name: string): unknown {
  return JSON.parse(readFileSync(path.join(frames, name), 'utf8'))
}

describe('resident protocol', () => {
  it('accepts a frame event wrapping every producer golden', () => {
    expect(goldenFiles).toHaveLength(14) // seven views, Chinese and English (#44)
    for (const name of goldenFiles) {
      const line = JSON.stringify({ version: 1, seq: 3, event: 'frame', frame: golden(name) })
      const event = parseResidentLine(line)
      expect(event.event, name).toBe('frame')
      expect(event.seq).toBe(3)
      if (event.event !== 'frame') return
      expect(event.frame.schema).toBe('dsh-uml-viewer/scene-frame')
      // The frame travels unchanged: the resident contract never reshapes the producer's document.
      expect(event.frame).toEqual(golden(name))
    }
  })

  it('accepts the documented ready, ack, error, search, context and state lines', () => {
    const lines = [
      { version: 1, seq: null, event: 'ready', schema: RESIDENT_SCHEMA, frameSchema: 'dsh-uml-viewer/scene-frame',
        frameVersion: 1, stateSchema: SESSION_STATE_SCHEMA, stateVersion: 1,
        actions: ['open', 'viewport', 'display', 'reload', 'back', 'namespace-tree', 'search', 'save', 'quit'] },
      { version: 1, seq: 4, event: 'ack' },
      { version: 1, seq: 5, event: 'error', message: 'unknown action "jump"' },
      { version: 1, seq: 6, event: 'search', query: 'graph', results: [{ id: ':ir.graph', name: 'Graph', ns: 'ir', label: 'ir' }] },
      { version: 1, seq: 7, event: 'context', context: { kind: 'proposal', proposalId: ':p-shop', name: '2026-10-24 10:00:00',
        edn: '{:context :proposal :name "2026-10-24 10:00:00" :proposal-id :p-shop}' } },
      { version: 1, seq: 8, event: 'state', state: {
        schema: SESSION_STATE_SCHEMA, version: 1, diagram: 'D:/project/diagram.edn', context: { kind: 'real', edn: '{:context :real}' },
        focus: [':core'], proposalId: null, openLayer: null, declutter: null,
        camera: { x: 0, y: 0, zoom: 1 }, selected: null, detail: null } },
      { version: 1, seq: null, event: 'error', message: 'not an object' },
    ]
    for (const line of lines) expect(validateResidentEvent(line), JSON.stringify(line).slice(0, 40)).toEqual(line)
  })

  it('accepts the quality-target answer and refuses a malformed target', () => {
    // #28: the answer the host validates before it queues a panel request.
    const line = { version: 1, seq: 17, event: 'quality-target',
      target: { id: ':core', kind: 'component', files: ['src/core/a.rs', 'src/core/b.rs'] } }
    expect(parseResidentLine(JSON.stringify(line))).toEqual(line)
    for (const target of [
      { id: 'core', kind: 'component', files: ['src/core/a.rs'] },
      { id: ':core', kind: 'layer', files: ['src/core/a.rs'] },
      { id: ':core', kind: 'component', files: [] },
      { id: ':core', kind: 'component', files: ['src/core/b.rs', 'src/core/a.rs'] },
      { id: ':core', kind: 'component', files: ['../outside.ts'] },
    ]) {
      expect(() => validateResidentEvent({ ...line, target }), JSON.stringify(target)).toThrow(/target/)
    }
  })

  it('validates the persisted session state the panel restores', () => {
    const state = { schema: SESSION_STATE_SCHEMA, version: 1, diagram: null,
      context: { kind: 'real', edn: '{:context :real}' }, focus: [], proposalId: null, openLayer: null,
      declutter: ':triangles', camera: { x: 1, y: 2, zoom: 0.5 }, selected: null, detail: ':ir.graph' }
    expect(validateSessionState(state)).toEqual(state)
    expect(() => validateSessionState({ ...state, schema: 'dsh-uml-viewer/other' })).toThrow(/schema/)
    expect(() => validateSessionState({ ...state, version: 2 })).toThrow(/version/)
    expect(() => validateSessionState({ ...state, focus: ':core' })).toThrow(/focus/)
  })

  it('carries versioned loaded-byte fingerprints through the state event, while accepting legacy state', () => {
    const state = { schema: SESSION_STATE_SCHEMA, version: 1, diagram: '/project/diagram.edn',
      loadedDiagram: { version: 1, sha256: 'a'.repeat(64) }, context: null, focus: [],
      proposalId: null, openLayer: null, declutter: null, camera: { x: 2, y: 3, zoom: 1 }, selected: null, detail: null }
    const event = { version: 1, seq: 12, event: 'state', state }
    expect(parseResidentLine(JSON.stringify(event))).toEqual(event)
    expect(validateSessionState({ ...state, loadedDiagram: null }).loadedDiagram).toBeNull()
    for (const loadedDiagram of [{ version: 2, sha256: 'a'.repeat(64) }, { version: 1, sha256: 'a'.repeat(63) },
      { version: 1, sha256: 'A'.repeat(64) }, { version: 1, sha256: null }]) {
      expect(() => validateSessionState({ ...state, loadedDiagram })).toThrow(/loadedDiagram/)
    }
    const { loadedDiagram: _fingerprint, ...legacy } = state
    expect(validateSessionState(legacy)).toEqual(legacy)
  })

  const frame = { version: 1, seq: 1, event: 'frame', frame: golden('root.frame.json') }
  const state = { version: 1, seq: 1, event: 'state', state: { schema: SESSION_STATE_SCHEMA, version: 1, diagram: null,
    context: null, focus: [], proposalId: null, openLayer: null, declutter: null, camera: { x: 0, y: 0, zoom: 1 },
    selected: null, detail: null } }
  const search = { version: 1, seq: 1, event: 'search', query: 'a', results: [] }
  it.each([
    ['another protocol version', frame, (line: Record<string, unknown>) => { line.version = 2 }, /version/],
    ['no event name', frame, (line: Record<string, unknown>) => { delete line.event }, /event/],
    ['an unknown event', frame, (line: Record<string, unknown>) => { line.event = 'paint' }, /event/],
    ['a frame event without a frame', frame, (line: Record<string, unknown>) => { delete line.frame }, /frame/],
    ['a frame event whose frame breaks the scene contract', frame, (line: Record<string, unknown>) => {
      (line.frame as Record<string, unknown>).version = 2
    }, /version/],
    ['an error event without a message', { version: 1, seq: 1, event: 'error' }, (line: Record<string, unknown>) => { line.message = 7 }, /message/],
    ['a state event whose state breaks the state contract', state, (line: Record<string, unknown>) => {
      (line.state as Record<string, unknown>).schema = 'x'
    }, /schema/],
    ['a search event with a non-array result list', search, (line: Record<string, unknown>) => { line.results = {} }, /results/],
  ])('rejects %s', (_label, base, mutate, message) => {
    const line = structuredClone(base) as Record<string, unknown>
    mutate(line)
    expect(() => validateResidentEvent(line)).toThrow(message)
  })

  it('rejects a stdout line that is not JSON at all', () => {
    expect(() => parseResidentLine('Wrote diagram.edn')).toThrow(/JSON/)
    expect(() => parseResidentLine('{"version":1,"event":"ack","seq":1}\\n')).toThrow(/JSON/)
  })

  it('writes one versioned action line per request', () => {
    expect(residentAction({ action: 'display', path: 'diagram.edn' }, 9)).toBe('{"version":1,"seq":9,"action":"display","path":"diagram.edn"}')
    expect(residentAction({ action: 'back' }, 2)).toBe('{"version":1,"seq":2,"action":"back"}')
    expect(JSON.parse(residentAction({ action: 'viewport', viewport: { width: 10, height: 20 } }, 1)))
      .toEqual({ version: RESIDENT_VERSION, seq: 1, action: 'viewport', viewport: { width: 10, height: 20 } })
  })
})
