import { test, expect } from '@playwright/test'
import { launchViewerLane } from './lane.mjs'
import { differs, inkCount, panelCamera, producerFrame, sample, same, screenRect, toScreen } from './canvas.mjs'

/**
 * Panel graph primitives (dsh-uml-viewer#24), the browser half of the egui
 * class-box driver scenarios (scripts/drivers/class_boxes.edn,
 * font_parity.edn, lod_shots.edn): the real panel canvas is read pixel by
 * pixel and compared with the real producer frame (`uml-viewer scene --frame`)
 * for the same IR, viewport and camera. The atlas sample has no metrics, so
 * every class and component frame is unmeasured (dashed); the foreign-package
 * ellipse keeps a solid outline.
 */

/** Screen rects of everything drawn over a box (text, dots, markers, lines, chrome). */
function occupied(frame) {
  const z = frame.camera.zoom
  const out = []
  for (const item of frame.world) {
    if (item.kind === 'text') {
      const [x, y] = toScreen(frame, item.x, item.top)
      out.push({ x, y, w: item.maxW * z, h: item.slot * z })
    } else if (item.kind === 'quality') {
      const [x, y] = toScreen(frame, item.center[0] - item.radius, item.center[1] - item.radius)
      out.push({ x, y, w: 2 * item.radius * z, h: 2 * item.radius * z })
    } else if (item.kind === 'marker' || item.kind === 'level') {
      const [x, y] = toScreen(frame, item.at[0], item.at[1])
      out.push({ x, y, w: item.size * z * 1.2, h: item.size * z * 1.4 })
    } else if (item.kind === 'no-data') {
      const [x, y] = toScreen(frame, item.right - item.size * 4, item.y)
      out.push({ x, y, w: item.size * 4 * z, h: item.size * z * 1.4 })
    } else if (item.kind === 'band' || item.kind === 'rule') {
      const [x1, y1] = toScreen(frame, item.from[0], item.from[1])
      const [x2] = toScreen(frame, item.to[0], item.to[1])
      out.push({ x: x1, y: y1 - 2, w: x2 - x1, h: 4 })
    }
  }
  for (const edge of frame.edges) {
    const points = edge.points ?? [edge.tip, edge.behind]
    for (const [x, y] of points.map(point => toScreen(frame, point[0], point[1]))) out.push({ x: x - 10, y: y - 10, w: 20, h: 20 })
    for (let i = 1; i < points.length; i += 1) {
      const [x1, y1] = toScreen(frame, points[i - 1][0], points[i - 1][1])
      const [x2, y2] = toScreen(frame, points[i][0], points[i][1])
      for (let t = 0; t <= 1; t += 0.05) out.push({ x: x1 + (x2 - x1) * t - 4, y: y1 + (y2 - y1) * t - 4, w: 8, h: 8 })
    }
  }
  for (const item of frame.chrome) {
    if (item.at) out.push({ x: item.at[0] - 8, y: item.at[1] - 4, w: 400, h: 30 })
  }
  return out
}

const inside = (r, x, y, margin = 0) => x >= r.x - margin && x <= r.x + r.w + margin && y >= r.y - margin && y <= r.y + r.h + margin

/** A point of `rect` (shrunk by `inset`) at least 4 px from anything drawn over it and outside `exclude`. */
function freePoint(frame, rect, inset = 5, exclude = []) {
  const busy = [...occupied(frame), ...exclude]
  for (let y = rect.y + inset; y <= rect.y + rect.h - inset; y += 2) {
    for (let x = rect.x + inset; x <= rect.x + rect.w - inset; x += 2) {
      if (!busy.some(r => inside(r, x, y, 4))) return [x, y]
    }
  }
  throw new Error(`no free point in ${JSON.stringify(rect)}`)
}

function find(frame, predicate, what) {
  const item = frame.world.find(predicate)
  if (!item) throw new Error(`producer frame has no ${what}`)
  return item
}

/** Color runs along a horizontal line just outside a box edge: a dashed edge alternates. */
async function edgeRuns(panel, x0, x1, y, outside) {
  const points = []
  for (let x = Math.ceil(x0); x <= Math.floor(x1); x += 1) points.push([x + 0.5, y])
  const pixels = await sample(panel, points)
  let runs = 1
  for (let i = 1; i < pixels.length; i += 1) {
    if (differs(pixels[i], outside, 30) !== differs(pixels[i - 1], outside, 30)) runs += 1
  }
  return runs
}

