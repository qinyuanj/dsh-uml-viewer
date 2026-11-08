/**
 * Canvas 2D scene-frame painter. The frame is the producer's documented
 * contract (docs/scene-frame.md); this module only *draws* it, never re-derives
 * geometry. Colors resolve through host theme tokens where the theme supplies
 * one, so the panel follows light/dark; the quality and violation roles keep
 * their hue and only change lightness with the mode.
 */
import type { SceneFrame } from '@dsh-uml-viewer/contracts'
import { fittedSize, Measure, rectVisible, viewTransform, worldToScreen, type ViewTransform } from './fit.js'
import { resolveTheme } from './theme.js'

export const FONT_STACK = '"UML Viewer Sarasa", "Sarasa Mono SC", monospace'

interface Painter {
  ctx: CanvasRenderingContext2D
  view: ViewTransform
  width: number
  height: number
  theme: ReturnType<typeof resolveTheme>
  measure: Measure
  frame: SceneFrame
}

function dimAlpha(frame: SceneFrame): number {
  // The producer's dimAlpha is 0-255 in the frame's style.
  return Math.max(0, Math.min(1, frame.style.dimAlpha / 255))
}

function roundRect(ctx: CanvasRenderingContext2D, x: number, y: number, w: number, h: number, r: number): void {
  const rr = Math.max(0, Math.min(r, Math.min(w, h) / 2))
  ctx.beginPath()
  ctx.moveTo(x + rr, y)
  ctx.arcTo(x + w, y, x + w, y + h, rr)
  ctx.arcTo(x + w, y + h, x, y + h, rr)
  ctx.arcTo(x, y + h, x, y, rr)
  ctx.arcTo(x, y, x + w, y, rr)
  ctx.closePath()
}

function polyline(ctx: CanvasRenderingContext2D, view: ViewTransform, points: readonly (readonly [number, number])[]): void {
  ctx.beginPath()
  let first = true
  for (const point of points) {
    const [sx, sy] = worldToScreen(view, point[0], point[1])
    if (first) {
      ctx.moveTo(sx, sy)
      first = false
    } else {
      ctx.lineTo(sx, sy)
    }
  }
}

function arrowheadOpen(ctx: CanvasRenderingContext2D, tx: number, ty: number, bx: number, by: number, len: number): void {
  const angle = Math.atan2(ty - by, tx - bx)
  const spread = (20 * Math.PI) / 180
  for (const sign of [-1, 1]) {
    ctx.beginPath()
    ctx.moveTo(tx, ty)
    ctx.lineTo(tx - len * Math.cos(angle + sign * spread), ty - len * Math.sin(angle + sign * spread))
    ctx.stroke()
  }
}

/** The closed-head outline; the caller fills it before stroking. */
function headPath(
  ctx: CanvasRenderingContext2D,
  tx: number,
  ty: number,
  bx: number,
  by: number,
  len: number,
  diamond: boolean,
): void {
  const angle = Math.atan2(ty - by, tx - bx)
  const spread = (20 * Math.PI) / 180
  ctx.beginPath()
  ctx.moveTo(tx, ty)
  ctx.lineTo(tx - len * Math.cos(angle - spread), ty - len * Math.sin(angle - spread))
  if (diamond) {
    // The diamond's back vertex sits 16·zoom behind the tip.
    ctx.lineTo(tx - len * Math.cos(angle), ty - len * Math.sin(angle))
  }
  ctx.lineTo(tx - len * Math.cos(angle + spread), ty - len * Math.sin(angle + spread))
  ctx.closePath()
}

function text(
  p: Painter,
  value: string,
  x: number,
  y: number,
  px: number,
  color: string,
  opts: { bold?: boolean; italic?: boolean; align?: 'left' | 'center' | 'right' } = {},
): void {
  const { ctx } = p
  if (px < p.frame.style.minTextPx) return
  ctx.font = `${opts.italic ? 'italic ' : ''}${opts.bold ? 'bold ' : ''}${px}px ${FONT_STACK}`
  ctx.fillStyle = color
  ctx.textBaseline = 'middle'
  ctx.textAlign = opts.align ?? 'left'
  ctx.fillText(value, x, y)
  if (opts.bold && (opts.align ?? 'left') === 'left') {
    // Bold is a second pass offset by 0.05·px, as the producer's frame does.
    ctx.fillText(value, x + px * 0.05, y)
  }
}

