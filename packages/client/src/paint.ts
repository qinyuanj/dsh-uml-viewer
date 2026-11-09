/**
 * Canvas 2D scene-frame painter. The frame is the producer's documented
 * contract (docs/scene-frame.md); this module only *draws* it, never re-derives
 * geometry. Colors resolve through host theme tokens where the theme supplies
 * one, so the panel follows light/dark; the quality and violation roles keep
 * their hue and only change lightness with the mode.
 *
 * World primitives follow the native egui painter (crates/viewer/src/paint.rs)
 * rule for rule: font sizes on whole pixels, corner radii dropped under 2 px,
 * rect outlines stroked inside the rect, unmeasured outlines dashed square,
 * foreign-package ellipses never dashed, and at the overview tiers class names
 * painted after every box.
 */
import type { SceneFrame } from '@dsh-uml-viewer/contracts'
import { cornerPx, fittedSize, fontPx, Measure, rectVisible, viewTransform, worldToScreen, type ViewTransform } from './fit.js'
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
  /** The tier of the zoom on screen (a local pan/zoom keeps the producer's items). */
  lod: SceneFrame['lod']
  /** Overview class names, drawn after every world item (native `names`). */
  deferred: (() => void)[] | null
}

/**
 * The tier for the camera's zoom (`style.lodZoom`, like the native
 * `lod::tier`). The producer tags every item with its coarsest tier, so a
 * frame drawn after a local wheel zoom paints exactly the window's tier.
 */
