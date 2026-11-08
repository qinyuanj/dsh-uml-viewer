/**
 * A readable TrueType/OpenType metrics reader for the packaged Sarasa face.
 * Only what a measurement test needs: `head`, `hhea`, `hmtx`, `maxp` and the
 * `cmap` (format 4 and 12 subtables). No shaping, no kerning: the layout
 * estimate is a per-character advance, so the test compares per-character
 * advances too.
 */
import { readFileSync } from 'node:fs'

export interface FontMetrics {
  family: string
  unitsPerEm: number
  advanceUnits(codePoint: number): number
  advancePx(codePoint: number, sizePx: number): number
  widthPx(text: string, sizePx: number): number
  has(codePoint: number): boolean
}

interface Cmap {
  glyphOf(codePoint: number): number
}

function table(buffer: Buffer, tag: string): { offset: number; length: number } {
  const count = buffer.readUInt16BE(4)
  for (let index = 0; index < count; index += 1) {
    const entry = 12 + index * 16
    if (buffer.toString('ascii', entry, entry + 4) === tag) {
      return { offset: buffer.readUInt32BE(entry + 8), length: buffer.readUInt32BE(entry + 12) }
    }
  }
  throw new Error(`font has no ${tag} table`)
}

function readCmap(buffer: Buffer, offset: number): Cmap {
  const count = buffer.readUInt16BE(offset + 2)
  let best: { format: number; offset: number } | null = null
  for (let index = 0; index < count; index += 1) {
    const record = offset + 4 + index * 8
    const platform = buffer.readUInt16BE(record)
    const encoding = buffer.readUInt16BE(record + 2)
    const subtable = offset + buffer.readUInt32BE(record + 4)
    const format = buffer.readUInt16BE(subtable)
    const unicodeFull = (platform === 3 && encoding === 10) || (platform === 0 && encoding >= 4)
    const unicodeBmp = platform === 0 || (platform === 3 && (encoding === 1 || encoding === 0))
    if (!unicodeFull && !unicodeBmp) continue
    // Prefer a full-repertoire subtable, then the first BMP one.
    if (best === null || (unicodeFull && format === 12 && best.format !== 12)) {
      best = { format, offset: subtable }
    }
  }
  if (best === null) throw new Error('font has no Unicode cmap subtable')

  if (best.format === 12) {
    const groups = buffer.readUInt32BE(best.offset + 12)
    return {
      glyphOf(codePoint: number): number {
        let lo = 0
        let hi = groups - 1
        while (lo <= hi) {
          const mid = (lo + hi) >> 1
          const group = best!.offset + 16 + mid * 12
          const start = buffer.readUInt32BE(group)
          const end = buffer.readUInt32BE(group + 4)
          if (codePoint < start) hi = mid - 1
          else if (codePoint > end) lo = mid + 1
          else return buffer.readUInt32BE(group + 8) + (codePoint - start)
        }
        return 0
      },
    }
  }

  if (best.format === 4) {
    const segCount = buffer.readUInt16BE(best.offset + 6) / 2
    const ends = best.offset + 14
    const starts = ends + segCount * 2 + 2
    const deltas = starts + segCount * 2
    const ranges = deltas + segCount * 2
    return {
      glyphOf(codePoint: number): number {
        for (let segment = 0; segment < segCount; segment += 1) {
          const end = buffer.readUInt16BE(ends + segment * 2)
          if (codePoint > end) continue
          const start = buffer.readUInt16BE(starts + segment * 2)
          if (codePoint < start) return 0
          const delta = buffer.readInt16BE(deltas + segment * 2)
          const rangeOffset = buffer.readUInt16BE(ranges + segment * 2)
          if (rangeOffset === 0) return (codePoint + delta) & 0xffff
          const address = ranges + segment * 2 + rangeOffset + (codePoint - start) * 2
          if (address + 1 >= buffer.length) return 0
          const glyph = buffer.readUInt16BE(address)
          return glyph === 0 ? 0 : (glyph + delta) & 0xffff
        }
        return 0
      },
    }
  }
  throw new Error(`unsupported cmap format ${best.format}`)
}

export function readFontMetrics(file: string, family: string): FontMetrics {
  const buffer = readFileSync(file)
  const head = table(buffer, 'head')
  const unitsPerEm = buffer.readUInt16BE(head.offset + 18)
  const hhea = table(buffer, 'hhea')
  const numberOfHMetrics = buffer.readUInt16BE(hhea.offset + 34)
  const maxp = table(buffer, 'maxp')
  const numGlyphs = buffer.readUInt16BE(maxp.offset + 4)
  const hmtx = table(buffer, 'hmtx')
  const cmap = readCmap(buffer, table(buffer, 'cmap').offset)

  const advanceUnits = (codePoint: number): number => {
    const glyph = cmap.glyphOf(codePoint)
    if (glyph <= 0 || glyph >= numGlyphs) return 0
    const index = Math.min(glyph, numberOfHMetrics - 1)
    return buffer.readUInt16BE(hmtx.offset + index * 4)
  }
  return {
    family,
    unitsPerEm,
    has: (codePoint: number) => cmap.glyphOf(codePoint) > 0,
    advanceUnits,
    advancePx: (codePoint: number, sizePx: number) => (advanceUnits(codePoint) * sizePx) / unitsPerEm,
    widthPx: (text: string, sizePx: number) => {
      let total = 0
      for (const character of text) total += (advanceUnits(character.codePointAt(0)!) * sizePx) / unitsPerEm
      return total
    },
  }
}
