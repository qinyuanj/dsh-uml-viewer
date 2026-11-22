import type { SceneFrame } from '@dsh-uml-viewer/contracts'

export const CHAR_W = 8.0

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
  if (codePoint >= 0x10000) return 2
  for (const [lo, hi] of WIDE) {
    if (codePoint >= lo && codePoint <= hi) return 2
  }
  return 1
}

export function textWidth16(text: string): number {
  let n = 0
  for (const c of text) {
    n += units(c.codePointAt(0)!)
  }
  return n
}

export function estimateW(text: string, size: number): number {
  return (textWidth16(text) * CHAR_W * size) / 16.0
}

export function fittedSize(
  measure: (size: number, text: string) => number,
  size: number,
  text: string,
  maxW: number,
  bold: boolean,
  italic: boolean,
): number {
  const width = (s: number) => measure(s, text) + (bold ? s * 0.05 : 0) + (italic ? s * 0.2 : 0)
  const nominal = Math.max(1, Math.floor(size))
  const natural = width(nominal)
  if (natural <= maxW || natural <= 0) return nominal
  let px = Math.min(Math.floor((nominal * Math.max(0, maxW)) / natural), nominal - 1)
  while (px >= 1 && width(px) > maxW) px -= 1
  return px
}

export function fontPx(size: number, zoom: number): number {
  return Math.max(1, Math.round(size * zoom))
}

export function cornerPx(radius: number, zoom: number): number {
  const r = radius * zoom
  return r < 2 ? 0 : r
}

export function estimatedMeasure(text: string): (size: number, t: string) => number {
  const w = textWidth16(text)
  return (size: number) => (w * CHAR_W * size) / 16.0
}

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

export interface ViewTransform {
  x: number
  y: number
  zoom: number
  minWorld: number
}

export function viewTransform(frame: SceneFrame, _width: number, _height: number): ViewTransform {
  const zoom = frame.camera.zoom
  return {
    x: frame.camera.x,
    y: frame.camera.y,
    zoom,
    minWorld: frame.style.minTextPx / zoom,
  }
}

export function worldToScreen(v: ViewTransform, x: number, y: number): [number, number] {
  return [(x - v.x) * v.zoom, (y - v.y) * v.zoom]
}

export function screenToWorld(v: ViewTransform, sx: number, sy: number): [number, number] {
  return [sx / v.zoom + v.x, sy / v.zoom + v.y]
}

export function cssToWorld(
  frame: SceneFrame,
  bounds: { width: number; height: number },
  cssX: number,
  cssY: number,
): [number, number] | null {
  if (!(bounds.width > 0) || !(bounds.height > 0)) return null
  const fx = cssX * frame.viewport.width / bounds.width
  const fy = cssY * frame.viewport.height / bounds.height
  return screenToWorld(viewTransform(frame, frame.viewport.width, frame.viewport.height), fx, fy)
}

export function rectVisible(v: ViewTransform, w: number, h: number): boolean {
  return w * v.zoom >= 1 && h * v.zoom >= 1
}