/** A fitted text draw: shrink to `maxW` when the face needs it. */
function fittedText(
  p: Painter,
  value: string,
  x: number,
  y: number,
  size: number,
  maxW: number,
  color: string,
  opts: { bold?: boolean; italic?: boolean; align?: 'left' | 'center' | 'right'; zoom?: number } = {},
): void {
  const zoom = opts.zoom ?? p.view.zoom
  const fitted = fittedSize(p.measure.measure.bind(p.measure), size, value, maxW, opts.bold ?? false, opts.italic ?? false)
  text(p, value, x, y, fitted * zoom, color, opts)
}

export function paintFrame(frame: SceneFrame, canvas: HTMLCanvasElement): void {
  const ctx = canvas.getContext('2d')
  if (!ctx) return
  const doc = canvas.ownerDocument
  const dpr = doc?.defaultView?.devicePixelRatio ?? 1
  const pixelWidth = Math.max(1, Math.round(frame.viewport.width * dpr))
  const pixelHeight = Math.max(1, Math.round(frame.viewport.height * dpr))
  if (canvas.width !== pixelWidth) canvas.width = pixelWidth
  if (canvas.height !== pixelHeight) canvas.height = pixelHeight
  ctx.setTransform(dpr, 0, 0, dpr, 0, 0)

  const theme = resolveTheme(canvas)
  const view = viewTransform(frame, frame.viewport.width, frame.viewport.height)
  const p: Painter = {
    ctx, view, width: frame.viewport.width, height: frame.viewport.height, theme,
    measure: new Measure(ctx), frame,
  }

  ctx.globalAlpha = 1
  ctx.setLineDash([])
  ctx.fillStyle = theme.frame('canvas')
  ctx.fillRect(0, 0, p.width, p.height)

  if (frame.waiting) {
    text(p, 'No diagram yet — run uml_ir, then uml_display.', 16, p.height / 2, frame.style.fonts.chrome,
      theme.frame('muted'))
    return
  }

  for (const item of frame.world) paintWorld(p, item)
  for (const edge of frame.edges) paintEdge(p, edge)
  for (const item of frame.chrome) paintChrome(p, item)
}

