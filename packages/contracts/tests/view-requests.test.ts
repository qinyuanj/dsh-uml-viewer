// #29: the panel requests that change what the diagram shows (omit, the
// discussion context, a new proposal). They need no quality refresh; the
// companion edits the policy in place and regenerates the IR.
import { describe, expect, it } from 'vitest'
import {
  IR_PARAMETERS, defaultProposalName, parseIrRequest, parsePanelAction, parseProposalName, renderContextRequest,
  renderNewProposalRequest, renderOmitRequest,
} from '../src/index.js'

const header = 'UML viewer panel request:\n'

describe('omit requests', () => {
  it('names the element, the diagram under discussion and the request id', () => {
    expect(renderOmitRequest({
      target: { id: ':core.order', kind: 'class' },
      context: { kind: 'real', edn: '{:context :real}' },
      request: 'a1b2c3d4e5f60718293a4b5c6d7e8f90',
    })).toBe(`${header}{:op :omit :target {:id :core.order :kind :class} :context {:context :real} :request "a1b2c3d4e5f60718293a4b5c6d7e8f90"}`)
    expect(renderOmitRequest({
      target: { id: ':core', kind: 'component' },
      context: { kind: 'proposal', proposalId: ':p-shop', name: '2026-10-24 10:00:00',
        edn: '{:context :proposal :name "2026-10-24 10:00:00" :proposal-id :p-shop}' },
      request: '0123456789abcdef0123456789abcdef',
    })).toBe(`${header}{:op :omit :target {:id :core :kind :component} `
      + ':context {:context :proposal :name "2026-10-24 10:00:00" :proposal-id :p-shop} :request "0123456789abcdef0123456789abcdef"}')
  })

  it('refuses a context map that is not one line of its own kind, and a malformed id', () => {
    const base = { target: { id: ':core', kind: 'class' as const }, context: { kind: 'real' as const, edn: '{:context :real}' }, request: 'a1b2c3d4e5f60718293a4b5c6d7e8f90' }
    for (const [value, message] of [
      [{ ...base, context: { kind: 'real', edn: '{:context :real}\n{:op :omit}' } }, /one line/],
      [{ ...base, context: { kind: 'real', edn: '{:context :proposal :proposal-id :p}' } }, /\{:context :real/],
      [{ ...base, context: { kind: 'proposal', edn: '{:context :proposal :proposal-id :p} :x' } }, /\{:context :proposal/],
      [{ ...base, context: { kind: 'proposal', edn: `{:context :proposal :name "${'x'.repeat(4096)}"}` } }, /4096 bytes/],
      [{ ...base, target: { id: 'core', kind: 'class' } }, /keyword/],
      [{ ...base, target: { id: ':core', kind: 'layer' } }, /kind/],
      [{ ...base, request: 'A1B2C3D4E5F60718293A4B5C6D7E8F90' }, /request id/],
      [{ ...base, request: '"} {:op :x' }, /request id/],
    ] as const) {
      expect(() => renderOmitRequest(value as Parameters<typeof renderOmitRequest>[0]), JSON.stringify(value).slice(0, 60)).toThrow(message)
    }
  })
})

describe('discussion context requests', () => {
  it('quotes the context map the resident switched to', () => {
    expect(renderContextRequest({ kind: 'real', edn: '{:context :real}' }))
      .toBe(`${header}{:op :context :context {:context :real}}`)
    expect(renderContextRequest({ kind: 'proposal', proposalId: ':p-shop', name: '中文',
      edn: '{:context :proposal :name "中文" :proposal-id :p-shop}' }))
      .toBe(`${header}{:op :context :context {:context :proposal :name "中文" :proposal-id :p-shop}}`)
    expect(() => renderContextRequest({ kind: 'real', edn: '{:context :real}\r' })).toThrow(/one line/)
  })
})

describe('new proposal requests', () => {
  it('names the proposal, escaped as an EDN string, with the request id', () => {
    expect(renderNewProposalRequest({ name: '2026-11-09 14:05:06', request: 'a1b2c3d4e5f60718293a4b5c6d7e8f90' }))
      .toBe(`${header}{:op :new-proposal :name "2026-11-09 14:05:06" :request "a1b2c3d4e5f60718293a4b5c6d7e8f90"}`)
    expect(renderNewProposalRequest({ name: 'Split "core" \\ io', request: 'a1b2c3d4e5f60718293a4b5c6d7e8f90' }))
      .toBe(`${header}{:op :new-proposal :name "Split \\"core\\" \\\\ io" :request "a1b2c3d4e5f60718293a4b5c6d7e8f90"}`)
  })

  it('accepts a trimmed one-line name of at most 256 bytes', () => {
    expect(parseProposalName('  拆分 core  ')).toBe('拆分 core')
    for (const [value, message] of [
      ['', /empty/], ['   ', /empty/], ['a\nb', /one line/], ['tab\there', /one line/],
      ['x'.repeat(257), /256 bytes/], [42, /string/],
    ] as const) {
      expect(() => parseProposalName(value), String(value).slice(0, 20)).toThrow(message)
    }
  })

  it('defaults the name to the local time to the second, like the egui viewer', () => {
    // 2026-11-09 14:05:06 local time, whatever the machine's time zone is.
    expect(defaultProposalName(new Date(2026, 10, 9, 14, 5, 6))).toBe('2026-11-09 14:05:06')
    expect(defaultProposalName(new Date(2027, 0, 2, 3, 4, 5, 999))).toBe('2027-01-02 03:04:05')
  })
})

describe('view panel actions', () => {
  it('accepts omit with one element id and new-proposal with an optional name', () => {
    expect(parsePanelAction({ action: 'omit', id: ':core.order', files: ['x'] })).toEqual({ action: 'omit', id: ':core.order' })
    expect(() => parsePanelAction({ action: 'omit', id: 'core' })).toThrow(/keyword/)
    expect(() => parsePanelAction({ action: 'omit' })).toThrow(/id/)
    expect(parsePanelAction({ action: 'new-proposal' })).toEqual({ action: 'new-proposal' })
    expect(parsePanelAction({ action: 'new-proposal', name: ' 拆分 ' })).toEqual({ action: 'new-proposal', name: '拆分' })
    expect(() => parsePanelAction({ action: 'new-proposal', name: 'a\nb' })).toThrow(/one line/)
  })

  it('lets uml_ir name the panel request it answers, and nothing else', () => {
    const id = 'a1b2c3d4e5f60718293a4b5c6d7e8f90'
    expect(parseIrRequest({ request: id })).toEqual({ request: id })
    expect(parseIrRequest({ policy: 'p.edn', request: id })).toEqual({ policy: 'p.edn', request: id })
    for (const bad of ['', 'a1b2c3d4e5f6', id.toUpperCase(), `${id}0`, 42]) {
      expect(() => parseIrRequest({ request: bad }), String(bad)).toThrow(/request id/)
    }
    expect(IR_PARAMETERS.properties.request).toMatchObject({ type: 'string', pattern: '^[0-9a-f]{32}$' })
  })
})
