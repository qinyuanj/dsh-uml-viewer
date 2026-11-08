import { describe, expect, it } from 'vitest'
import { parseIrRequest } from '../src/index.js'

describe('IR request', () => {
  it('accepts policy and output paths literally and rejects unknown authority options', () => {
    expect(parseIrRequest({ policy: 'examples/a & b.policy.edn', output: 'diagram.edn' }))
      .toEqual({ policy: 'examples/a & b.policy.edn', output: 'diagram.edn' })
    expect(parseIrRequest({})).toEqual({})
    expect(() => parseIrRequest({ cwd: '/another/project' })).toThrow(/unknown/i)
    expect(() => parseIrRequest({ policy: '' })).toThrow(/policy/i)
    expect(() => parseIrRequest(null)).toThrow(/object/i)
    expect(() => parseIrRequest({ output: String.fromCharCode(0) })).toThrow(/NUL/i)
    expect(parseIrRequest({ policy: String.raw`examples\a\policy.edn` })).toEqual({ policy: String.raw`examples\a\policy.edn` })
  })
})