function paintWorld(p: Painter, item: SceneFrame['world'][number]): void {
  const { ctx, view, theme, frame } = p
  const lod = frame.lod
  const atOrBelow = (tier: 'blocks' | 'frames' | 'full'): boolean =>
    lod === 'blocks' ? tier === 'blocks' : lod === 'frames' ? tier !== 'full' : true
  const colorOf = (c: { role: string; grade?: number | null; ratio?: number | null }): string =>
    theme.frame(c.role, c.grade ?? null, c.ratio ?? null)

  switch (item.kind) {
    case 'package': {
      if (!atOrBelow('blocks')) return
      const [sx, sy] = worldToScreen(view, item.rect.x, item.rect.y)
      const sw = item.rect.w * view.zoom
      const sh = item.rect.h * view.zoom
      if (!rectVisible(view, item.rect.w, item.rect.h)) return
      const radius = frame.style.radius.package * view.zoom
      ctx.setLineDash(item.missing ? [frame.style.widths.dash, frame.style.widths.dashGap] : [])
      ctx.globalAlpha = item.dimmed ? dimAlpha(frame) : 1
      ctx.fillStyle = colorOf(item.fill)
      ctx.strokeStyle = colorOf(item.selected ? { role: 'selected' } : item.stroke)
      ctx.lineWidth = item.strokeWidth
      roundRect(ctx, sx, sy, sw, sh, Math.max(0, Math.min(radius, sw / 2, sh / 2)))
      ctx.fill()
      ctx.stroke()
      ctx.setLineDash([])
      if (item.text !== null) {
        // The title dims with its box, as the native ink(*title, *dimmed).
        const [tx, ty] = worldToScreen(view, item.rect.x + 12, item.rect.y + 8)
        fittedText(p, item.text, tx, ty + (item.size * view.zoom) / 2, item.size, item.maxW,
          colorOf(item.ink), { italic: true, zoom: view.zoom })
      }
      ctx.globalAlpha = 1
      return
    }
    case 'class': {
      if (!atOrBelow('blocks')) return
      if (!rectVisible(view, item.rect.w, item.rect.h)) return
      const [sx, sy] = worldToScreen(view, item.rect.x, item.rect.y)
      const sw = item.rect.w * view.zoom
      const sh = item.rect.h * view.zoom
      ctx.setLineDash(item.missing ? [frame.style.widths.dash, frame.style.widths.dashGap] : [])
      ctx.globalAlpha = item.dimmed ? dimAlpha(frame) : 1
      ctx.fillStyle = colorOf(item.fill)
      ctx.strokeStyle = colorOf(item.selected ? { role: 'selected' } : item.stroke)
      ctx.lineWidth = item.strokeWidth
      if (item.shape === 'oval') {
        ctx.beginPath()
        ctx.ellipse(sx + sw / 2, sy + sh / 2, sw / 2, sh / 2, 0, 0, Math.PI * 2)
      } else {
        roundRect(ctx, sx, sy, sw, sh, Math.max(0, Math.min(frame.style.radius.class * view.zoom, sw / 2, sh / 2)))
      }
      ctx.fill()
      ctx.stroke()
      ctx.setLineDash([])
      ctx.globalAlpha = 1
      return
    }
    case 'quality': {
      if (!atOrBelow('frames')) return
      const [cx, cy] = worldToScreen(view, item.center[0], item.center[1])
      const radius = item.radius * view.zoom
      if (radius < 0.5) return
      ctx.globalAlpha = item.dimmed ? dimAlpha(frame) : 1
      ctx.fillStyle = colorOf(item.fill)
      ctx.strokeStyle = colorOf(item.stroke)
      ctx.lineWidth = Math.max(1, frame.style.widths.dot)
      ctx.beginPath()
      ctx.arc(cx, cy, radius, 0, Math.PI * 2)
      ctx.fill()
      ctx.stroke()
      if (lod === 'full') {
        // The letter dims with its badge, as the native ink(*letter, *dimmed).
        text(p, item.text, cx, cy, frame.style.fonts.badge * view.zoom, colorOf(item.ink), { align: 'center' })
      }
      ctx.globalAlpha = 1
      return
    }
    case 'marker':
    case 'level': {
      if (lod !== 'full') return
      const [sx, sy] = worldToScreen(view, item.at[0], item.at[1])
      ctx.globalAlpha = item.dimmed ? dimAlpha(frame) : 1
      text(p, item.text, sx, sy + (item.size * view.zoom) / 2, item.size * view.zoom, colorOf(item.ink))
      ctx.globalAlpha = 1
      return
    }
    case 'band':
    case 'rule': {
      if (lod !== 'full') return
      const [fx, fy] = worldToScreen(view, item.from[0], item.from[1])
      const [tx, ty] = worldToScreen(view, item.to[0], item.to[1])
      ctx.globalAlpha = item.dimmed ? dimAlpha(frame) : 1
      ctx.strokeStyle = colorOf(item.stroke)
      ctx.lineWidth = item.kind === 'band' ? frame.style.widths.band : frame.style.widths.rule
      ctx.beginPath()
      ctx.moveTo(fx, fy)
      ctx.lineTo(tx, ty)
      ctx.stroke()
      ctx.globalAlpha = 1
      return
    }
    case 'text': {
      if (item.line === 'name' ? !atOrBelow('frames') : lod !== 'full') return
      const size = item.slot * view.zoom
      if (size < frame.style.minTextPx) return
      const [sx, sy] = worldToScreen(view, item.center ? item.x + item.maxW / 2 : item.x, item.top + item.slot / 2)
      ctx.globalAlpha = item.dimmed ? dimAlpha(frame) : 1
      fittedText(p, item.text ?? '', sx, sy, item.slot, item.maxW, colorOf(item.ink),
        { bold: item.bold, italic: item.italic, align: item.center ? 'center' : 'left' })
      ctx.globalAlpha = 1
      return
    }
    case 'no-data': {
      if (lod !== 'full') return
      const [sx, sy] = worldToScreen(view, item.right, item.y)
      ctx.globalAlpha = item.dimmed ? dimAlpha(frame) : 1
      text(p, item.text, sx, sy, item.size * view.zoom, colorOf(item.ink), { align: 'right' })
      ctx.globalAlpha = 1
      return
    }
    case 'child-wash': {
      if (lod !== 'full') return
      const [sx, sy] = worldToScreen(view, item.rect.x, item.rect.y)
      const sw = item.rect.w * view.zoom
      const sh = item.rect.h * view.zoom
      ctx.globalAlpha = item.dimmed ? dimAlpha(frame) : 1
      ctx.fillStyle = colorOf(item.marker.wash)
      roundRect(ctx, sx, sy, sw, sh, Math.max(0, frame.style.radius.rowWash * view.zoom))
      ctx.fill()
      ctx.fillStyle = colorOf(item.marker.bar)
      ctx.fillRect(sx, sy, item.marker.barWidth * view.zoom, sh)
      ctx.globalAlpha = 1
      return
    }
    case 'port-link': {
      if (lod !== 'full') return
      const [fx, fy] = worldToScreen(view, item.behind[0], item.behind[1])
      const [tx, ty] = worldToScreen(view, item.tip[0], item.tip[1])
      ctx.globalAlpha = item.dimmed ? dimAlpha(frame) : 1
      ctx.strokeStyle = colorOf(item.stroke)
      ctx.lineWidth = Math.max(1, frame.style.widths.portLink)
      ctx.beginPath()
      ctx.moveTo(fx, fy)
      ctx.lineTo(tx, ty)
      ctx.stroke()
      arrowheadOpen(ctx, tx, ty, fx, fy, 10 * view.zoom)
      ctx.globalAlpha = 1
      return
    }
    case 'port': {
      if (lod !== 'full') return
      const [sx, sy] = worldToScreen(view, item.rect.x, item.rect.y)
      const sw = item.rect.w * view.zoom
      const sh = item.rect.h * view.zoom
      ctx.globalAlpha = item.dimmed ? dimAlpha(frame) : 1
      ctx.fillStyle = colorOf(item.marked ? { role: 'port-fill-marked' } : item.fill)
      ctx.strokeStyle = colorOf(item.stroke)
      ctx.lineWidth = item.strokeWidth
      roundRect(ctx, sx, sy, sw, sh, Math.max(0, Math.min(frame.style.radius.port * view.zoom, sw / 2, sh / 2)))
      ctx.fill()
      ctx.stroke()
      fittedText(p, item.text, sx + sw / 2, sy + sh / 2, frame.style.fonts.port, item.rect.w - 6,
        colorOf(item.ink), { align: 'center' })
      ctx.globalAlpha = 1
      return
    }
    default:
      // An additively added kind the renderer does not know: skip it.
      return
  }
}

