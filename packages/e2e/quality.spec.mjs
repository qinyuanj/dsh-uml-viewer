import fs from 'node:fs/promises'
import path from 'node:path'
import { test, expect } from '@playwright/test'
import { launchViewerLane } from './lane.mjs'
import { producerFrame, sample, same, toScreen, panelCamera, inkCount } from './canvas.mjs'

// #26: real IR/resident/panel, no injected frame. Replaces quality/highlight
// assertions in egui commands/driver scenarios; geometry and grades come from
// the independent producer oracle. Authored under RESOURCE-POLICY: NOT RUN
// until a matching EXISTING binary is provided, never build from this lane.
const palettes = {
  dark: { fill: ['#4a2818', '#3d3a18', '#1e4a38'], stroke: ['#e07a4a', '#d4c05a', '#5fb58a'], noData: '#c42a24', violation: '#c42a24', hot: '#ff4030', chain: '#3e98ec' },
  light: { fill: ['#e6c3b2', '#e2dfb6', '#b6e2d0'], stroke: ['#b7501f', '#aa952c', '#44926b'], noData: '#b52721', violation: '#b52721', hot: '#f01300', chain: '#136ec3' },
}
const rgb = hex => [1, 3, 5].map(i => parseInt(hex.slice(i, i + 2), 16))
// The documented frame::theme ramp, independently evaluated by the test oracle.
function ramp(grade, anchors) {
  const [a, b, t] = grade <= 5 ? [anchors[0], anchors[1], grade / 5] : [anchors[1], anchors[2], (grade - 5) / 5]
  return rgb(a).map((v, i) => Math.round(v + (rgb(b)[i] - v) * t))
}
const mix = (a, b, t) => a.map((v, i) => v + (b[i] - v) * t)

async function prepareQuality(projects) {
  // Three distinct missing-data routes: association has CRAP only; dependency
  // mutation only; observer neither. hub has both and is healthy.
  const root = projects.nexus
  await fs.mkdir(path.join(root, '.metrics', 'mutate'), { recursive: true })
  await fs.writeFile(path.join(root, '.metrics', 'crap.edn'), `{:entries [
    {:name "Hub" :namespace "nexus.hub" :complexity 1 :coverage 100.0 :crap 1.0}
    {:name "Target" :namespace "nexus.association" :complexity 1 :coverage 100.0 :crap 1.0}]}`)
  for (const [name, member] of [['hub', 'Hub'], ['dependency', 'Target']]) {
    await fs.writeFile(path.join(root, '.metrics', 'mutate', `${name}.edn`), `{:version 1
      :namespace "nexus.${name}" :source "src/${name}.ts" :outcomes {}
      :forms [{:id "defn/${member}" :kind "defn" :file "src/${name}.ts" :line 1 :end-line 3
               :hash "fixture" :killed 10 :survived 0 :uncovered 0 :sites 10}]}`)
  }
}
async function settle(panel) {
  await panel.page.mouse.move(2, 2)
  await panel.page.waitForTimeout(300)
}
async function visibleFull(lane, panel) {
  for (let i = 0; i < 12 && (await panelCamera(panel)).zoom < 0.9; i++) await lane.wheel(panel, -1)
  await settle(panel)
}
async function near(panel, point, color, tolerance = 20, radius = 2) {
  const probes = []
  for (let y = -radius; y <= radius; y++) for (let x = -radius; x <= radius; x++) probes.push([point[0] + x, point[1] + y])
  return (await sample(panel, probes)).some(pixel => same(pixel, color, tolerance))
}
function center(frame, item) { return toScreen(frame, item.rect.x + item.rect.w / 2, item.rect.y + item.rect.h / 2) }
function shaftProbe(frame, stroke) {
  // Pick a visible segment midpoint outside every box and head, so neither
  // world paint nor head fill can make a missing relation pass.
  const boxes = frame.world.filter(i => i.kind === 'class').map(i => i.rect)
  const candidates = []
  for (let i = 1; i < stroke.points.length; i++) {
    const a = stroke.points[i - 1], b = stroke.points[i], p = mix(a, b, 0.5)
    const s = toScreen(frame, ...p)
    if (s[0] < 12 || s[1] < 65 || s[0] > frame.viewport.width - 12 || s[1] > frame.viewport.height - 12) continue
    if (boxes.some(r => p[0] >= r.x - 20 && p[0] <= r.x + r.w + 20 && p[1] >= r.y - 20 && p[1] <= r.y + r.h + 20)) continue
    candidates.push({ p, s, length: Math.hypot(b[0] - a[0], b[1] - a[1]) })
  }
  candidates.sort((a, b) => b.length - a.length)
  expect(candidates.length, `${stroke.from}->${stroke.to} has a visible independent probe`).toBeGreaterThan(0)
  return candidates[0]
}
/** True when any canvas pixel inside a CSS-pixel rect matches `color`. */
async function regionHas(panel, rect, color, tolerance = 45) {
  return panel.canvas.evaluate((canvas, args) => {
    const ctx = canvas.getContext('2d'), s = canvas.width / canvas.getBoundingClientRect().width
    const data = ctx.getImageData(Math.floor(args.rect.x * s), Math.floor(args.rect.y * s),
      Math.max(1, Math.floor(args.rect.w * s)), Math.max(1, Math.floor(args.rect.h * s))).data
    for (let i = 0; i < data.length; i += 4) {
      if ([0, 1, 2].every(k => Math.abs(data[i + k] - args.color[k]) <= args.tolerance)) return true
    }
    return false
  }, { rect, color, tolerance })
}

