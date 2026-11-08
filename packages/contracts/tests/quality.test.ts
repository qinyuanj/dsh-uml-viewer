import { readFileSync } from 'node:fs'
import { describe, expect, it } from 'vitest'
import { parseQualityResult, parseQualityToolRequest, validateQualityToolResult } from '../src/index.js'

// Lines printed by the Rust binary (crates/uml-viewer/tests/quality.rs checks
// the binary still prints exactly these fields).
const samples = readFileSync(new URL('../../../fixtures/quality/records.jsonl', import.meta.url), 'utf8')
  .split('\n').filter(Boolean)

describe('quality result line', () => {
  it('accepts every committed sample line unchanged', () => {
    expect(samples.length).toBeGreaterThanOrEqual(4)
    for (const line of samples) {
      const parsed = parseQualityResult(line)
      expect(parsed).toEqual(JSON.parse(line))
    }
  })

  it('tells a refusal from a tool that really exited 1', () => {
    const parsed = samples.map(parseQualityResult)
    const refused = parsed.find((r) => r.reason !== null)
    const failed = parsed.find((r) => r.reason === null && r.exitCode === 1)
    expect(refused?.exitCode).toBe(1)
    expect(refused?.validResult).toBe(false)
    expect(failed?.validResult).toBe(false)
  })

  it('rejects other shapes', () => {
    const base = JSON.parse(samples[0]!)
    for (const bad of [
      { ...base, operation: 'ir' }, { ...base, version: 2 }, { ...base, exitCode: null },
      { ...base, extra: 1 }, { ...base, reason: 'x', validResult: true },
      { ...base, operation: 'mutate', crapScope: 'targets' },
    ]) expect(() => parseQualityResult(JSON.stringify(bad))).toThrow()
    const { reason: _, ...missing } = base
    expect(() => parseQualityResult(JSON.stringify(missing))).toThrow(/reason/)
  })
})

describe('quality tool request', () => {
  it('keeps every mutation target in one argv and scopes --mutate-all to them', () => {
    expect(parseQualityToolRequest('mutate', { targets: ['src/a.rs', 'src/dir b/c.rs'], mutateAll: true }))
      .toEqual(['--mutate-all', 'src/a.rs', 'src/dir b/c.rs'])
    expect(parseQualityToolRequest('crap', {})).toEqual([])
    expect(parseQualityToolRequest('crap', { targets: ['src/a.rs'] })).toEqual(['src/a.rs'])
    expect(parseQualityToolRequest('dry', {})).toEqual([])
  })

  it('refuses an empty mutation, authority paths, options and unknown fields before spawn', () => {
    expect(() => parseQualityToolRequest('mutate', { targets: [] })).toThrow(/at least one/)
    expect(() => parseQualityToolRequest('mutate', { targets: ['../outside.rs'] })).toThrow(/inside/)
    expect(() => parseQualityToolRequest('mutate', { targets: ['C:/outside.rs'] })).toThrow(/inside/)
    expect(() => parseQualityToolRequest('mutate', { targets: ['--mutate-all'] })).toThrow(/option/)
    expect(() => parseQualityToolRequest('mutate', { targets: ['a.rs'], root: '/elsewhere' })).toThrow(/Unknown/)
    expect(() => parseQualityToolRequest('crap', { mutateAll: true })).toThrow(/Unknown/)
    expect(() => parseQualityToolRequest('dry', { targets: [] })).toThrow(/Unknown/)
  })
})

describe('quality tool result', () => {
  const record = {
    version: 1, operation: 'crap', projectRoot: '/p', exitCode: 0, validResult: true,
    crapScope: 'whole-tree', configured: false, tool: 'crapper', reason: null,
  }

  it('accepts a host refusal with no child exit and rejects cross-operation values', () => {
    expect(validateQualityToolResult({ ...record, exitCode: null, validResult: false,
      reason: 'read-only sandbox: refused' }, 'crap')).toMatchObject({ validResult: false, exitCode: null })
    expect(() => validateQualityToolResult(record, 'mutate')).toThrow(/must be mutate/)
    expect(() => validateQualityToolResult({ ...record, reason: 'refused' })).toThrow(/never a valid result/)
    expect(() => validateQualityToolResult({ ...record, operation: 'ir' })).toThrow(/crap, mutate or dry/)
  })
})
