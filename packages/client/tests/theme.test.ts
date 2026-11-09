import { describe, expect, it } from 'vitest'
import { CHAR_W, estimateW, fittedSize, textWidth16 } from '../src/fit.js'
import { DARK_HUES, hueOf, LIGHT_HUES, resolveTheme } from '../src/theme.js'
import { fakeCanvas, DARK_TOKENS, LIGHT_TOKENS } from './fakes.js'

describe('layout estimate', () => {
  it('counts CJK and astral characters as two half-width units', () => {
    // The Rust source of truth: crates/scene/src/layout.rs.
    expect(textWidth16('。')).toBe(2)
    expect(textWidth16('一')).toBe(2)
    expect(textWidth16('ab')).toBe(2)
    expect(textWidth16('😀')).toBe(2)
    expect(textWidth16('提案')).toBe(4)
    expect(textWidth16('')).toBe(0)
    expect(estimateW('无数据', 16)).toBe(3 * 2 * CHAR_W * 16 / 16)
  })

  it('shrinks text until it fits maxW, and never grows past the nominal size', () => {
    // Monospaced 8 px per half-width at 16 px, so 20 units need 160 px.
    const measure = (size: number, text: string): number => (text.length * size) / 2
    expect(fittedSize(measure, 16, 'abcd', 200, false, false)).toBe(16)
    const shrunk = fittedSize(measure, 16, 'a'.repeat(20), 40, false, false)
    expect(shrunk).toBeLessThan(16)
    expect(measure(shrunk, 'a'.repeat(20))).toBeLessThanOrEqual(40 + 1e-6)
  })

  it('accounts for italic shear and bold offset when fitting, like the producer', () => {
    const measure = (size: number, text: string): number => text.length * size
    // Plain text is 8·16 = 128 wide; italic shear adds 0.2·size, so at a
    // 130 px budget plain text fits and italic text does not.
    const plain = fittedSize(measure, 16, '01234567', 130, false, false)
    expect(plain).toBe(16)
    const italic = fittedSize(measure, 16, '01234567', 130, false, true)
    expect(italic).toBeLessThan(16)
    expect(italic + italic * 0.2).toBeLessThanOrEqual(130 + 1e-6)
    const bold = fittedSize(measure, 16, '01234567', 130, true, false)
    expect(bold).toBeLessThanOrEqual(plain)
  })
})

