import { describe, expect, it } from 'vitest'
import { DARK_HUES, hueOf, LIGHT_HUES, resolveTheme } from '../src/theme.js'
import { fakeCanvas, DARK_TOKENS, LIGHT_TOKENS } from './fakes.js'

const SKINNED: Record<string, string> = {
  '--dsw-alias-state-success-primary': '#0000ff',
  '--dsw-alias-state-warn-primary': '#ff00ff',
  '--dsw-alias-state-error-primary': '#00ffff',
}

describe('overlay quality roles', () => {
  for (const [mode, tokens, hues] of [['light', LIGHT_TOKENS, LIGHT_HUES], ['dark', DARK_TOKENS, DARK_HUES]] as const) {
    it(`keeps quality, violation and no-data hues when host state tokens change (${mode})`, () => {
      const dark = mode === 'dark'
      const base = resolveTheme(fakeCanvas({ tokens, dark }))
      const skinned = resolveTheme(fakeCanvas({ tokens: { ...tokens, ...SKINNED }, dark }))
      for (const grade of [0, 2.5, 5, 6, 7.5, 9, 10, null]) {
        for (const role of ['quality-fill', 'quality-stroke', 'package-fill', 'package-stroke']) {
          expect(skinned.frame(role, grade), `${role} ${grade}`).toBe(base.frame(role, grade))
        }
      }
      for (const role of ['violation', 'violation-hot', 'no-data']) {
        expect(skinned.frame(role), role).toBe(base.frame(role))
      }
      expect(base.frame('no-data')).toBe(hues.noData)
      expect(base.frame('no-data')).not.toBe(tokens['--dsw-alias-state-error-primary'])
      const red = hueOf(base.frame('no-data'))
      expect(Math.min(red, 360 - red)).toBeLessThanOrEqual(5)
    })

    it(`grades a continuous ramp with no invented buckets (${mode})`, () => {
      const theme = resolveTheme(fakeCanvas({ tokens, dark: mode === 'dark' }))
      expect(theme.frame('quality-stroke', 6)).not.toBe(theme.frame('quality-stroke', 7))
      expect(theme.frame('quality-stroke', 8)).not.toBe(theme.frame('quality-stroke', 9))
      expect(theme.frame('quality-stroke', 4.9)).not.toBe(theme.frame('quality-stroke', 4))
      expect(theme.frame('quality-stroke', 0)).toBe(hues.stroke[0])
      expect(theme.frame('quality-stroke', 5)).toBe(hues.stroke[1])
      expect(theme.frame('quality-stroke', 10)).toBe(hues.stroke[2])
      expect(theme.frame('quality-stroke', null)).toBe(hues.neutralStroke)
    })

    it(`lets the coverage ratio choose the documented coverage ink (${mode})`, () => {
      const theme = resolveTheme(fakeCanvas({ tokens, dark: mode === 'dark' }))
      const good = tokens['--dsw-alias-state-success-primary']
      const gold = tokens['--dsw-alias-state-business-primary']
      const low = tokens['--dsw-alias-state-warn-primary']
      expect(theme.frame('coverage', null, 0.9)).toBe(good)
      expect(theme.frame('coverage', null, 0.8)).toBe(good)
      expect(theme.frame('coverage', null, 0.79)).toBe(gold)
      expect(theme.frame('coverage', null, 0.5)).toBe(gold)
      expect(theme.frame('coverage', null, 0.2)).toBe(low)
      expect(theme.frame('coverage', null, null)).toBe(tokens['--dsw-alias-label-tertiary'])
      expect(new Set([0.9, 0.6, 0.2].map(r => theme.frame('coverage', null, r))).size).toBe(3)
    })
  }

  it('changes only lightness between light and dark for quality and violation', () => {
    const light = resolveTheme(fakeCanvas({ tokens: LIGHT_TOKENS }))
    const dark = resolveTheme(fakeCanvas({ tokens: DARK_TOKENS, dark: true }))
    for (const role of ['violation', 'no-data']) {
      expect(light.frame(role)).not.toBe(dark.frame(role))
      expect(Math.abs(hueOf(light.frame(role)) - hueOf(dark.frame(role))), role).toBeLessThanOrEqual(1.5)
    }
    for (const grade of [0, 6, 10]) {
      expect(Math.abs(hueOf(light.frame('quality-stroke', grade)) - hueOf(dark.frame('quality-stroke', grade))),
        `stroke ${grade}`).toBeLessThanOrEqual(5)
    }
  })
})