/** The label's text slot on screen. */
function slot(frame, item) {
  const [x, y] = toScreen(frame, item.x, item.top)
  return { x, y, w: item.maxW * frame.camera.zoom, h: item.slot * frame.camera.zoom }
}

/**
 * No label spills past a box: the columns 4 px outside its left and right
 * edges stay `outside`, except where a relation line leaves the box.
 */
async function expectNoOverflow(panel, frame, r, outside, label) {
  const busy = occupied(frame)
  const column = []
  for (let y = r.y + 3; y < r.y + r.h - 3; y += 1) column.push([r.x - 4, y], [r.x + r.w + 4, y])
  column.splice(0, column.length, ...column.filter(([x, y]) => !busy.some(b => inside(b, x, y))))
  expect(column.length, `${label} free column`).toBeGreaterThan(r.h / 2)
  for (const pixel of await sample(panel, column)) expect(same(pixel, outside, 8), `${label} overflow ${pixel}`).toBe(true)
}

test('paints class boxes, component boxes, α/I, levels, CJK names and the foreign ellipse like the producer', async ({ browser }) => {
  const lane = await launchViewerLane(browser)
  try {
    const atlas = await lane.openPanel('atlas')
    // The panel repaints once its packaged Sarasa face has loaded.
    await expect.poll(() => atlas.page.evaluate(() => [...document.fonts]
      .some(face => face.family.replaceAll('"', '') === 'UML Viewer Sarasa' && face.status === 'loaded')), { timeout: 30_000 }).toBe(true)
    await atlas.page.waitForTimeout(500)
    const frame = await producerFrame(atlas, { root: lane.projects.atlas, home: lane.home })
    expect(frame.lod).toBe('full')
    await lane.screenshot(atlas, 'atlas-full')

    const background = (await sample(atlas, [[frame.viewport.width - 6, frame.viewport.height - 6]]))[0]
    const classes = frame.world.filter(item => item.kind === 'class' && item.shape !== 'oval')
    expect(classes.map(item => item.id).sort()).toEqual([':gateway', ':priced', ':订单'])
    const classRects = classes.map(item => screenRect(frame, item.rect))

    // 组件框: a translucent component fill over the canvas, unlike the opaque class fill.
    const pkgRect = screenRect(frame, find(frame, item => item.kind === 'package', 'package').rect)
    const [pkgInterior, classFill] = await sample(atlas, [freePoint(frame, pkgRect, 8, classRects), freePoint(frame, classRects[0])])
    expect(differs(pkgInterior, background, 4), `package ${pkgInterior} vs canvas ${background}`).toBe(true)
    expect(differs(classFill, pkgInterior, 10), `class ${classFill} vs package ${pkgInterior}`).toBe(true)
    // Unmeasured: the component frame is dashed (6 on, 4 off) along its top edge.
    expect(await edgeRuns(atlas, pkgRect.x + 10, pkgRect.x + pkgRect.w - 10, pkgRect.y - 1, background)).toBeGreaterThanOrEqual(8)

    for (const [index, item] of classes.entries()) {
      const r = classRects[index]
      expect(await edgeRuns(atlas, r.x + 6, r.x + r.w - 6, r.y - 1, pkgInterior), `${item.id} dashed`).toBeGreaterThanOrEqual(6)
      // Every box has the same producer fill (unmeasured red), the sampled one.
      expect(item.fill, `${item.id} fill role`).toEqual(classes[0].fill)
      await expectNoOverflow(atlas, frame, r, pkgInterior, item.id)
      // Every class name is inked in its slot.
      const name = find(frame, t => t.kind === 'text' && t.line === 'name' && t.owner === item.id, `${item.id} name`)
      expect(await inkCount(atlas, slot(frame, name), classFill), `${item.id} name ${name.text}`).toBeGreaterThan(20)
    }

    // 架构级别编号: gateway on level 1, priced on level 0.
    const levels = frame.world.filter(item => item.kind === 'level')
    expect(levels.map(item => [item.owner, item.text]).sort()).toEqual([[':gateway', '1'], [':priced', '0']])
    for (const level of levels) {
      const [x, y] = toScreen(frame, level.at[0], level.at[1])
      const px = Math.round(level.size * frame.camera.zoom)
      expect(await inkCount(atlas, { x, y, w: px, h: px * 1.3 }, classFill), `level ${level.text}`).toBeGreaterThan(3)
    }

    // α/I and the stereotype of the interface.
    const marker = find(frame, item => item.kind === 'marker', 'marker')
    expect([marker.owner, marker.text]).toEqual([':priced', 'I'])
    const [mx, my] = toScreen(frame, marker.at[0], marker.at[1])
    const mpx = Math.round(marker.size * frame.camera.zoom)
    expect(await inkCount(atlas, { x: mx, y: my, w: mpx, h: mpx * 1.3 }, classFill)).toBeGreaterThan(3)
    const stereo = find(frame, item => item.kind === 'text' && item.line === 'stereo', 'stereotype')
    expect([stereo.owner, stereo.text]).toEqual([':priced', '«interface»'])
    expect(await inkCount(atlas, slot(frame, stereo), classFill)).toBeGreaterThan(20)

    // 外来包椭圆: filled inside, canvas in the bounding-box corner, a solid ring.
    const oval = find(frame, item => item.kind === 'class' && item.shape === 'oval', 'oval')
    expect([oval.id, oval.missing]).toEqual([':react', true])
    const o = screenRect(frame, oval.rect)
    const [cx, cy, rx, ry] = [o.x + o.w / 2, o.y + o.h / 2, o.w / 2, o.h / 2]
    const [corner, body] = await sample(atlas, [[o.x + 2, o.y + 2], [cx - rx * 0.55, cy + ry * 0.45]])
    expect(same(corner, background, 8), `oval corner ${corner}`).toBe(true)
    expect(same(body, classFill, 6), `oval body ${body}`).toBe(true)
    const ring = []
    for (let k = 0; k < 24; k += 1) ring.push([cx + rx * Math.cos((2 * Math.PI * k) / 24), cy + ry * Math.sin((2 * Math.PI * k) / 24)])
    for (const [k, pixel] of (await sample(atlas, ring)).entries()) expect(differs(pixel, background), `oval ring ${k} ${pixel}`).toBe(true)

    // CJK is measured with the packaged Sarasa face: 1 em per CJK glyph, 0.5 em per Latin one.
    const widths = await atlas.page.evaluate(() => {
      const ctx = document.createElement('canvas').getContext('2d')
      ctx.font = '16px "UML Viewer Sarasa", monospace'
      return { cjk: ctx.measureText('订单').width, latin: ctx.measureText('Gateway').width }
    })
    expect(widths).toEqual({ cjk: 32, latin: 56 })

    // The gateway's title band and member line, inked at the full tier.
    const bandRegion = view => {
      const band = find(view, item => item.kind === 'band' && item.owner === ':gateway', 'gateway band')
      const op = find(view, item => item.kind === 'text' && item.line === 'op' && item.owner === ':gateway', 'gateway op')
      const [bx, by] = toScreen(view, band.from[0] + 2, band.from[1] - 2)
      const [ex, ey] = toScreen(view, band.to[0] - 2, op.top + op.slot)
      return { x: bx, y: by, w: ex - bx, h: ey - by }
    }
    expect(await inkCount(atlas, bandRegion(frame), classFill), 'full-tier band and member line').toBeGreaterThan(60)

    // lod_shots: wheel out to the frames tier. Names stay; the band, member
    // lines and level numbers of the full tier are gone.
    let camera = await panelCamera(atlas)
    for (let notch = 0; notch < 30 && camera.zoom >= frame.style.lodZoom.full; notch += 1) {
      await lane.wheel(atlas, 1)
      camera = await panelCamera(atlas)
    }
    expect(camera.zoom).toBeLessThan(frame.style.lodZoom.full)
    expect(camera.zoom).toBeGreaterThanOrEqual(frame.style.lodZoom.frames)
    await atlas.page.waitForTimeout(300)
    const far = await producerFrame(atlas, { root: lane.projects.atlas, home: lane.home })
    expect(far.lod).toBe('frames')
    await lane.screenshot(atlas, 'atlas-frames')
    const gatewayName = find(far, t => t.kind === 'text' && t.line === 'name' && t.owner === ':gateway', 'gateway name')
    expect(await inkCount(atlas, slot(far, gatewayName), classFill)).toBeGreaterThan(5)
    // A 2-point band alone would ink ~70 pixels here; a crossing relation line a few.
    expect(await inkCount(atlas, bandRegion(far), classFill), 'no full-tier band or member line').toBeLessThanOrEqual(4)
  } finally {
    await lane.close()
  }
})
