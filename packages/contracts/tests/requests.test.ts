import { describe, expect, it } from 'vitest'
import {
  QUALITY_REQUESTS, QUALITY_TARGET_LIMITS, parsePanelAction, renderPanelRequest, validatePanelRequest,
  validateQualityTarget,
} from '../src/index.js'

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
describe('resident quality targets', () => {
  const target = { id: ':core', kind: 'component' as const, files: ['src/core/a.ts', 'src/core/b.ts'] }

  it('accepts the producer answer and checks it is about the element asked for', () => {
    expect(validateQualityTarget(target)).toEqual(target)
    expect(validateQualityTarget(target, 'resident event.target', ':core')).toEqual(target)
    expect(() => validateQualityTarget(target, 'quality target', ':api')).toThrow(/does not match the requested :api/)
  })

  it('keeps the producer bounds and order as one contract', () => {
    // viewer_session::targets QUALITY_TARGET_* — the numbers must stay equal.
    expect(QUALITY_TARGET_LIMITS).toEqual({ files: 4096, pathBytes: 4096, eventBytes: 1024 * 1024 })
    for (const [value, message] of [
      [{ ...target, id: 'core' }, /keyword/],
      [{ ...target, kind: 'layer' }, /kind/],
      [{ ...target, files: [] }, /files/],
      [{ ...target, files: Array.from({ length: QUALITY_TARGET_LIMITS.files + 1 }, (_, index) => `src/f${index}.ts`) }, /limit is 4096/],
      [{ ...target, files: ['../outside.ts'] }, /relative/],
      [{ ...target, files: ['/etc/passwd'] }, /relative/],
      [{ ...target, files: ['D:/abs.ts'] }, /relative/],
      [{ ...target, files: ['src\\core\\a.ts'] }, /forward slashes/],
      [{ ...target, files: ['-rf'] }, /command option/],
      [{ ...target, files: ['src/core/a.ts', 'src/core/a.ts'] }, /duplicate/],
      [{ ...target, files: ['src/core/b.ts', 'src/core/a.ts'] }, /ascending path order/],
      [{ ...target, files: ['a'.repeat(QUALITY_TARGET_LIMITS.pathBytes + 1)] }, /limit is 4096/],
      [null, /must be an object/],
    ] as const) {
      expect(() => validateQualityTarget(value), JSON.stringify(value).slice(0, 40)).toThrow(message)
    }
  })

  it('orders non-ASCII paths by their UTF-8 bytes, as the resident sorts them', () => {
    const files = ['src/a.ts', 'src/é.ts']
    expect(validateQualityTarget({ ...target, files })).toEqual({ ...target, files })
    expect(() => validateQualityTarget({ ...target, files: ['src/é.ts', 'src/a.ts'] })).toThrow(/ascending/)
  })
})
