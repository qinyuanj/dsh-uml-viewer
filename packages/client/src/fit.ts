/**
 * Text width estimation and fitting, a port of `crates/frame/src/commands.rs`.
 * The producer writes `maxW` and `size` in the frame; the client only measures
 * or estimates to decide if the text must shrink to fit, and by how much.
 *
 * Text width is a pure estimate — `CHAR_W` half-width units at the 16 px body
 * font, CJK and astral characters count double (`crates/scene/src/layout.rs`)
 * — so layout never depends on a loaded font. The real measure comes from the
 * loaded Sarasa face when the font is available; otherwise this same estimate
 * is used, which stays *wider* than the real face for the CJK characters the
 * estimate overcounts, so boxes never grow beyond what layout fixed.
 */
import type { SceneFrame } from '@dsh-uml-viewer/contracts'

/** Estimated advance of one half-width character at the 16 px body font. */
export const CHAR_W = 8.0

// East Asian Wide/Fullwidth BMP blocks: CJK, kana, Hangul, full-width forms.
const WIDE: readonly [number, number][] = [
  [0x1100, 0x115f],
  [0x2e80, 0x303e],
  [0x3041, 0x33ff],
  [0x3400, 0x4dbf],
  [0x4e00, 0x9fff],
  [0xa000, 0xa4cf],
  [0xac00, 0xd7a3],
  [0xf900, 0xfaff],
  [0xfe30, 0xfe4f],
  [0xff00, 0xff60],
  [0xffe0, 0xffe6],
]

function units(codePoint: number): number {
  if (codePoint >= 0x10000) return 2 // astral: one JVM UTF-16 surrogate pair
  for (const [lo, hi] of WIDE) {
    if (codePoint >= lo && codePoint <= hi) return 2
  }
  return 1
}

/** UTF-16 unit count, except full-width CJK counts double (scene::text_w). */
export function textWidth16(text: string): number {
  let n = 0
  for (const c of text) {
    n += units(c.codePointAt(0)!)
  }
  return n
}

/** The layout's 8 px/char monospace estimate at 16 px, scaled. */
export function estimateW(text: string, size: number): number {
  return (textWidth16(text) * CHAR_W * size) / 16.0
}

/**
 * The producer's Rust `fit` shrinks until real measured text + bold offset +
 * italic shear fit inside `maxW`. This port is used only when the Sarasa
 * face is available to give a real measure; the result is always at most
 * `size`, never grows.
 */
export function fittedSize(
  measure: (size: number, text: string) => number,
  size: number,
  text: string,
  maxW: number,
  bold: boolean,
  italic: boolean,
): number {
  const width = (s: number) => measure(s, text) + (bold ? s * 0.05 : 0) + (italic ? s * 0.2 : 0)
  const natural = width(size)
  if (natural <= maxW || natural <= 0) return size
  let lo = 0
  let hi = size
  // 32 bisections land within a quarter-point; the draw happens at screen
  // resolution, so that is plenty.
  for (let i = 0; i < 32; i += 1) {
    const mid = (lo + hi) / 2
    if (width(mid) > maxW) hi = mid
    else lo = mid
  }
  return lo
}

/**
 * The fallback measure when the packaged face has not loaded yet. Using the
 * estimate here is conservative for CJK because the estimate counts every CJK
 * character as two halfwidth units while the real face is narrower for most
 * such characters; the fitted size is therefore never *larger* than what the
 * real face would produce, so text never overflows a box laid out to `maxW`.
 */
export function estimatedMeasure(text: string): (size: number, t: string) => number {
  const w = textWidth16(text)
  return (size: number) => (w * CHAR_W * size) / 16.0
}

/** One Canvas 2D measurement channel, shared by every fitted call in a paint. */
export class Measure {
  private readonly canvas: CanvasRenderingContext2D

  public constructor(canvas: CanvasRenderingContext2D) {
    this.canvas = canvas
  }

  public measure(size: number, text: string): number {
    this.canvas.font = `${size}px "UML Viewer Sarasa", monospace`
    return this.canvas.measureText(text).width
  }
}

/** One viewport transform for the whole frame. */
export interface ViewTransform {
  x: number
  y: number
  zoom: number
  /** world-space minimum feature size below which world points collapse */
  minWorld: number
}

export function viewTransform(frame: SceneFrame, width: number, height: number): ViewTransform {
  const zoom = frame.camera.zoom
  const cx = frame.camera.x
  const cy = frame.camera.y
  return {
    x: cx - width / 2 / zoom,
    y: cy - height / 2 / zoom,
    zoom,
    minWorld: frame.style.minTextPx / zoom,
  }
}

/** World → screen, keeping the frame's origin at the top-left of the viewport. */
export function worldToScreen(v: ViewTransform, x: number, y: number): [number, number] {
  return [(x - v.x) * v.zoom, (y - v.y) * v.zoom]
}

/** Screen → world. */
export function screenToWorld(v: ViewTransform, sx: number, sy: number): [number, number] {
  return [sx / v.zoom + v.x, sy / v.zoom + v.y]
}

/** A world-space rect of the given size in world units is at least 0.5 screen pixels on screen. */
export function rectVisible(v: ViewTransform, w: number, h: number): boolean {
  return w * v.zoom >= 0.5 && h * v.zoom >= 0.5
}