async function hostColors(panel) {
  return panel.canvas.evaluate(el => {
    const style = getComputedStyle(el), c = document.createElement('canvas').getContext('2d')
    const color = name => { c.fillStyle = style.getPropertyValue(name).trim(); c.fillRect(0, 0, 1, 1); return Array.from(c.getImageData(0, 0, 1, 1).data) }
    return { canvas: color('--dsw-alias-bg-base'), hover: color('--dsw-alias-label-primary'), relation: color('--dsw-alias-label-tertiary') }
  })
}

test('quality colors, C/M dots and no-data survive light/dark and changed skin tokens', async ({ browser }) => {
  const lane = await launchViewerLane(browser, { prepareProjects: prepareQuality })
  try {
    const panel = await lane.openPanel('nexus')
    await visibleFull(lane, panel)
    const frame = await producerFrame(panel, { root: lane.projects.nexus, home: lane.home })
    expect(frame.lod).toBe('full')
    const classes = frame.world.filter(i => i.kind === 'class')
    expect(classes).toHaveLength(8)
    expect(classes.find(i => i.id === ':hub').missing).toBe(false)
    for (const id of [':association', ':dependency', ':observer']) {
      const box = classes.find(i => i.id === id)
      expect([box.missing, box.band], id).toEqual([true, 'red'])
      expect(frame.world.filter(i => i.kind === 'no-data' && i.owner === id)).toHaveLength(1)
    }
    for (const mode of ['dark', 'light']) for (const skin of [false, true]) {
      await panel.panel.evaluate((el, { mode, skin }) => {
        const tokens = skin ? {
          '--dsw-alias-bg-base': '#3d3929', '--dsw-alias-label-primary': '#f0eee6',
          '--dsw-alias-label-tertiary': '#b1ada1', '--dsw-alias-state-business-primary': '#d97757',
          '--dsw-alias-state-success-primary': '#8a5cf6', '--dsw-alias-state-error-primary': '#517fff',
        } : {}
        for (const key of ['--dsw-alias-bg-base', '--dsw-alias-label-primary', '--dsw-alias-label-tertiary', '--dsw-alias-state-business-primary', '--dsw-alias-state-success-primary', '--dsw-alias-state-error-primary']) {
          if (skin) el.style.setProperty(key, tokens[key]); else el.style.removeProperty(key)
        }
        document.body.toggleAttribute('data-ds-dark-theme', mode === 'dark')
        // The public theme attribute also requests repaint when only tokens changed.
        document.body.dataset.theme = `${mode}-${skin}`
      }, { mode, skin })
      await panel.page.waitForTimeout(100)
      const palette = palettes[mode]
      for (const box of classes) {
        // In the lower empty part of each box, away from the title and badges.
        const fill = toScreen(frame, box.rect.x + box.rect.w / 2, box.rect.y + box.rect.h * 0.75)
        expect(await near(panel, fill, ramp(box.fill.grade, palette.fill)), `${mode}/${skin} ${box.id} fill`).toBe(true)
      }
      const markerInk = (await hostColors(panel)).hover
      const dots = frame.world.filter(i => i.kind === 'quality' && i.of === 'class')
      expect(dots).toHaveLength(16)
      for (const dot of dots) {
        // Avoid the letter at the centre, sample inside left of it and at ring left.
        const c = toScreen(frame, ...dot.center), r = dot.radius * frame.camera.zoom
        expect(await near(panel, [c[0] - r * 0.55, c[1]], ramp(dot.fill.grade, palette.fill), 20, 1), `${dot.id} ${dot.text} fill`).toBe(true)
        expect(await near(panel, [c[0] - r, c[1]], ramp(dot.stroke.grade, palette.stroke), 35, 1), `${dot.id} ${dot.text} ring`).toBe(true)
        const letter = await panel.canvas.evaluate((canvas, { c, r, markerInk }) => {
          const ctx = canvas.getContext('2d'), s = canvas.width / canvas.getBoundingClientRect().width
          const d = ctx.getImageData(Math.floor((c[0] - r / 2) * s), Math.floor((c[1] - r / 2) * s), Math.max(1, Math.floor(r * s)), Math.max(1, Math.floor(r * s))).data
          // Compare actual marker ink inside the disc, not brightness: the
          // light-mode disc is itself bright and must not satisfy this test.
          let count = 0
          for (let i = 0; i < d.length; i += 4) if ([0, 1, 2].every(k => Math.abs(d[i + k] - markerInk[k]) <= 25)) count++
          return count
        }, { c, r, markerInk })
        expect(letter, `${dot.id} ${dot.text} letter`).toBeGreaterThan(0)
      }
      // No-data is red ink at the producer's right-aligned text location.
      for (const mark of frame.world.filter(i => i.kind === 'no-data')) {
        const right = toScreen(frame, mark.right, mark.y)
        const points = []
        for (let y = 0; y < mark.size * frame.camera.zoom; y++) for (let x = 0; x < 3 * mark.size * frame.camera.zoom; x++) points.push([right[0] - x, right[1] + y])
        expect((await sample(panel, points)).some(p => same(p, rgb(palette.noData), 40)), `${mode}/${skin} ${mark.owner} no-data ink`).toBe(true)
      }
      const violation = frame.edges.find(e => e.kind === 'stroke' && e.stroke.role === 'violation')
      expect(violation).toBeTruthy()
      expect(await near(panel, shaftProbe(frame, violation).s, rgb(palette.violation), 35), `${mode}/${skin} violation hue`).toBe(true)
    }
    await lane.screenshot(panel, 'quality-light-skin')
    expect(panel.problems).toEqual([])
  } finally { await lane.close() }
})