function paintEdge(p: Painter, edge: SceneFrame['edges'][number]): void {
  const { ctx, view, theme, frame } = p
  const lod = frame.lod
  const colorOf = (c: { role: string; grade?: number | null; ratio?: number | null }): string =>
    theme.frame(c.role, c.grade ?? null, c.ratio ?? null)

  if (edge.kind === 'stroke') {
    if (lod === 'blocks') return
    ctx.globalAlpha = edge.dimmed ? dimAlpha(frame) : 1
    ctx.strokeStyle = colorOf(edge.stroke)
    ctx.lineWidth = edge.width
    polyline(ctx, view, edge.points)
    ctx.stroke()
    ctx.globalAlpha = 1
    return
  }
  if (edge.kind === 'head') {
    if (lod !== 'full') return
    const [tx, ty] = worldToScreen(view, edge.tip[0], edge.tip[1])
    const [bx, by] = worldToScreen(view, edge.behind[0], edge.behind[1])
    const len = 16 * view.zoom
    const dim = edge.dimmed ? dimAlpha(frame) : 1
    ctx.strokeStyle = colorOf(edge.stroke)
    ctx.lineWidth = edge.width
    if (edge.head === 'open') {
      ctx.globalAlpha = dim
      arrowheadOpen(ctx, tx, ty, bx, by, len)
    } else {
      // A closed head's interior stays fully opaque: the canvas or ink fill masks
      // the shaft that runs through to the tip, and only the outline dims
      // (docs/scene-frame.md, "Edges"). Never lower alpha for the fill.
      headPath(ctx, tx, ty, bx, by, len, edge.head !== 'triangle')
      ctx.globalAlpha = 1
      ctx.fillStyle = colorOf(edge.fill)
      ctx.fill()
      ctx.globalAlpha = dim
      ctx.stroke()
    }
    ctx.globalAlpha = 1
    return
  }
  if (lod === 'blocks') return
  ctx.globalAlpha = edge.dimmed ? dimAlpha(frame) : 1
  ctx.fillStyle = colorOf(edge.fill)
  ctx.strokeStyle = ctx.fillStyle
  ctx.lineWidth = 1
  polyline(ctx, view, edge.points)
  ctx.closePath()
  ctx.fill()
  ctx.stroke()
  ctx.globalAlpha = 1
}

