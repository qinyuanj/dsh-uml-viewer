import { readFileSync } from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'
import { describe, expect, it } from 'vitest'
import {
  DISPLAY_RESULT_SCHEMA, PANEL_FONT_PATH, REMOTE_SERVICE, UML_VIEWER_REMOTE, parseDisplayRequest,
  parsePanelAction, parsePanelEvent, validateDisplayResult,
} from '../src/index.js'

const frames = path.resolve(fileURLToPath(new URL('../../../crates/frame/tests/fixtures/frames', import.meta.url)))
const root = JSON.parse(readFileSync(path.join(frames, 'root.frame.json'), 'utf8')) as unknown

describe('panel Remote descriptors', () => {
  it('declares one typed watch stream, one action call and the explicit close, all agent-bound', () => {
    const descriptors = UML_VIEWER_REMOTE.descriptors
    expect(UML_VIEWER_REMOTE.package).toBe('dsh-uml-viewer')
    // #32/D13: the owned pair is additive; the three version-1 methods keep their wire shape.
    expect(descriptors.map(descriptor => descriptor.method).sort()).toEqual(['action', 'close', 'closeOwned', 'watch', 'watchOwned'])
    expect(new Set(descriptors.map(descriptor => descriptor.id)).size).toBe(descriptors.length)
    const watch = descriptors.find(descriptor => descriptor.method === 'watch')!
    expect(watch.mode).toBe('stream')
    expect(watch.service).toBe(REMOTE_SERVICE)
    expect(watch.namespace).toBe(REMOTE_SERVICE)
    expect(watch.scope).toEqual({ context: 'agent', wire: 'agentId' })
    expect(watch.parameters.map(parameter => [parameter.wire, parameter.source])).toEqual([['agentId', 'lookup']])
    expect(watch.parameters[0]!.codec.typeSymbol).toBe('@deepseek-ai/dsh-session/types#SessionId')
    expect(watch.cancellation).toEqual({ parameter: 'signal' })
    const action = descriptors.find(descriptor => descriptor.method === 'action')!
    expect(action.mode).toBeUndefined()
    expect(action.parameters.map(parameter => [parameter.wire, parameter.source])).toEqual([['agentId', 'lookup'], ['action', 'json']])
    // #32/D13 additive close: a watch stream ending is a carrier loss, so the
    // panel needs one definite call of its own.
    const close = descriptors.find(descriptor => descriptor.method === 'close')!
    expect(close.id).toBe('dsh-uml-viewer#umlViewer/close')
    expect(close.service).toBe(REMOTE_SERVICE)
    expect(close.mode).toBeUndefined()
    expect(close.cancellation).toBeUndefined()
    expect(close.scope).toEqual({ context: 'agent', wire: 'agentId' })
    expect(close.parameters.map(parameter => [parameter.wire, parameter.source])).toEqual([['agentId', 'lookup']])
    expect(close.parameters[0]!.codec.typeSymbol).toBe('@deepseek-ai/dsh-session/types#SessionId')
    expect(close.result.create().parse({ closed: true })).toEqual({ closed: true })
    expect(close.result.create().parse({ closed: false })).toEqual({ closed: false })
    expect(() => close.result.create().parse({ closed: 'yes' })).toThrow(/closed/)
    expect(() => close.result.create().parse({})).toThrow(/closed/)
    expect(() => close.result.create().parse(null)).toThrow(/object/)
  })

  it('declares owned watch/close with agent lookup plus a strict primitive owner', () => {
    const descriptors = UML_VIEWER_REMOTE.descriptors
    const watch = descriptors.find(descriptor => descriptor.method === 'watchOwned')!
    const close = descriptors.find(descriptor => descriptor.method === 'closeOwned')!
    expect(watch.id).toBe('dsh-uml-viewer#umlViewer/watchOwned')
    expect(watch.mode).toBe('stream')
    expect(watch.cancellation).toEqual({ parameter: 'signal' })
    expect(close.id).toBe('dsh-uml-viewer#umlViewer/closeOwned')
    expect(close.mode).toBeUndefined()
    expect(close.cancellation).toBeUndefined()
    for (const descriptor of [watch, close]) {
      expect(descriptor.scope).toEqual({ context: 'agent', wire: 'agentId' })
      expect(descriptor.parameters.map(parameter => [parameter.wire, parameter.source])).toEqual([['agentId', 'lookup'], ['owner', 'json']])
      expect(descriptor.parameters[0]!.codec.typeSymbol).toBe('@deepseek-ai/dsh-session/types#SessionId')
      const owner = descriptor.parameters[1]!.codec.create()
      expect(owner.parse({ pageId: 'page-1', holderId: 'a_b-C' })).toEqual({ pageId: 'page-1', holderId: 'a_b-C' })
      expect(() => owner.parse({ pageId: 'page', holderId: '' })).toThrow(/owner/)
      expect(() => owner.parse({ pageId: 'x'.repeat(129), holderId: 'h' })).toThrow(/owner/)
      expect(() => owner.parse({ pageId: '../p', holderId: 'h' })).toThrow(/owner/)
      expect(() => owner.parse({ pageId: 'p' })).toThrow(/holderId/)
      // Extra fields are dropped; nothing beyond the two ids crosses.
      expect(owner.parse({ pageId: 'p', holderId: 'h', sessionId: 'other' })).toEqual({ pageId: 'p', holderId: 'h' })
    }
    expect(close.result.create().parse({ closed: false })).toEqual({ closed: false })
  })

  it('parses every event kind the panel receives, and rejects the rest', () => {
    const events = [
      { kind: 'ready', actions: ['open', 'display'] },
      { kind: 'frame', seq: 2, frame: root },
      { kind: 'context', context: { kind: 'real', edn: '{:context :real}' } },
      { kind: 'context', context: { kind: 'proposal', proposalId: ':p-shop', name: 'P', edn: '{:context :proposal}' } },
      { kind: 'state', state: { schema: 'dsh-uml-viewer/session-state', version: 1, diagram: null, context: null,
        focus: [], proposalId: null, openLayer: null, declutter: null, camera: { x: 0, y: 0, zoom: 1 }, selected: null, detail: null } },
      { kind: 'search', query: 'a', results: [] },
      { kind: 'refused', reason: 'read-only sandbox' },
      { kind: 'failed', reason: 'resident exited 3' },
    ]
    for (const event of events) expect(parsePanelEvent(event), String(event.kind)).toEqual(event)
    expect(() => parsePanelEvent({ kind: 'paint' })).toThrow(/kind/)
    expect(() => parsePanelEvent(null)).toThrow(/object/)
    expect(() => parsePanelEvent({ kind: 'frame', seq: 1, frame: { schema: 'x' } })).toThrow(/schema/)
    expect(() => parsePanelEvent({ kind: 'refused' })).toThrow(/reason/)
  })

  it('accepts the documented panel actions and refuses anything else', () => {
    const actions = [
      { action: 'viewport', viewport: { width: 800, height: 600 } },
      { action: 'camera', x: 1, y: 2, zoom: 0.5 },
      { action: 'display', path: 'diagram.edn' },
      { action: 'reload' }, { action: 'back' }, { action: 'namespace-tree' }, { action: 'declutter' }, { action: 'save' },
      { action: 'drill-in', id: ':core' }, { action: 'drill-in', at: [1, 2] },
      { action: 'select', at: null }, { action: 'select', at: [3, 4] },
      { action: 'search', query: 'graph' }, { action: 'locate', id: ':ir.graph' },
      { action: 'context', proposalId: null }, { action: 'context', proposalId: ':p-shop' },
    ]
    for (const action of actions) expect(parsePanelAction(action), action.action).toEqual(action)
    expect(() => parsePanelAction({ action: 'shell', argv: ['rm'] })).toThrow(/action/)
    expect(() => parsePanelAction({ action: 'display', path: '../escape.edn' })).toThrow(/relative/)
    expect(() => parsePanelAction({ action: 'viewport', viewport: { width: 'big', height: 1 } })).toThrow(/width/)
    expect(() => parsePanelAction({ action: 'search' })).toThrow(/query/)
  })

  it('names the packaged Sarasa face the client loads', () => {
    expect(PANEL_FONT_PATH).toBe('/dsh-uml-viewer/font/SarasaMonoSC-Regular.ttf')
  })
})

