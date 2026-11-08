import { readFileSync } from 'node:fs'
import { describe, expect, it } from 'vitest'
import { parseQualityResult } from '../src/index.js'

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