test('overview hover names the class at the producer popup position and clears on leave', async ({ browser }) => {
  const lane = await launchViewerLane(browser)
  try {
    const panel = await lane.openPanel('nexus')
    for (let i = 0; i < 30 && (await panelCamera(panel)).zoom >= 0.4; i++) await lane.wheel(panel, 1)
    await settle(panel)
    const options = { root: lane.projects.nexus, home: lane.home }
    const frame = await producerFrame(panel, options)
    expect(frame.lod).toBe('frames')
    const hub = frame.world.find(i => i.kind === 'class' && i.id === ':hub')
    const point = center(frame, hub)
    const hovered = await producerFrame(panel, { ...options, hover: '{:kind :class :id :hub}', pointer: point })
    const popup = hovered.chrome.find(i => i.kind === 'popup')
    expect(popup).toBeTruthy()
    expect(popup.rows).toHaveLength(1)
    const colors = await hostColors(panel)
    const textRect = { x: popup.rect.x + 8, y: popup.rect.y + 8, w: popup.rect.w - 16, h: 18 }
    const before = await inkCount(panel, textRect, colors.canvas)
    await panel.canvas.hover({ position: { x: point[0], y: point[1] } })
    // The popup's row is visible at its Rust-produced bounds, then leaving the
    // canvas removes it. The mouse never triggers a resident selection action.
    const shown = await inkCount(panel, textRect, colors.canvas)
    expect(shown).toBeGreaterThan(before)
    await settle(panel)
    expect(await inkCount(panel, textRect, colors.canvas)).toBe(before)
    expect(panel.problems).toEqual([])
  } finally { await lane.close() }
})