function paintChrome(p: Painter, item: SceneFrame['chrome'][number]): void {
  const { ctx, theme, width, height, frame } = p
  const colorOf = (c: { role: string; grade?: number | null; ratio?: number | null }): string =>
    theme.frame(c.role, c.grade ?? null, c.ratio ?? null)

  switch (item.kind) {
    case 'title':
    case 'proposal-banner':
    case 'back': {
      const [x, y] = item.at
      const px = frame.style.fonts.chrome
      ctx.font = `${px}px ${FONT_STACK}`
      const measured = ctx.measureText(item.text).width
      ctx.fillStyle = colorOf(item.plate)
      roundRect(ctx, x - 6, y - 2, measured + 12, px + 4, 3)
      ctx.fill()
      text(p, item.text, x, y + px / 2, px, colorOf(item.ink))
      return
    }
    case 'status': {
      const [x, y] = item.at
      text(p, item.text, x, y, frame.style.fonts.chrome, colorOf(item.ink))
      return
    }
    case 'hint': {
      text(p, item.text, width / 2, height / 2, frame.style.fonts.chrome, colorOf(item.ink), { align: 'center' })
      return
    }
    case 'popup': {
      ctx.fillStyle = colorOf(item.fill)
      ctx.strokeStyle = colorOf(item.border)
      ctx.lineWidth = 1
      roundRect(ctx, item.rect.x, item.rect.y, item.rect.w, item.rect.h, 4)
      ctx.fill()
      ctx.stroke()
      item.rows.forEach((row, index) => {
        text(p, row.text, item.rect.x + 8, item.rect.y + 8 + 20 * index, 14, colorOf(row.ink))
      })
      return
    }
    default:
      return
  }
}

/** A hit test over the frame's item bounds, for pointer selection. */
export function hitTest(frame: SceneFrame, world: readonly [number, number]): string | null {
  for (let index = frame.world.length - 1; index >= 0; index -= 1) {
    const item = frame.world[index]!
    const bounds = item.bounds
    if (!bounds) continue
    if (world[0] >= bounds.x && world[0] <= bounds.x + bounds.w && world[1] >= bounds.y && world[1] <= bounds.y + bounds.h) {
      if (item.kind === 'package' || item.kind === 'class' || item.kind === 'port') return item.id
    }
  }
  return null
}
