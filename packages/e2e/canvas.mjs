import { execFileSync } from 'node:child_process'
import fs from 'node:fs'
import path from 'node:path'
import { viewerBinary } from './dsh-web.mjs'

/**
 * Pixel reads of the real panel canvas and the producer oracle they are
 * compared with. The oracle is the real binary's `scene --frame` for the same
 * IR, focus, viewport and camera the panel shows, so the expected geometry
 * comes from Rust, never from the TypeScript painter under test.
 */

/** The camera the panel summary reports (`Camera: x, y, zoom.`). */
export async function panelCamera(panel) {
  const text = await panel.summary.innerText()
  const match = /Camera: (-?[\d.]+), (-?[\d.]+), (-?[\d.]+)\./.exec(text)
  if (!match) throw new Error(`summary has no camera: ${text}`)
  return { x: Number(match[1]), y: Number(match[2]), zoom: Number(match[3]) }
}

/** The CSS viewport the panel sends to the resident (rounded canvas box). */
export async function panelViewport(panel) {
  return panel.canvas.evaluate(canvas => {
    const rect = canvas.getBoundingClientRect()
    return { width: Math.max(1, Math.round(rect.width)), height: Math.max(1, Math.round(rect.height)) }
  })
}

/** The producer frame for what `panel` shows (`uml-viewer scene --frame`). */
export async function producerFrame(panel, { root, home, focus = null, extra = '{}', select = null, hover = null, pointer = null }) {
  const camera = await panelCamera(panel)
  const viewport = await panelViewport(panel)
  const output = path.join(home, `oracle-${Date.now()}.frame.json`)
  execFileSync(viewerBinary(), [
    'scene', '--frame',
    '--ir', path.join(root, 'diagram.edn'),
    '--metrics-root', root,
    // Source-tree views take the wide layout in the window and the resident.
    '--wide',
    ...(focus ? ['--focus', `[${focus}]`] : []),
    '--width', String(viewport.width), '--height', String(viewport.height),
    '--zoom', String(camera.zoom), '--camera', `${camera.x},${camera.y}`,
    '--extra', extra,
    ...(select ? ['--select', select] : []),
    ...(hover ? ['--hover', hover] : []),
    ...(pointer ? ['--pointer', pointer.join(',')] : []),
    '--output', output,
  ], { stdio: ['ignore', 'pipe', 'pipe'] })
  return JSON.parse(fs.readFileSync(output, 'utf8'))
}

/** `screen = (world - camera) * zoom` (docs/scene-frame.md). */
export function toScreen(frame, x, y) {
  return [(x - frame.camera.x) * frame.camera.zoom, (y - frame.camera.y) * frame.camera.zoom]
}

export function screenRect(frame, rect) {
  const [x, y] = toScreen(frame, rect.x, rect.y)
  return { x, y, w: rect.w * frame.camera.zoom, h: rect.h * frame.camera.zoom }
}

/** RGBA of each CSS-pixel point on the panel canvas. */
export async function sample(panel, points) {
  return panel.canvas.evaluate((canvas, pts) => {
    const ctx = canvas.getContext('2d')
    const scale = canvas.width / canvas.getBoundingClientRect().width
    return pts.map(([x, y]) => Array.from(ctx.getImageData(Math.floor(x * scale), Math.floor(y * scale), 1, 1).data))
  }, points)
}

/** Pixels of a CSS-pixel rect whose color differs from `reference` by more than `tolerance` per channel. */
export async function inkCount(panel, rect, reference, tolerance = 24) {
  return panel.canvas.evaluate((canvas, args) => {
    const ctx = canvas.getContext('2d')
    const scale = canvas.width / canvas.getBoundingClientRect().width
    const x = Math.floor(args.rect.x * scale)
    const y = Math.floor(args.rect.y * scale)
    const w = Math.max(1, Math.floor(args.rect.w * scale))
    const h = Math.max(1, Math.floor(args.rect.h * scale))
    const data = ctx.getImageData(x, y, w, h).data
    let count = 0
    for (let i = 0; i < data.length; i += 4) {
      if ([0, 1, 2].some(c => Math.abs(data[i + c] - args.reference[c]) > args.tolerance)) count += 1
    }
    return count
  }, { rect, reference, tolerance })
}

export function same(a, b, tolerance = 6) {
  return [0, 1, 2].every(c => Math.abs(a[c] - b[c]) <= tolerance)
}

export function differs(a, b, tolerance = 24) {
  return [0, 1, 2].some(c => Math.abs(a[c] - b[c]) > tolerance)
}