test('resident dependency chain, relation dimming and temporary client class/edge hover', async ({ browser }) => {
  const lane = await launchViewerLane(browser)
  try {
    const panel = await lane.openPanel('nexus')
    await panel.page.evaluate(() => document.body.setAttribute('data-ds-dark-theme', ''))
    await visibleFull(lane, panel)
    const options = { root: lane.projects.nexus, home: lane.home }
    let frame = await producerFrame(panel, options)
    const hub = frame.world.find(i => i.kind === 'class' && i.id === ':hub')
    const [x, y] = center(frame, hub)
    await panel.canvas.click({ position: { x, y } })
    await expect(panel.summary).toHaveAttribute('data-selected', ':hub')
    await settle(panel)
    frame = await producerFrame(panel, { ...options, select: '{:kind :class :id :hub}' })
    const chain = frame.edges.filter(e => e.kind === 'stroke' && e.stroke.role === 'dependency-chain')
    const dimmed = frame.edges.filter(e => e.kind === 'stroke' && e.dimmed)
    expect(chain.map(e => [e.from, e.to])).toEqual([[':hub', ':dependency']])
    expect(dimmed).toHaveLength(5)
    const colors = await hostColors(panel)
    for (const edge of chain) expect(await near(panel, shaftProbe(frame, edge).s, rgb(palettes.dark.chain), 35)).toBe(true)
    // The exact dimmed appearance comes from the producer's dimAlpha, applied
    // over the real canvas. #25 edges.spec separately asserts opaque hollow heads.
    const edge = dimmed.find(e => e.to === ':association')
    expect(edge).toBeTruthy()
    const probe = shaftProbe(frame, edge)
    expect(await near(panel, probe.s, mix(colors.canvas, colors.relation, frame.style.dimAlpha / 255), 22, 1)).toBe(true)
    await panel.canvas.hover({ position: { x: probe.s[0], y: probe.s[1] } })
    expect(await near(panel, probe.s, colors.hover, 35)).toBe(true)
    // Selection belongs to the resident and stays the same under local hover.
    await expect(panel.summary).toHaveAttribute('data-selected', ':hub')
    await settle(panel)
    expect(await near(panel, probe.s, mix(colors.canvas, colors.relation, frame.style.dimAlpha / 255), 22, 1)).toBe(true)
    await panel.canvas.hover({ position: { x, y } })
    // Class hover warms every touching relation, including violating reverse edge.
    for (const e of frame.edges.filter(e => e.kind === 'stroke')) {
      const violating = ['violation', 'violation-hot'].includes(e.stroke.role)
      expect(await near(panel, shaftProbe(frame, e).s, violating ? rgb(palettes.dark.hot) : colors.hover, 35), `${e.from}->${e.to} warm`).toBe(true)
    }
    // Edge hover: the producer's own dependency tooltip rows, rendered at its
    // placement. The rows come from --hover-metadata, never from the client.
    const tooltipEdge = frame.edges.find(e => e.kind === 'stroke' && e.to === ':association')
    const probePoint = shaftProbe(frame, tooltipEdge).s
    const tooltip = await producerFrame(panel, { ...options, hover: '{:kind :edge :from :hub :to :association}', pointer: probePoint, hoverMetadata: true })
    expect(tooltip.hover.complete).toBe(true)
    const entry = tooltip.hover.edges.find(e => e.from === ':hub' && e.to === ':association')
    expect(entry.rows.length).toBeGreaterThan(0)
    const popup = tooltip.chrome.find(c => c.kind === 'popup')
    expect(popup.rows).toEqual(entry.rows)
    await panel.canvas.hover({ position: { x: probePoint[0], y: probePoint[1] } })
    // The only violating row is the first one, inked in 违规关系色 over the popup.
    const firstRow = { x: popup.rect.x + 8, y: popup.rect.y + 8, w: popup.rect.w - 16, h: 18 }
    expect(await regionHas(panel, firstRow, rgb(palettes.dark.violation), 60), 'dependency tooltip row ink').toBe(true)
    await lane.screenshot(panel, 'dependency-chain-class-hover')
    expect(panel.problems).toEqual([])
  } finally { await lane.close() }
})
