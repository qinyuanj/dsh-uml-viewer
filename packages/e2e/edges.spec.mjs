import { test, expect } from '@playwright/test'
import { launchViewerLane } from './lane.mjs'
import { panBesideInspector } from './gestures.mjs'
import { producerFrame, sample, same, toScreen, panelCamera, inkCount } from './canvas.mjs'

// #25: real source -> uml_ir -> resident -> panel. The separate binary frame
// supplies the geometry; assertions read the displayed canvas, not paint calls.
const relations = {
  association: 'open', dependency: 'open', inheritance: 'triangle',
  implements: 'triangle', aggregation: 'diamond', composition: 'diamond-fill',
}
const red = p => p[0] > 100 && p[0] > p[1] * 1.7 && p[0] > p[2] * 1.5
const mix = (a, b, t) => a.map((v, i) => v + (b[i] - v) * t)
const distance = (a, b) => Math.hypot(a[0] - b[0], a[1] - b[1])

async function colors(panel) {
  // Resolve the host's public theme tokens independently of the painter.
  return panel.panel.evaluate(el => {
    const style = getComputedStyle(el)
    const ctx = document.createElement('canvas').getContext('2d')
    const color = name => {
      ctx.fillStyle = style.getPropertyValue(name).trim()
      ctx.fillRect(0, 0, 1, 1)
      return Array.from(ctx.getImageData(0, 0, 1, 1).data)
    }
    return { relation: color('--dsw-alias-label-tertiary'), ink: color('--dsw-alias-label-primary') }
  })
}

async function near(panel, points, predicate, radius = 1) {
  const probes = points.flatMap(([x, y]) => {
    const out = []
    for (let dy = -radius; dy <= radius; dy += 1) {
      for (let dx = -radius; dx <= radius; dx += 1) out.push([x + dx, y + dy])
    }
    return out
  })
  const pixels = await sample(panel, probes)
  const group = (radius * 2 + 1) ** 2
  return points.map((_, i) => pixels.slice(i * group, (i + 1) * group).some(predicate))
}

function visible(frame, [x, y], margin = 8) {
  return x > margin && y > 65 && x < frame.viewport.width - margin && y < frame.viewport.height - margin
}

// Uniform arc-length probes catch dash gaps as well as missing or straightened
// curves. Skip the heads, class interiors and screen chrome.
function shaftPoints(frame, stroke) {
  const points = stroke.points.map(p => toScreen(frame, ...p))
  const classes = frame.world.filter(p => p.kind === 'class').map(p => {
    const [x, y] = toScreen(frame, p.rect.x, p.rect.y)
    return { x, y, w: p.rect.w * frame.camera.zoom, h: p.rect.h * frame.camera.zoom }
  })
  const out = []
  for (let i = 1; i < points.length; i += 1) {
    const a = points[i - 1], b = points[i]
    for (let d = 0; d < distance(a, b); d += 2) {
      const p = mix(a, b, d / distance(a, b))
      if (!visible(frame, p) || classes.some(r => p[0] >= r.x - 18 && p[0] <= r.x + r.w + 18 && p[1] >= r.y - 18 && p[1] <= r.y + r.h + 18)) continue
      out.push(p)
    }
  }
  return out
}

function headGeometry(frame, head) {
  const tip = toScreen(frame, ...head.tip), behind = toScreen(frame, ...head.behind)
  const length = 16 * frame.camera.zoom
  const d = distance(tip, behind)
  const axis = [(behind[0] - tip[0]) / d, (behind[1] - tip[1]) / d]
  const at = (along, across = 0) => [tip[0] + length * (axis[0] * along - axis[1] * across), tip[1] + length * (axis[1] * along + axis[0] * across)]
  return { tip, at }
}

async function expectHeads(panel, frame, palette, { dimmed = false } = {}) {
  const background = (await sample(panel, [[frame.viewport.width - 5, frame.viewport.height - 5]]))[0]
  const heads = frame.edges.filter(e => e.kind === 'head')
  expect(heads.filter(h => h.from === ':hub').map(h => [h.to.slice(1), h.head]).sort()).toEqual(Object.entries(relations).sort())
  for (const head of heads) {
    const { at } = headGeometry(frame, head)
    expect(visible(frame, at(0.5)), `${head.to} head visible`).toBe(true)
    const violating = head.from === ':dependency'
    expect(head.stroke.role, `${head.to} producer role`).toBe(violating ? 'violation' : 'relation')
    if (dimmed && !violating) expect(head.dimmed, `${head.to} dimmed`).toBe(true)
    const dim = head.dimmed ? frame.style.dimAlpha / 255 : 1
    const relation = mix(background, palette.relation, dim)
    const outline = violating ? (dimmed ? p => p[0] > p[1] + 10 : red) : p => same(p, relation, 35)
    // Both wings must be inked. Their independent positions come from UML's
    // 20-degree open/closed head geometry, not a painter helper.
    const wing = Math.sin(Math.PI / 9) * 0.65
    const along = Math.cos(Math.PI / 9) * 0.65
    expect(await near(panel, [at(along, wing), at(along, -wing)], outline), `${head.to} wings`).toEqual([true, true])
    if (head.head !== 'open') {
      // These pixels lie on the shaft, inside the closed head: the opaque
      // fill must mask it even when the outline and shaft are dimmed.
      const interior = await sample(panel, [at(0.55), at(0.65)])
      const fill = head.head === 'diamond-fill' ? palette.ink : background
      for (const pixel of interior) expect(same(pixel, fill, 22), `${head.to} opaque interior ${pixel}, fill ${fill}`).toBe(true)
    }
  }
}