describe('uml_display contract', () => {
  it('accepts an optional project-relative diagram path', () => {
    expect(parseDisplayRequest({})).toEqual({})
    expect(parseDisplayRequest({ path: 'examples/a b.edn' })).toEqual({ path: 'examples/a b.edn' })
    expect(() => parseDisplayRequest({ path: '/etc/passwd' })).toThrow(/relative/)
    expect(() => parseDisplayRequest({ path: 'a/../../b.edn' })).toThrow(/relative/)
    expect(() => parseDisplayRequest({ output: 'diagram.edn' })).toThrow(/unknown/i)
  })

  it('returns a version-1 record with the ADR 0001 fields', () => {
    const record = { version: 1, operation: 'display', projectRoot: 'D:/p', path: 'diagram.edn', exitCode: null,
      validResult: true, crapScope: null, configured: false, tool: 'uml-viewer', status: 'ok', reason: null, target: null }
    expect(validateDisplayResult(record)).toEqual(record)
    expect(DISPLAY_RESULT_SCHEMA.properties.operation).toMatchObject({ const: 'display' })
    expect(() => validateDisplayResult({ ...record, status: 'applied' })).toThrow(/status/)
    expect(() => validateDisplayResult({ ...record, operation: 'ir' })).toThrow(/operation/)
  })

  // #54: the panel request a display answers names one element; the result
  // carries it so 「在面板中查看」 can locate the element. A display that answers
  // no request says so with null.
  it('carries the element target a display answers, validated like a panel request', () => {
    const record = { version: 1, operation: 'display', projectRoot: 'D:/p', path: 'diagram.edn', exitCode: null,
      validResult: true, crapScope: null, configured: false, tool: 'uml-viewer', status: 'ok', reason: null,
      target: { id: ':core', kind: 'component' } }
    expect(validateDisplayResult(record)).toEqual(record)
    expect(DISPLAY_RESULT_SCHEMA.required).toContain('target')
    expect(() => validateDisplayResult({ ...record, target: { id: 'core', kind: 'class' } })).toThrow(/keyword/)
    expect(() => validateDisplayResult({ ...record, target: { id: ':core', kind: 'layer' } })).toThrow(/kind/)
    expect(() => validateDisplayResult({ ...record, target: undefined })).toThrow(/target/)
    expect(validateDisplayResult({ ...record, target: null }).target).toBeNull()
  })
})
