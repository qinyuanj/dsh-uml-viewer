/**
 * Theme roles for the Canvas 2D panel.
 *
 * Canvas cannot read `var(--*)`, so each role resolves once per paint from the
 * host's CSS custom properties, read through `getComputedStyle` on the panel
 * element. Chrome (surfaces, text, lines, interaction) therefore follows the
 * host theme exactly, including light/dark.
 *
 * Two groups cannot come from the host theme: the 质量色 quality ramp and the
 * 违规关系色 violation color. Their **hue is fixed** (docs/scene-frame.md) and
 * only their lightness adapts to light/dark, so they come from this module's
 * two literal palettes. Every returned color keeps the same hue in both modes.
 */
import type { SceneFrame } from '@dsh-uml-viewer/contracts'

/** The host theme tokens per role; `null` marks a hue-fixed role. */
const TOKENS: Readonly<Record<string, string | null>> = {
  canvas: '--dsw-alias-bg-base',
  panel: '--dsw-alias-bg-layer-2',
  sidebar: '--dsw-alias-bg-layer-3',
  line: '--dsw-alias-border-l2',
  ink: '--dsw-alias-label-primary',
  muted: '--dsw-alias-label-tertiary',
  accent: '--dsw-alias-state-business-primary',
  marker: '--dsw-alias-label-primary',
  selected: '--dsw-alias-label-primary',
  good: '--dsw-alias-state-success-primary',
  danger: '--dsw-alias-state-error-primary',
  'chrome-plate': '--dsw-alias-bg-overlay',
  popup: '--dsw-alias-bg-overlay',
  'row-wash': '--dsw-alias-state-business-tertiary',
  'row-bar': '--dsw-alias-state-business-primary',
  'quality-fill': null,
  'quality-stroke': null,
  'package-fill': null,
  'package-stroke': null,
  coverage: '--dsw-alias-state-warn-primary',
  'no-data': null,
  relation: '--dsw-alias-label-tertiary',
  'relation-hover': '--dsw-alias-label-primary',
  'dependency-chain': '--dsw-alias-state-business-primary',
  violation: null,
  'violation-hot': null,
  indicator: '--dsw-alias-label-tertiary',
  'port-fill': '--dsw-alias-bg-layer-2',
  'port-fill-marked': '--dsw-alias-state-done-secondary',
  'port-outline': '--dsw-alias-label-tertiary',
  'port-link': '--dsw-alias-label-tertiary',
  'button-hover': '--dsw-alias-interactive-bg-hover',
  'button-active': '--dsw-alias-interactive-bg-active',
  'source-bg': '--dsw-alias-bg-layer-2',
  'source-ink': '--dsw-alias-label-primary',
  'source-gutter-bg': '--dsw-alias-bg-layer-3',
  'source-gutter': '--dsw-alias-label-secondary',
  'source-selection': '--dsw-alias-state-business-tertiary',
  'source-comment': '--dsw-alias-label-tertiary',
  'source-string': '--dsw-alias-state-warn-primary',
  'source-keyword': '--dsw-alias-state-business-primary',
}

/**
 * The hue-fixed palettes. The fill and stroke ramps run red → yellow-green →
 * green and the violation color is the 违规关系色 red. Light/dark changes only
 * **lightness**: each light value is its dark counterpart's hue and saturation
 * at a different L (HLS), so a grade or a violating relation reads as the same
 * color in both modes. `docs/scene-frame.md` names the dark values.
 */
export interface HuePalette {
  fill: readonly [string, string, string]
  stroke: readonly [string, string, string]
  violation: string
  violationHot: string
  noData: string
  /** Unmeasured quality: neutral fill and neutral stroke (`#243430`/`#5a6e64`). */
  neutralFill: string
  neutralStroke: string
}

export const DARK_HUES: HuePalette = {
  fill: ['#4a2818', '#3d3a18', '#1e4a38'],
  stroke: ['#e07a4a', '#d4c05a', '#5fb58a'],
  violation: '#c42a24',
  violationHot: '#ff4030',
  noData: '#c42a24',
  neutralFill: '#243430',
  neutralStroke: '#5a6e64',
}

export const LIGHT_HUES: HuePalette = {
  // The dark hues at a lighter L; only lightness differs from DARK_HUES.
  fill: ['#e6c3b2', '#e2dfb6', '#b6e2d0'],
  stroke: ['#b7501f', '#aa952c', '#44926b'],
  violation: '#b52721',
  violationHot: '#f01300',
  noData: '#b52721',
  neutralFill: '#c6dbd5',
  neutralStroke: '#587f75',
}

function hexChannel(value: string): number {
  const n = value.trim()
  const six = /^#([0-9a-fA-F]{6})$/.exec(n)
  if (six) return parseInt(six[1]!, 16)
  const three = /^#([0-9a-fA-F]{3})$/.exec(n)
  if (three) {
    const [r, g, b] = three[1]!
    return parseInt(`${r}${r}${g}${g}${b}${b}`, 16)
  }
  return -1
}