async function settled(panel) {
  await panel.page.mouse.move(2, 2) // no local hover recoloring the sampled edge
  await panel.page.waitForTimeout(350)
}

test('six relation heads and solid curved shafts, with red violating dependencies', async ({ browser }) => {
  const lane = await launchViewerLane(browser)
  try {
    const panel = await lane.openPanel('nexus')
    // Enlarge the central head region with real wheel input for interior reads.
    for (let i = 0; i < 6 && (await panelCamera(panel)).zoom < 0.9; i += 1) await lane.wheel(panel, -1)
    await settled(panel)
    const frame = await producerFrame(panel, { root: lane.projects.nexus, home: lane.home })
    expect(frame.lod).toBe('full')
    const palette = await colors(panel)
    await expectHeads(panel, frame, palette)
    const strokes = frame.edges.filter(e => e.kind === 'stroke')
    expect(strokes.length).toBe(7)
    expect(strokes.filter(s => s.points.length > 10).length, 'real producer curves').toBe(2)
    for (const stroke of strokes) {
      const probes = shaftPoints(frame, stroke)
      expect(probes.length, `${stroke.to} visible shaft probes`).toBeGreaterThan(8)
      const matches = await near(panel, probes, stroke.from === ':dependency' ? red : p => same(p, palette.relation, 35))
      expect(matches.filter(Boolean).length / matches.length, `${stroke.to} continuous shaft`).toBeGreaterThan(0.94)
    }
    await lane.screenshot(panel, 'nexus-six-relations')
    expect(panel.problems).toEqual([])
  } finally { await lane.close() }
})

test('closed heads stay opaque when relations dim, including the mesh scale', async ({ browser }) => {
  const lane = await launchViewerLane(browser)
  try {
    const selected = '{:kind :class :id :observer}'
    const panel = await lane.openPanel('nexus')
    // The isolated observer is behind the inspector in the fitted sidebar.
    await panBesideInspector(panel)
    let frame = await producerFrame(panel, { root: lane.projects.nexus, home: lane.home })
    const observer = frame.world.find(p => p.kind === 'class' && p.id === ':observer')
    expect(observer, 'isolated observer').toBeTruthy()
    const at = toScreen(frame, observer.rect.x + observer.rect.w / 2, observer.rect.y + observer.rect.h / 2)
    await panel.canvas.click({ position: { x: at[0], y: at[1] } })
    await expect(panel.summary).toHaveAttribute('data-selected', ':observer')
    // Restore the head region after selecting the exposed observer.
    await panel.canvas.focus()
    await panel.page.keyboard.press('Control+0')
    await settled(panel)
    const palette = await colors(panel)
    for (const zoom of [0.9, 0.5]) {
      // At 0.5 the native mesh path previously flushed shafts over heads;
      // the panel must keep the opaque head above its shaft at either scale.
      if (zoom === 0.9) {
        for (let i = 0; i < 6 && (await panelCamera(panel)).zoom < 0.9; i += 1) await lane.wheel(panel, -1)
        await settled(panel)
      } else {
        for (let i = 0; i < 12 && (await panelCamera(panel)).zoom > 0.56; i += 1) await lane.wheel(panel, 1)
        await settled(panel)
      }
      frame = await producerFrame(panel, { root: lane.projects.nexus, home: lane.home, select: selected })
      if (zoom === 0.5) expect(frame.camera.zoom).toBeLessThan(0.6)
      await expectHeads(panel, frame, palette, { dimmed: true })
    }
    await lane.screenshot(panel, 'nexus-dimmed-heads')
    expect(panel.problems).toEqual([])
  } finally { await lane.close() }
})

