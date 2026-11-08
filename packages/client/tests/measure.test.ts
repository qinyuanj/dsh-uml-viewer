import { readFileSync } from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'
import { describe, expect, it } from 'vitest'
import type { SceneFrame } from '@dsh-uml-viewer/contracts'
import { estimateW, textWidth16 } from '../src/fit.js'
import { readFontMetrics } from './font-metrics.js'

const root = path.resolve(fileURLToPath(new URL('../../..', import.meta.url)))
const face = path.join(root, 'fonts', 'SarasaMonoSC-Regular.ttf')
const sarasa = readFontMetrics(face, 'Sarasa Mono SC')
const framesDir = path.join(root, 'crates', 'frame', 'tests', 'fixtures', 'frames')

function golden(name: string): SceneFrame {
  return JSON.parse(readFileSync(path.join(framesDir, name), 'utf8')) as SceneFrame
}

function textOf(frame: SceneFrame): string[] {
  const out: string[] = []
  for (const item of frame.world) {
    if (item.kind === 'text' && item.text) out.push(item.text)
    if (item.kind === 'package' && item.text) out.push(item.text)
    if (item.kind === 'port') out.push(item.text)
  }
  for (const item of frame.chrome) {
    if ('text' in item && typeof item.text === 'string') out.push(item.text)
  }
  return out
}

describe('Sarasa measurement against the layout estimate', () => {
  it('covers the CJK repertoire the labels use', () => {
    expect(sarasa.unitsPerEm).toBe(1000)
    for (const character of '无数据提案未在源码中实例化返回') {
      expect(sarasa.has(character.codePointAt(0)!), `missing glyph ${character}`).toBe(true)
    }
  })

  it('advances CJK at two half-width units, the width the layout estimate sized the box for', () => {
    for (const character of '无数据提案未在源码中实例化') {
      const codePoint = character.codePointAt(0)!
      // CJK counts as two half-width units: 16 px at the 16 px body size.
      expect(sarasa.advancePx(codePoint, 16)).toBeCloseTo(estimateW(character, 16), 6)
    }
    // ASCII is monospaced at 0.5 em, the same eight-pixel advance the estimate uses.
    for (const character of 'abcdefghijklmnopqrstuvwxyz0123456789') {
      expect(sarasa.advancePx(character.codePointAt(0)!, 16)).toBeCloseTo(8, 6)
    }
    // A pure-CJK label the producer puts in a box is never wider than its estimate.
    for (const label of ['无数据', '提案']) {
      expect(sarasa.widthPx(label, 16)).toBeLessThanOrEqual(estimateW(label, 16) + 1e-6)
    }
  })

  it('fits real Sarasa text inside the box maxW by shrinking (CJK and long labels)', () => {
    const frame = golden('core-selected.frame.json')
    const labels = textOf(frame).filter(value => textWidth16(value) > 0)
    expect(labels.length).toBeGreaterThan(5)
    for (const item of frame.world) {
      if (item.kind !== 'text' || !item.text) continue
      const advance = (size: number, value: string): number => sarasa.widthPx(value, size)
      // Bisect exactly like the painter's `fittedSize`.
      let size = item.size
      if (advance(size, item.text) > item.maxW) {
        let lo = 0
        let hi = item.size
        for (let step = 0; step < 32; step += 1) {
          const mid = (lo + hi) / 2
          if (advance(mid, item.text) > item.maxW) hi = mid
          else lo = mid
        }
        size = lo
      }
      expect(advance(size, item.text), `${item.text} fits maxW ${item.maxW}`).toBeLessThanOrEqual(item.maxW + 1e-6)
      expect(size).toBeLessThanOrEqual(item.size)
    }
  })
})
