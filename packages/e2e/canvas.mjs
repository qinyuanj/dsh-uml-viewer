import { execFileSync } from 'node:child_process'
import fs from 'node:fs'
import path from 'node:path'
import { expect } from '@playwright/test'
import { viewerBinary } from './dsh-web.mjs'

export async function panelCamera(panel) {
  const text = await panel.summary.innerText()
  const match = /Camera: (-?[\d.]+), (-?[\d.]+), (-?[\d.]+)\./.exec(text)
  if (!match) throw new Error(`summary has no camera: ${text}`)
  return { x: Number(match[1]), y: Number(match[2]), zoom: Number(match[3]) }
}

export async function panelViewport(panel) {
  return panel.canvas.evaluate(canvas => {
    const rect = canvas.getBoundingClientRect()
    return { width: Math.max(1, Math.round(rect.width)), height: Math.max(1, Math.round(rect.height)) }
  })
}

/**
 * A resize reaches the canvas CSS box at once, but the panel paints the new box
 * only once the resident answers the resize with a frame for it. Until then
 * `frame.viewport` -- the backing store, and the viewport the panel maps pointer
 * coordinates through in `cssToWorld` -- is the old one, and the browser
 * stretches the old bitmap into the new box. A point aimed from the producer
 * oracle assumes a canvas painted 1:1, so during the stretch it resolves to a
 * different world point: the click selects another class or none, and the class
 * card never opens (#148). Wait for the panel's own signal -- the backing store
 * matching the box -- instead of for a delay. A panel with no frame yet has
 * nothing to settle.
 */
export async function settleCanvas(panel) {
  if (await panel.summary?.getAttribute?.('data-kind') !== 'frame') return
  await expect.poll(async () => panel.canvas.evaluate(canvas => {
    const dpr = window.devicePixelRatio || 1
    const box = canvas.getBoundingClientRect()
    if (!(box.width > 0) || !(box.height > 0)) return false
    return Math.abs(canvas.width / dpr - box.width) <= 1 && Math.abs(canvas.height / dpr - box.height) <= 1
  }), { message: 'the panel painted its canvas at the current box' }).toBe(true)
}

/**
 * Hold the renderer so a resize just dispatched cannot have been painted yet.
 * That is the loaded-host window #148 failed in, made deterministic: the canvas
 * box is already the new one while `frame.viewport` is still the old one. A
 * regression case that resizes and then aims through the oracle runs through it
 * every time instead of only on a busy runner.
 */
export async function holdRenderer(panel, ms = 1500) {
  await panel.page.evaluate(budget => {
    const end = performance.now() + budget
    while (performance.now() < end) continue
  }, ms)
}

export async function producerFrame(panel, { root, home, focus = null, extra = '{}', select = null, hover = null, pointer = null, hoverMetadata = false }) {
  for (let attempt = 1; ; attempt++) {
    await settleCanvas(panel)
    const camera = await panelCamera(panel)
    const viewport = await panelViewport(panel)
    const output = path.join(home, `oracle-${Date.now()}-${attempt}.frame.json`)
    execFileSync(viewerBinary(), [
      'scene', '--frame',
      '--ir', path.join(root, 'diagram.edn'),
      '--metrics-root', root,
      '--wide',
      ...(focus ? ['--focus', `[${focus}]`] : []),
      '--width', String(viewport.width), '--height', String(viewport.height),
      '--zoom', String(camera.zoom), '--camera', `${camera.x},${camera.y}`,
      '--extra', extra,
      ...(select ? ['--select', select] : []),
      ...(hover ? ['--hover', hover] : []),
      ...(pointer ? ['--pointer', pointer.join(',')] : []),
      ...(hoverMetadata ? ['--hover-metadata'] : []),
      '--output', output,
    ], { stdio: ['ignore', 'pipe', 'pipe'] })
    const frame = JSON.parse(fs.readFileSync(output, 'utf8'))
    const after = await panelViewport(panel)
    if (after.width === viewport.width && after.height === viewport.height) return frame
    if (attempt >= 3) throw new Error(`the panel resized while the producer oracle measured it: ${viewport.width}x${viewport.height} -> ${after.width}x${after.height}`)
  }
}

export function toScreen(frame, x, y) {
  return [(x - frame.camera.x) * frame.camera.zoom, (y - frame.camera.y) * frame.camera.zoom]
}

export function screenRect(frame, rect) {
  const [x, y] = toScreen(frame, rect.x, rect.y)
  return { x, y, w: rect.w * frame.camera.zoom, h: rect.h * frame.camera.zoom }
}

export async function sample(panel, points) {
  return panel.canvas.evaluate((canvas, pts) => {
    const ctx = canvas.getContext('2d')
    const scale = canvas.width / canvas.getBoundingClientRect().width
    return pts.map(([x, y]) => Array.from(ctx.getImageData(Math.floor(x * scale), Math.floor(y * scale), 1, 1).data))
  }, points)
}

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