describe('host theme tokens', () => {
  it('reads chrome roles from the host tokens, in light and dark', () => {
    const light = resolveTheme(fakeCanvas({ tokens: LIGHT_TOKENS }))
    expect(light.mode).toBe('light')
    expect(light.frame('canvas')).toBe('#ffffff')
    expect(light.frame('ink')).toBe('#1c2324')
    expect(light.frame('relation')).toBe('#6b7d76')

    const dark = resolveTheme(fakeCanvas({ tokens: DARK_TOKENS, dark: true }))
    expect(dark.mode).toBe('dark')
    expect(dark.frame('canvas')).toBe('#161c20')
    expect(dark.frame('ink')).toBe('#ecece4')
    // A token the host theme does not define falls back to ink, never to a stray color.
    expect(dark.frame('source-gutter')).toBe('#c7d6cc')
  })

  it('keeps the quality and violation hue in both modes, changing only lightness', () => {
    const light = resolveTheme(fakeCanvas({ tokens: LIGHT_TOKENS }))
    const dark = resolveTheme(fakeCanvas({ tokens: DARK_TOKENS, dark: true }))
    // The ramp anchors are the documented red / yellow-green / green; the light
    // palette holds their hue.
    for (const [index, grade] of [0, 5, 10].entries()) {
      const lightFill = light.frame('quality-fill', grade)
      const darkFill = dark.frame('quality-fill', grade)
      expect(Math.abs(hueOf(lightFill) - hueOf(darkFill)), `fill anchor ${grade}`).toBeLessThanOrEqual(1.5)
      const lightStroke = light.frame('quality-stroke', grade)
      const darkStroke = dark.frame('quality-stroke', grade)
      expect(Math.abs(hueOf(lightStroke) - hueOf(darkStroke)), `stroke anchor ${grade} (${index})`).toBeLessThanOrEqual(1.5)
    }
    // Interpolated grades mix the anchors in sRGB exactly like
    // crates/frame/src/theme.rs, so the hue stays within a few degrees of the
    // dark ramp rather than jumping colour family.
    for (const grade of [2.5, 7.5]) {
      expect(Math.abs(hueOf(light.frame('quality-fill', grade)) - hueOf(dark.frame('quality-fill', grade))),
        `fill grade ${grade}`).toBeLessThanOrEqual(5)
      expect(Math.abs(hueOf(light.frame('quality-stroke', grade)) - hueOf(dark.frame('quality-stroke', grade))),
        `stroke grade ${grade}`).toBeLessThanOrEqual(5)
    }
    // 违规关系色 is the same red in both modes.
    expect(Math.abs(hueOf(light.frame('violation')) - hueOf(dark.frame('violation')))).toBeLessThanOrEqual(1.5)
    // The palettes really do differ in lightness, otherwise the test proves nothing.
    expect(light.frame('canvas')).not.toBe(dark.frame('canvas'))
    expect(DARK_HUES.violation).not.toBe(LIGHT_HUES.violation)
  })

  it('does not invent RGB chrome colors when host tokens are absent', () => {
    const theme = resolveTheme(fakeCanvas({ tokens: {} }))
    for (const role of ['canvas', 'ink', 'muted', 'line', 'accent', 'danger']) {
      expect(theme.frame(role), role).toBe('')
    }
  })

  it('distinguishes hot violations from ordinary violations in both modes', () => {
    const light = resolveTheme(fakeCanvas({ tokens: LIGHT_TOKENS }))
    const dark = resolveTheme(fakeCanvas({ tokens: DARK_TOKENS, dark: true }))
    expect(dark.frame('violation')).toBe('#c42a24')
    expect(dark.frame('violation-hot')).toBe('#ff4030')
    expect(light.frame('violation-hot')).not.toBe(light.frame('violation'))
    expect(Math.abs(hueOf(light.frame('violation-hot')) - hueOf(dark.frame('violation-hot')))).toBeLessThanOrEqual(1.5)
  })

  it('paints the 依赖链高亮 in the egui window’s blue, not the host accent', () => {
    // frame::theme RELATED is #3e98ec; the default dark host accent is gold.
    const dark = resolveTheme(fakeCanvas({ tokens: DARK_TOKENS, dark: true }))
    expect(dark.frame('dependency-chain')).toBe('#3e98ec')
    expect(dark.frame('dependency-chain')).not.toBe(DARK_TOKENS['--dsw-alias-state-business-primary'])
    const light = resolveTheme(fakeCanvas({ tokens: LIGHT_TOKENS }))
    expect(light.frame('dependency-chain')).toBe('#136ec3')
    expect(Math.abs(hueOf(light.frame('dependency-chain')) - hueOf(dark.frame('dependency-chain')))).toBeLessThanOrEqual(1.5)
  })

  it('a skin that changes every host token leaves the hue-fixed roles unchanged in both modes', () => {
    // dsh-claude-style overrides the --dsw-alias-* tokens (warm orange accent,
    // its own reds and surfaces). Only chrome may follow; 质量色, 违规关系色,
    // 无数据 and 依赖链高亮 keep their colors and differ only by light/dark.
    const skin = (tokens: Record<string, string>): Record<string, string> => Object.fromEntries(
      Object.keys(tokens).map((key, index) => [key, ['#d97757', '#c6613f', '#3d3929', '#f0eee6', '#8a5cf6', '#1f1e1d'][index % 6]!]))
    const hueFixed: [string, number | null][] = [
      ...[0, 1, 2.5, 5, 7.5, 10, null].flatMap(g => [['quality-fill', g], ['quality-stroke', g], ['package-fill', g], ['package-stroke', g]] as [string, number | null][]),
      ['violation', null], ['violation-hot', null], ['no-data', null], ['dependency-chain', null],
    ]
    for (const [mode, tokens] of [['dark', DARK_TOKENS], ['light', LIGHT_TOKENS]] as const) {
      const host = resolveTheme(fakeCanvas({ tokens, dark: mode === 'dark' }))
      const skinned = resolveTheme(fakeCanvas({ tokens: skin(tokens), dark: mode === 'dark' }))
      // The skin really changed the chrome, otherwise the comparison proves nothing.
      expect(skinned.frame('canvas')).not.toBe(host.frame('canvas'))
      expect(skinned.frame('relation')).not.toBe(host.frame('relation'))
      for (const [role, grade] of hueFixed) {
        expect(skinned.frame(role, grade), `${mode} ${role} ${grade}`).toBe(host.frame(role, grade))
      }
    }
    const dark = resolveTheme(fakeCanvas({ tokens: skin(DARK_TOKENS), dark: true }))
    const light = resolveTheme(fakeCanvas({ tokens: skin(LIGHT_TOKENS) }))
    for (const role of ['violation', 'violation-hot', 'no-data', 'dependency-chain']) {
      expect(Math.abs(hueOf(light.frame(role)) - hueOf(dark.frame(role))), role).toBeLessThanOrEqual(1.5)
      expect(light.frame(role), role).not.toBe(dark.frame(role))
    }
  })

  it('draws an unknown role as ink and an unmeasured grade as the neutral ramp anchor', () => {
    const theme = resolveTheme(fakeCanvas({ tokens: DARK_TOKENS, dark: true }))
    expect(theme.frame('a-role-from-the-future')).toBe('#ecece4')
    expect(theme.frame('quality-fill', null)).toBe(DARK_HUES.neutralFill)
    expect(theme.frame('quality-stroke', 11)).toBe(theme.frame('quality-stroke', 10))
  })
})