/** `#rrggbb` for a color the host theme may give as hex, rgb() or hsl(). */
function normalize(value: string): string | null {
  const text = value.trim()
  if (text === '') return null
  if (hexChannel(text) >= 0) {
    const n = hexChannel(text)
    return `#${n.toString(16).padStart(6, '0')}`
  }
  const rgb = /^rgba?\(\s*(\d+)[\s,]+(\d+)[\s,]+(\d+)/i.exec(text)
  if (rgb) {
    const n = (Number(rgb[1]) << 16) | (Number(rgb[2]) << 8) | Number(rgb[3])
    return `#${n.toString(16).padStart(6, '0')}`
  }
  return null
}

function mix(a: string, b: string, t: number): string {
  const ca = hexChannel(a)
  const cb = hexChannel(b)
  if (ca < 0 || cb < 0) return a
  const lerp = (shift: number): number => {
    const x = (ca >> shift) & 0xff
    const y = (cb >> shift) & 0xff
    return Math.round(x + (y - x) * t)
  }
  return `#${((lerp(16) << 16) | (lerp(8) << 8) | lerp(0)).toString(16).padStart(6, '0')}`
}

/** RGB hue angle in degrees, for tests and for the ADR's hue-fixed claim. */
export function hueOf(color: string): number {
  const n = hexChannel(color)
  if (n < 0) throw new Error(`not a hex color: ${color}`)
  const r = ((n >> 16) & 0xff) / 255
  const g = ((n >> 8) & 0xff) / 255
  const b = (n & 0xff) / 255
  const max = Math.max(r, g, b)
  const min = Math.min(r, g, b)
  const delta = max - min
  if (delta === 0) return 0
  const hue = max === r ? ((g - b) / delta) % 6 : max === g ? (b - r) / delta + 2 : (r - g) / delta + 4
  return ((hue * 60) % 360 + 360) % 360
}

export type ThemeMode = 'light' | 'dark'

export function isDarkTheme(el: HTMLElement): boolean {
  const body = el.ownerDocument?.body
  if (!body) return false
  if (body.dataset.dsDarkTheme !== undefined) return true
  const theme = body.dataset.theme
  return theme !== undefined && theme.toLowerCase() === 'dark'
}

export interface Theme {
  mode: ThemeMode
  /** Resolve a frame color role (grade/ratio only matter for ramp roles). */
  frame(role: string, grade?: number | null, ratio?: number | null): string
}

/** Package ramp alphas, 0-255, from `frame::theme` (docs/scene-frame.md). */
const PACKAGE_FILL_ALPHA = 80
const PACKAGE_STROKE_ALPHA = 170

/** `rgba(r, g, b, a)` for a `#rrggbb` ramp color and a 0-255 alpha. */
function withAlpha(color: string, alpha: number): string {
  const n = hexChannel(color)
  if (n < 0) return color
  return `rgba(${(n >> 16) & 0xff}, ${(n >> 8) & 0xff}, ${n & 0xff}, ${Number((alpha / 255).toFixed(4))})`
}

function quality(grade: number | null, ramp: readonly [string, string, string], neutral: string): string {
  if (grade === null) return neutral
  const g = Math.min(10, Math.max(0, grade))
  return g <= 5 ? mix(ramp[0], ramp[1], g / 5) : mix(ramp[1], ramp[2], (g - 5) / 5)
}

/**
 * Resolve every role once for one panel element and the current theme. The
 * token values are snapshotted at this call: the panel repaints on a theme
 * change, so one snapshot per paint is exactly the lifetime needed.
 */
export function resolveTheme(el: HTMLElement): Theme {
  const mode: ThemeMode = isDarkTheme(el) ? 'dark' : 'light'
  const hues = mode === 'dark' ? DARK_HUES : LIGHT_HUES
  const style = getComputedStyle(el)
  const cache = new Map<string, string | null>()
  const read = (token: string): string | null => {
    if (!cache.has(token)) cache.set(token, normalize(style.getPropertyValue(token)))
    return cache.get(token) ?? null
  }
  // Read every mapped token now, so the snapshot cannot drift mid-paint.
  for (const token of Object.values(TOKENS)) if (token) read(token)
  const frame = (role: string, grade: number | null = null, ratio: number | null = null): string => {
    switch (role) {
      case 'quality-fill':
        return quality(grade, hues.fill, hues.neutralFill)
      case 'package-fill':
        // A component box is the quality ramp seen through (frame::theme: alpha 80).
        return withAlpha(quality(grade, hues.fill, hues.neutralFill), PACKAGE_FILL_ALPHA)
      case 'quality-stroke':
        return quality(grade, hues.stroke, hues.neutralStroke)
      case 'package-stroke':
        return withAlpha(quality(grade, hues.stroke, hues.neutralStroke), PACKAGE_STROKE_ALPHA)
      case 'violation':
        return hues.violation
      case 'violation-hot':
        return hues.violationHot
      case 'no-data':
        return hues.noData
      case 'coverage':
        if (ratio === null) return read(TOKENS.relation!) ?? read(TOKENS.ink!) ?? ''
        if (ratio >= 0.8) return read(TOKENS.good!) ?? read(TOKENS.ink!) ?? ''
        if (ratio >= 0.5) return read(TOKENS.accent!) ?? read(TOKENS.ink!) ?? ''
        return read(TOKENS.coverage!) ?? read(TOKENS.ink!) ?? ''
      default: {
        const token = TOKENS[role]
        if (token === null || token === undefined) {
          // An unknown role draws as ink (docs/scene-frame.md).
          return read(TOKENS.ink!) ?? ''
        }
        return read(token) ?? read(TOKENS.ink!) ?? ''
      }
    }
  }
  return { mode, frame }
}