test('proposal relation stays solid in full and frames tiers and disappears in blocks', async ({ browser }) => {
  const lane = await launchViewerLane(browser)
  try {
    const panel = await lane.openPanel('ledger')
    const options = { root: lane.projects.ledger, home: lane.home, extra: '{:proposal true :proposal-id :p-ledger}' }
    const palette = await colors(panel)
    let frame = await producerFrame(panel, options)
    const background = (await sample(panel, [[frame.viewport.width - 5, frame.viewport.height - 5]]))[0]
    expect(frame.chrome.filter(c => c.kind === 'proposal-banner').length).toBe(1)
    expect(await inkCount(panel, { x: 12, y: 8, w: 200, h: 24 }, background), 'proposal banner ink').toBeGreaterThan(20)
    for (const tier of ['full', 'frames', 'blocks']) {
      if (tier !== 'full') {
        const threshold = frame.style.lodZoom[tier === 'frames' ? 'full' : 'frames']
        for (let i = 0; i < 40 && (await panelCamera(panel)).zoom >= threshold; i += 1) await lane.wheel(panel, 1)
      }
      await settled(panel)
      frame = await producerFrame(panel, options)
      expect(frame.lod).toBe(tier)
      const strokes = frame.edges.filter(e => e.kind === 'stroke')
      expect(strokes.map(s => [s.from, s.to])).toEqual([[':api', ':orders']])
      const stroke = strokes[0]
      const points = [0.3, 0.4, 0.5, 0.6, 0.7].map(t => toScreen(frame, ...mix(stroke.points[0], stroke.points.at(-1), t)))
      for (const point of points) expect(visible(frame, point), `proposal ${tier} shaft visible`).toBe(true)
      const painted = await near(panel, points, pixel => same(pixel, palette.relation, 35))
      expect(painted, `proposal ${tier} shaft pixels`).toEqual(Array(5).fill(tier !== 'blocks'))
    }
    await lane.screenshot(panel, 'ledger-proposal-blocks')
    expect(panel.problems).toEqual([])
  } finally { await lane.close() }
})

test('triangles mode paints incoming/outgoing indicators and preserves violating red', async ({ browser }) => {
  const lane = await launchViewerLane(browser)
  try {
    const panel = await lane.openPanel('nexus')
    const control = panel.panel.getByRole('button', { name: /Declutter:/ })
    await expect(control).toHaveAttribute('data-mode', 'full')
    await control.click()
    await expect(control).toHaveAttribute('data-mode', 'arrows')
    await expect(control).toBeEnabled()
    await control.click()
    await expect(control).toHaveAttribute('data-mode', 'triangles')
    await settled(panel)
    const frame = await producerFrame(panel, { root: lane.projects.nexus, home: lane.home, extra: '{:declutter :triangles}' })
    expect(frame.edges.length).toBe(9)
    expect(frame.edges.every(e => e.kind === 'indicator')).toBe(true)
    const palette = await colors(panel)
    const background = (await sample(panel, [[frame.viewport.width - 5, frame.viewport.height - 5]]))[0]
    expect(frame.edges.filter(e => e.violating).length).toBe(2)
    expect(frame.edges.filter(e => !e.violating).length).toBe(7)
    const directions = []
    for (const indicator of frame.edges) {
      const points = indicator.points.map(p => toScreen(frame, ...p))
      const center = [0, 1].map(i => points.reduce((sum, p) => sum + p[i], 0) / 3)
      expect(visible(frame, center), `${indicator.id} indicator visible`).toBe(true)
      const owner = frame.world.find(p => p.kind === 'class' && p.id === indicator.id)
      expect(owner, `${indicator.id} indicator owner`).toBeTruthy()
      const ownerTop = toScreen(frame, owner.rect.x, owner.rect.y)[1]
      const incoming = center[1] < ownerTop
      directions.push(incoming ? 'in' : 'out')
      // Incoming indicators sit above their owner and point up; outgoing
      // indicators sit below it and point down (scene::hit::dep_indicators).
      const apex = Math.abs(points[1][1] - points[2][1]) < 0.01 ? points[0] : points[2]
      expect(apex[1] < center[1], `${indicator.id} indicator direction`).toBe(incoming)
      // The filled triangle's centroid is ink, dimmed like its relation; a
      // violation keeps its fixed red role.
      const ink = indicator.violating ? red : p => same(p, mix(background, palette.relation, indicator.dimmed ? frame.style.dimAlpha / 255 : 1), 40)
      expect((await near(panel, [center], ink, 1))[0], `${indicator.id} indicator`).toBe(true)
    }
    expect(directions.filter(d => d === 'in').length, 'incoming indicators').toBe(7)
    expect(directions.filter(d => d === 'out').length, 'outgoing indicators').toBe(2)
    await lane.screenshot(panel, 'nexus-triangles')
    expect(panel.problems).toEqual([])
  } finally { await lane.close() }
})
