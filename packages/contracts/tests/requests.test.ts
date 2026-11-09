import { describe, expect, it } from 'vitest'
import { QUALITY_REQUESTS, parsePanelAction, renderPanelRequest, validatePanelRequest } from '../src/index.js'

describe('quality panel requests', () => {
  it('offers exactly the three quality refreshes, in menu order', () => {
    expect(QUALITY_REQUESTS).toEqual(['refresh-crap', 'refresh-mutate', 'refresh-mutate-all'])
  })

  it('accepts a quality-request panel action for a class or component id', () => {
    for (const request of QUALITY_REQUESTS) {
      expect(parsePanelAction({ action: 'quality-request', request, id: ':core.order' }))
        .toEqual({ action: 'quality-request', request, id: ':core.order' })
    }
    expect(() => parsePanelAction({ action: 'quality-request', request: 'refresh-dry', id: ':core' })).toThrow(/request/)
    expect(() => parsePanelAction({ action: 'quality-request', request: 'omit', id: ':core' })).toThrow(/request/)
    expect(() => parsePanelAction({ action: 'quality-request', request: 'refresh-crap' })).toThrow(/id/)
    expect(() => parsePanelAction({ action: 'quality-request', request: 'refresh-crap', id: 'core' })).toThrow(/keyword/)
    expect(() => parsePanelAction({ action: 'quality-request', request: 'refresh-crap', id: ':a b' })).toThrow(/keyword/)
    expect(() => parsePanelAction({ action: 'quality-request', request: 'refresh-crap', id: ':a"]} :x' })).toThrow(/keyword/)
    // The client names the element only; it can never supply files.
    expect(parsePanelAction({ action: 'quality-request', request: 'refresh-mutate', id: ':core', files: ['/etc/passwd'] }))
      .toEqual({ action: 'quality-request', request: 'refresh-mutate', id: ':core' })
  })

  it('validates the delivered request: op, target and nonempty project-relative files', () => {
    const request = { op: 'refresh-mutate', target: { id: ':core', kind: 'component' }, files: ['src/core/a.ts', 'src/core/b.ts'] }
    expect(validatePanelRequest(request)).toEqual(request)
    expect(() => validatePanelRequest({ ...request, op: 'refresh-everything' })).toThrow(/op/)
    expect(() => validatePanelRequest({ ...request, target: { id: ':core', kind: 'layer' } })).toThrow(/kind/)
    expect(() => validatePanelRequest({ ...request, target: { id: 'core', kind: 'class' } })).toThrow(/keyword/)
    expect(() => validatePanelRequest({ ...request, files: [] })).toThrow(/files/)
    expect(() => validatePanelRequest({ ...request, files: ['../outside.ts'] })).toThrow(/relative/)
    expect(() => validatePanelRequest({ ...request, files: ['D:/abs.ts'] })).toThrow(/relative/)
    expect(() => validatePanelRequest({ ...request, files: ['src/a.ts', 'src/a.ts'] })).toThrow(/duplicate/)
  })

  it('renders one visible user message whose body is the structured EDN request', () => {
    expect(renderPanelRequest({ op: 'refresh-crap', target: { id: ':core.order', kind: 'class' }, files: ['src/core/order.ts'] }))
      .toBe('UML viewer panel request:\n{:op :refresh-crap :target {:id :core.order :kind :class} :files ["src/core/order.ts"]}')
    expect(renderPanelRequest({ op: 'refresh-mutate-all', target: { id: ':ir', kind: 'component' },
      files: ['crates/ir/src/a b.rs', 'crates/ir/src/q"uote.rs'] }))
      .toBe('UML viewer panel request:\n{:op :refresh-mutate-all :target {:id :ir :kind :component} :files ["crates/ir/src/a b.rs" "crates/ir/src/q\\"uote.rs"]}')
  })
})