export function tierOf(frame: SceneFrame): SceneFrame['lod'] {
  const zoom = frame.camera.zoom > 0 ? frame.camera.zoom : 1
  if (zoom < frame.style.lodZoom.frames) return 'blocks'
  if (zoom < frame.style.lodZoom.full) return 'frames'
  return 'full'
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

/** egui `rect_filled` + `rect_stroke(StrokeKind::Inside)`, or `dashed_rect` when unmeasured. */
function box(
  p: Painter,
  x: number,
  y: number,
  w: number,
  h: number,
  corner: number,
  fill: string,
  stroke: string,
  lineWidth: number,
  dashed: boolean,
): void {
  const { ctx, frame } = p
  ctx.setLineDash([])
  ctx.fillStyle = fill
  roundRect(ctx, x, y, w, h, corner)
  ctx.fill()
  ctx.strokeStyle = stroke
  ctx.lineWidth = lineWidth
  if (dashed) {
    // The four edges of the rect, dash and gap in screen points, square corners.
    ctx.setLineDash([frame.style.widths.dash, frame.style.widths.dashGap])
    ctx.beginPath()
    ctx.moveTo(x, y)
    ctx.lineTo(x + w, y)
    ctx.lineTo(x + w, y + h)
    ctx.lineTo(x, y + h)
    ctx.closePath()
    ctx.stroke()
    ctx.setLineDash([])
    return
  }
  // An inside stroke: the centre line runs half the width in from the edge.
  const inset = Math.min(lineWidth / 2, w / 2, h / 2)
  roundRect(ctx, x + inset, y + inset, w - 2 * inset, h - 2 * inset, Math.max(0, corner - inset))
  ctx.stroke()
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

/**
 * Mid-tier edge strokes (`render_cache::decimated` + `lod::simplify`): at the
 * `frames` tier drop interior points closer than `edgeDecimatePx` on screen,
 * and those within 0.5 screen points of the chord from the last kept point to
 * their successor. Ends always survive. The budget follows the quarter-octave
 * zoom step, not the raw zoom, so pan and small zooms keep the same polyline.
 */
function decimateEdgePoints(frame: SceneFrame, zoom: number, points: readonly (readonly [number, number])[]): readonly (readonly [number, number])[] {
  if (tierOf(frame) !== 'frames' || points.length <= 2) return points
  const octave = Math.log2(zoom) * 4
  // Rust f64::round rounds negative halfway values away from zero.
  const step = Math.sign(octave) * Math.round(Math.abs(octave))
  const stepZoom = Math.pow(2, step / 4)
  const minDist = frame.style.edgeDecimatePx / stepZoom
  const tolerance = 0.5 / stepZoom
  const out = [points[0]!]
  for (let i = 1; i < points.length - 1; i += 1) {
    const last = out[out.length - 1]!
    const p = points[i]!
    const next = points[i + 1]!
    const [px, py] = [p[0] - last[0], p[1] - last[1]]
    if (px * px + py * py < minDist * minDist) continue
    const [dx, dy] = [next[0] - last[0], next[1] - last[1]]
    const len = Math.sqrt(dx * dx + dy * dy)
    if (len > 0 && Math.abs(px * dy - py * dx) / len < tolerance) continue
    out.push(p)
  }
  out.push(points[points.length - 1]!)
  return out
}

/**
 * A stroke at the tier the native painter draws as one unfeathered quad mesh
 * (`MESH_EDGE_SCALE`): each segment is its own butt-ended quad, so no round
 * join bulges at a bend. Below that scale a 1.6 pt line needs no anti-aliasing.
 */
function segments(ctx: CanvasRenderingContext2D, view: ViewTransform, points: readonly (readonly [number, number])[]): void {
  for (let i = 1; i < points.length; i += 1) {
    const [ax, ay] = worldToScreen(view, points[i - 1]![0], points[i - 1]![1])
    const [bx, by] = worldToScreen(view, points[i]![0], points[i]![1])
    if (ax === bx && ay === by) continue
    ctx.beginPath()
    ctx.lineJoin = 'miter'
    ctx.lineCap = 'butt'
    ctx.moveTo(ax, ay)
    ctx.lineTo(bx, by)
    ctx.stroke()
  }
}

/** The native `paint_head` vertices: tip, wing A, wing C and the diamond back. */
function headVertices(tx: number, ty: number, bx: number, by: number, len: number): {
  a: [number, number]
  c: [number, number]
  back: [number, number]
} {
  let dx = tx - bx
  let dy = ty - by
  const dist = Math.sqrt(dx * dx + dy * dy)
  if (dist === 0) {
    dx = 1
    dy = 0
  } else {
    dx /= dist
    dy /= dist
  }
  // egui rotates the direction vector by ±PI/9 (20°).
  const sin = Math.sin(Math.PI / 9)
  const cos = Math.cos(Math.PI / 9)
  return {
    a: [tx - len * (dx * cos - dy * sin), ty - len * (dy * cos + dx * sin)],
    c: [tx - len * (dx * cos + dy * sin), ty - len * (dy * cos - dx * sin)],
    back: [tx - len * dx, ty - len * dy],
  }
}

function arrowheadOpen(ctx: CanvasRenderingContext2D, tx: number, ty: number, bx: number, by: number, len: number): void {
  const { a, c } = headVertices(tx, ty, bx, by, len)
  for (const [wx, wy] of [a, c]) {
    ctx.beginPath()
    ctx.moveTo(tx, ty)
    ctx.lineTo(wx, wy)
    ctx.stroke()
  }
}

/** The closed-head outline; the caller fills it before stroking. */
function headPath(ctx: CanvasRenderingContext2D, tx: number, ty: number, bx: number, by: number, len: number, diamond: boolean): void {
  const { a, c, back } = headVertices(tx, ty, bx, by, len)
  ctx.beginPath()
  ctx.moveTo(tx, ty)
  ctx.lineTo(a[0], a[1])
  if (diamond) ctx.lineTo(back[0], back[1])
  ctx.lineTo(c[0], c[1])
  ctx.closePath()
}

interface TextOptions {
  bold?: boolean
  italic?: boolean
  align?: 'left' | 'center' | 'right'
  baseline?: 'top' | 'middle'
}

/** Chrome and badge text: one draw at `px`, vertically centred on `y` by default. */
function text(p: Painter, value: string, x: number, y: number, px: number, color: string, opts: TextOptions = {}): void {
  const { ctx } = p
  if (px < p.frame.style.minTextPx) return
  ctx.font = `${opts.italic ? 'italic ' : ''}${px}px ${FONT_STACK}`
  ctx.fillStyle = color
  ctx.textBaseline = opts.baseline ?? 'middle'
  ctx.textAlign = opts.align ?? 'left'
  ctx.fillText(value, x, y)
  if (opts.bold) {
    // Bold is a second pass offset by 0.05·px, as the native draw_text does.
    ctx.fillText(value, x + px * 0.05, y)
  }
}

/**
 * Native `draw_text` for canvas labels: `y` is the top of the label, `px` a
 * whole-pixel size; with a `budget` (screen points) the label shrinks to the
 * largest whole pixel that fits, measured in the packaged Sarasa face.
 */
function label(
  p: Painter,
  value: string,
  x: number,
  y: number,
  px: number,
  color: string,
  opts: TextOptions & { budget?: number } = {},
): void {
  if (value === '' || px < 1) return
  const size = opts.budget === undefined
    ? px
    : fittedSize(p.measure.measure.bind(p.measure), px, value, opts.budget, opts.bold ?? false, opts.italic ?? false)
  text(p, value, x, y, size, color, { ...opts, baseline: 'top' })
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
  const lod = tierOf(frame)
  const p: Painter = {
    ctx, view, width: frame.viewport.width, height: frame.viewport.height, theme,
    measure: new Measure(ctx), frame, lod, deferred: lod === 'full' ? null : [],
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
  for (const draw of p.deferred ?? []) draw()
  for (const edge of frame.edges) paintEdge(p, edge)
  for (const item of frame.chrome) paintChrome(p, item)
}

function paintWorld(p: Painter, item: SceneFrame['world'][number]): void {
  const { ctx, view, theme, frame } = p
  const lod = p.lod
  const zoom = view.zoom
  const atOrBelow = (tier: 'blocks' | 'frames' | 'full'): boolean =>
    lod === 'blocks' ? tier === 'blocks' : lod === 'frames' ? tier !== 'full' : true
  const colorOf = (c: { role: string; grade?: number | null; ratio?: number | null }): string =>
    theme.frame(c.role, c.grade ?? null, c.ratio ?? null)
  const alphaOf = (dimmed: boolean): number => (dimmed ? dimAlpha(frame) : 1)

  switch (item.kind) {
    case 'package': {
      if (!atOrBelow('blocks')) return
      if (!rectVisible(view, item.rect.w, item.rect.h)) return
      const [sx, sy] = worldToScreen(view, item.rect.x, item.rect.y)
      ctx.globalAlpha = alphaOf(item.dimmed)
      box(p, sx, sy, item.rect.w * zoom, item.rect.h * zoom, cornerPx(frame.style.radius.package, zoom),
        colorOf(item.fill), colorOf(item.stroke), item.strokeWidth, item.missing)
      if (item.text !== null) {
        // The italic title at (12, 8) scaled, fitted to maxW; it dims with its box.
        label(p, item.text, sx + 12 * zoom, sy + 8 * zoom, fontPx(item.size, zoom), colorOf(item.ink),
          { italic: true, budget: item.maxW * zoom })
      }
      ctx.globalAlpha = 1
      return
    }
    case 'class': {
      if (!atOrBelow('blocks')) return
      if (!rectVisible(view, item.rect.w, item.rect.h)) return
      const [sx, sy] = worldToScreen(view, item.rect.x, item.rect.y)
      const sw = item.rect.w * zoom
      const sh = item.rect.h * zoom
      ctx.globalAlpha = alphaOf(item.dimmed)
      if (item.shape === 'oval') {
        // 外来包椭圆: a filled ellipse with a middle stroke, never dashed.
        ctx.setLineDash([])
        ctx.fillStyle = colorOf(item.fill)
        ctx.strokeStyle = colorOf(item.stroke)
        ctx.lineWidth = item.strokeWidth
        ctx.beginPath()
        ctx.ellipse(sx + sw / 2, sy + sh / 2, sw / 2, sh / 2, 0, 0, Math.PI * 2)
        ctx.fill()
        ctx.stroke()
      } else {
        box(p, sx, sy, sw, sh, cornerPx(frame.style.radius.class, zoom), colorOf(item.fill), colorOf(item.stroke),
          item.strokeWidth, item.missing)
      }
      ctx.globalAlpha = 1
      return
    }
    case 'quality': {
      if (!atOrBelow('frames')) return
      const [cx, cy] = worldToScreen(view, item.center[0], item.center[1])
      const radius = item.radius * zoom
      ctx.globalAlpha = alphaOf(item.dimmed)
      ctx.fillStyle = colorOf(item.fill)
      ctx.strokeStyle = colorOf(item.stroke)
      ctx.lineWidth = frame.style.widths.dot
      ctx.beginPath()
      ctx.arc(cx, cy, radius, 0, Math.PI * 2)
      ctx.fill()
      ctx.stroke()
      if (lod === 'full') {
        // The letter dims with its badge, as the native ink(*letter, *dimmed).
        text(p, item.text, cx, cy, fontPx(frame.style.fonts.badge, zoom), colorOf(item.ink), { align: 'center' })
      }
      ctx.globalAlpha = 1
      return
    }
    case 'marker':
    case 'level': {
      if (lod !== 'full') return
      const [sx, sy] = worldToScreen(view, item.at[0], item.at[1])
      ctx.globalAlpha = alphaOf(item.dimmed)
      label(p, item.text, sx, sy, fontPx(item.size, zoom), colorOf(item.ink))
      ctx.globalAlpha = 1
      return
    }
    case 'band':
    case 'rule': {
      if (lod !== 'full') return
      const [fx, fy] = worldToScreen(view, item.from[0], item.from[1])
      const [tx, ty] = worldToScreen(view, item.to[0], item.to[1])
      ctx.globalAlpha = alphaOf(item.dimmed)
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
      const px = fontPx(item.slot, zoom)
      if (px < frame.style.minTextPx) return
      const left = item.center ? item.x + item.maxW / 2 : item.x
      const [sx, top] = worldToScreen(view, left, item.top)
      // Centred in its slot by the unfitted size, like the native base_y.
      const sy = top + Math.max(0, (item.slot * zoom - px) / 2)
      const ink = colorOf(item.ink)
      const alpha = alphaOf(item.dimmed)
      const draw = (): void => {
        ctx.globalAlpha = alpha
        label(p, item.text ?? '', sx, sy, px, ink,
          { bold: item.bold, italic: item.italic, align: item.center ? 'center' : 'left', budget: item.maxW * zoom })
        ctx.globalAlpha = 1
      }
      if (p.deferred) p.deferred.push(draw)
      else draw()
      return
    }
    case 'no-data': {
      if (lod !== 'full') return
      const [sx, sy] = worldToScreen(view, item.right, item.y)
      ctx.globalAlpha = alphaOf(item.dimmed)
      label(p, item.text, sx, sy, fontPx(item.size, zoom), colorOf(item.ink), { align: 'right' })
      ctx.globalAlpha = 1
      return
    }
    case 'child-wash': {
      if (lod !== 'full') return
      const [sx, sy] = worldToScreen(view, item.rect.x, item.rect.y)
      const sw = item.rect.w * zoom
      const sh = item.rect.h * zoom
      ctx.globalAlpha = alphaOf(item.dimmed)
      ctx.fillStyle = colorOf(item.marker.wash)
      roundRect(ctx, sx, sy, sw, sh, cornerPx(frame.style.radius.rowWash, zoom))
      ctx.fill()
      ctx.fillStyle = colorOf(item.marker.bar)
      ctx.fillRect(sx, sy, item.marker.barWidth * zoom, sh)
      ctx.globalAlpha = 1
      return
    }
    case 'port-link': {
      if (lod !== 'full') return
      const [fx, fy] = worldToScreen(view, item.behind[0], item.behind[1])
      const [tx, ty] = worldToScreen(view, item.tip[0], item.tip[1])
      ctx.globalAlpha = alphaOf(item.dimmed)
      ctx.strokeStyle = colorOf(item.stroke)
      ctx.lineWidth = frame.style.widths.portLink
      ctx.beginPath()
      ctx.moveTo(fx, fy)
      ctx.lineTo(tx, ty)
      ctx.stroke()
      // The head is 10 screen points at any zoom.
      if (fx !== tx || fy !== ty) arrowheadOpen(ctx, tx, ty, fx, fy, 10)
      ctx.globalAlpha = 1
      return
    }
    case 'port': {
      if (lod !== 'full') return
      const [sx, sy] = worldToScreen(view, item.rect.x, item.rect.y)
      const sw = item.rect.w * zoom
      const sh = item.rect.h * zoom
      ctx.globalAlpha = alphaOf(item.dimmed)
      box(p, sx, sy, sw, sh, cornerPx(frame.style.radius.port, zoom), colorOf(item.fill), colorOf(item.stroke),
        item.strokeWidth, false)
      // Centred, its top 5 points (scaled) above the port's centre.
      label(p, item.text, sx + sw / 2, sy + sh / 2 - 5 * zoom, fontPx(frame.style.fonts.port, zoom), colorOf(item.ink),
        { align: 'center', budget: sw - 6 * zoom })
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
  const lod = p.lod
  const colorOf = (c: { role: string; grade?: number | null; ratio?: number | null }): string =>
    theme.frame(c.role, c.grade ?? null, c.ratio ?? null)

  if (edge.kind === 'stroke') {
    if (lod === 'blocks') return
    ctx.globalAlpha = edge.dimmed ? dimAlpha(frame) : 1
    ctx.strokeStyle = colorOf(edge.stroke)
    ctx.lineWidth = edge.width
    const points = decimateEdgePoints(frame, view.zoom, edge.points)
    if (lod === 'frames' || view.zoom < 0.6) segments(ctx, view, points)
    else {
      polyline(ctx, view, points)
      ctx.stroke()
    }
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
      box(p, item.rect.x, item.rect.y, item.rect.w, item.rect.h, 4,
        colorOf(item.fill), colorOf(item.border), 1, false)
      item.rows.forEach((row, index) => {
        label(p, row.text, item.rect.x + 8, item.rect.y + 8 + 20 * index,
          frame.style.fonts.chrome, colorOf(row.ink))
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
