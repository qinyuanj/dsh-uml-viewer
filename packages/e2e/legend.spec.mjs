import { test, expect } from '@playwright/test'
import { launchViewerLane } from './lane.mjs'
import { producerFrame, same, screenRect, toScreen } from './canvas.mjs'
import { toggleFullscreen } from './lifecycle.mjs'

/**
 * The diagram's legend (#50, ADR 0015): the rows are typeset by the resident
 * (#44's language table), the panel draws them as its own DOM, 「更多」 opens
 * them as a popover below 900px of panel width, and from 900px they are
 * resident beside the diagram — a reserved column, never an overlay on the
 * classes. A language switch rewords the rows with the next frame.
 */
let lane

test.beforeAll(async ({ browser }) => { lane = await launchViewerLane(browser) })
test.afterAll(async () => { await lane?.close() })

const legend = panel => panel.panel.locator('.dsh-uml-legend')
const moreButton = panel => panel.panel.locator('.dsh-uml-viewer-more')
const moreMenu = panel => panel.panel.locator('.dsh-uml-viewer-more-menu')
const row = (panel, id) => legend(panel).locator(`[data-row="${id}"]`)

/**
 * Narrow the sidebar to about 400px with the host's own width handle when the
 * default column is wider; the panel's container width is what the layout reads
 * (the same helper the drawer spec uses, #49).
 */
async function narrowSidebar(panel, width = 400) {
  const current = await panel.panel.evaluate(element => element.clientWidth)
  if (current <= width + 40) return current
  const box = await panel.panel.boundingBox()
  const handle = panel.page.locator('[data-sidebar-right-resize], [role="separator"][aria-orientation="vertical"]').last()
  if (await handle.count()) {
    const grip = await handle.boundingBox()
    const x = grip.x + grip.width / 2
    const y = grip.y + grip.height / 2
    await panel.page.mouse.move(x, y)
    await panel.page.mouse.down()
    await panel.page.mouse.move(x + (current - width), y, { steps: 10 })
    await panel.page.mouse.up()
  } else {
    const viewport = panel.page.viewportSize()
    await panel.page.setViewportSize({ width: Math.max(800, Math.round(viewport.width - (box.width - width) * 2)), height: viewport.height })
  }
  return panel.panel.evaluate(element => element.clientWidth)
}

/** Switch the DSH language the way a user does (Settings → General → Language). */
async function switchLanguage(page, label) {
  await page.getByRole('button', { name: /^(Account menu|账号菜单)$/ }).first().click()
  await page.locator('[role^="menuitem"]').filter({ hasText: /^(Settings|设置)/ }).first().click()
  await page.getByText(/^(General|通用设置)$/).first().click()
  const languageRow = page.getByText(/^(Language|语言)$/).first().locator('xpath=ancestor::*[.//button[@aria-haspopup="menu"]][1]')
  await languageRow.locator('button[aria-haspopup="menu"]').click()
  await page.locator('[role^="menuitem"]').filter({ hasText: new RegExp(`^${label}$`) }).first().click()
  await page.keyboard.press('Escape')
}

/** A canvas point on a class box, from the producer frame. */
async function classPoint(panel) {
  const frame = await producerFrame(panel, { root: lane.projects.shop, home: lane.home })
  const item = frame.world.find(entry => entry.kind === 'class' && entry.shape !== 'oval')
  expect(item, 'the frame has a class box').toBeTruthy()
  const [x, y] = toScreen(frame, item.rect.x + item.rect.w / 2, item.rect.y + item.rect.h / 2)
  return [x, y]
}

test('更多 opens the legend as a popover, and Esc closes it without going back', async () => {
  const panel = await lane.openPanel('shop')
  await narrowSidebar(panel)
  await expect(panel.panel).toHaveAttribute('data-layout', 'drawer')
  // Below 900px the legend is a popover: closed until 「更多」 opens it.
  await expect(legend(panel)).toBeHidden()

  const at = await classPoint(panel)
  await panel.canvas.click({ position: { x: at[0], y: at[1] } })
  await expect(panel.summary).toHaveAttribute('data-selected', /^:/)

  await moreButton(panel).click()
  const entry = moreMenu(panel).getByRole('menuitem', { name: 'Legend', exact: true })
  await expect(entry).toBeVisible()
  await entry.click()
  const region = legend(panel)
  await expect(region).toBeVisible()
  await expect(region).toHaveAttribute('data-mode', 'popover')
  await expect(region).toHaveAccessibleName('Legend')
  // The rows are the resident's text, in the DSH language (English here).
  await expect(row(panel, 'class-box')).toContainText('Class box')
  await expect(row(panel, 'component-box')).toContainText('Component box')
  await expect(row(panel, 'foreign-package')).toContainText('Foreign package')
  await expect(row(panel, 'quality-ramp')).toContainText('lower CRAP is better')
  await expect(row(panel, 'no-data')).toContainText('No data')
  await expect(row(panel, 'dependency-chain')).toContainText('Dependency chain')
  await expect(legend(panel).locator('[data-section]')).toHaveCount(3)
  await lane.screenshot(panel, 't50-legend-popover')

  // Esc closes the popover and stops there: the selection survives.
  await panel.page.keyboard.press('Escape')
  await expect(region).toBeHidden()
  await expect(panel.summary).toHaveAttribute('data-selected', /^:/)
  expect(panel.problems).toEqual([])
})

test('from 900px the legend is resident beside the diagram, and below it returns to the popover', async () => {
  const panel = await lane.openPanel('shop')
  await narrowSidebar(panel)
  await expect(panel.panel).toHaveAttribute('data-layout', 'drawer')
  await expect(legend(panel)).toBeHidden()

  // The host's fullscreen form widens the same tab; nothing remounts.
  await toggleFullscreen(panel, true)
  await expect.poll(() => panel.panel.evaluate(element => element.clientWidth)).toBeGreaterThanOrEqual(900)
  await expect(panel.panel).toHaveAttribute('data-layout', 'docked')
  const region = legend(panel)
  await expect(region).toBeVisible()
  await expect(region).toHaveAttribute('data-mode', 'docked')
  // Resident without being opened: no popover state and no close button.
  await expect(region).toHaveAttribute('data-open', 'false')
  await expect(region.getByRole('button', { name: 'Close legend' })).toBeHidden()

  // Pinned, not overlaid (F28): the canvas row reserves a strip below the
  // diagram, so the legend and the drawing share no pixel and the canvas keeps
  // the row's full width.
  const legendBox = await region.boundingBox()
  const canvasBox = await panel.canvas.boundingBox()
  const rowBox = await panel.panel.locator('.dsh-uml-viewer-canvas-row').boundingBox()
  expect(legendBox.y, 'no class sits under the legend').toBeGreaterThanOrEqual(canvasBox.y + canvasBox.height - 1)
  expect(Math.round(canvasBox.width - rowBox.width), 'the drawing keeps the whole width').toBeLessThanOrEqual(2)
  expect(Math.round(legendBox.width - rowBox.width), 'the strip spans the drawing').toBeLessThanOrEqual(2)
  expect(Math.round(legendBox.y + legendBox.height - (rowBox.y + rowBox.height)), 'the strip ends the row').toBeLessThanOrEqual(2)
  expect(legendBox.height, 'the resident legend is a readable strip').toBeGreaterThan(60)
  // The resident was told the narrower viewport, so the diagram is fitted to
  // what is left rather than clipped behind the legend.
  const viewport = await panel.canvas.evaluate(canvas => {
    const rect = canvas.getBoundingClientRect()
    return { width: Math.round(rect.width), height: Math.round(rect.height) }
  })
  expect(Math.abs(viewport.width - canvasBox.width)).toBeLessThanOrEqual(2)
  await lane.screenshot(panel, 't50-legend-docked')

  // The overlay root stops above the strip, so even a docked card — which
  // overlays the drawing (F28's open question, not this ticket's) — leaves the
  // resident legend in view.
  const frame = await producerFrame(panel, { root: lane.projects.shop, home: lane.home })
  const item = frame.world.find(entry => entry.kind === 'class' && entry.shape !== 'oval')
  expect(item, 'the frame has a class box').toBeTruthy()
  const rect = screenRect(frame, item.rect)
  await panel.canvas.dblclick({ position: { x: rect.x + 12, y: rect.y + 12 } })
  const card = panel.panel.getByRole('dialog').first()
  await expect(card).toBeVisible()
  await expect.poll(async () => {
    const box = await card.boundingBox()
    return Math.round(box.y + box.height - legendBox.y)
  }, { message: 'the docked card ends where the legend strip begins' }).toBeLessThanOrEqual(2)
  await expect(region).toBeVisible()
  await panel.page.keyboard.press('Escape')
  await expect(card).toHaveCount(0)
  await expect(region).toBeVisible()

  // Below 900px the resident legend withdraws into the popover again.
  await toggleFullscreen(panel, false)
  await expect(panel.panel).toHaveAttribute('data-layout', 'drawer')
  await expect(region).toBeHidden()
  await expect(region).toHaveAttribute('data-mode', 'popover')
  await moreButton(panel).click()
  await moreMenu(panel).getByRole('menuitem', { name: 'Legend', exact: true }).click()
  await expect(region).toBeVisible()
  await expect(region).toHaveAttribute('data-mode', 'popover')
  await panel.page.keyboard.press('Escape')
  await expect(region).toBeHidden()
  expect(panel.problems).toEqual([])
})

test('the legend swatches are the colors the diagram paints with', async () => {
  const panel = await lane.openPanel('inspector')
  // The hue-fixed roles keep their documented dark values (docs/scene-frame.md).
  await panel.page.evaluate(() => document.body.setAttribute('data-ds-dark-theme', ''))
  await narrowSidebar(panel)
  await moreButton(panel).click()
  await moreMenu(panel).getByRole('menuitem', { name: 'Legend', exact: true }).click()
  await expect(legend(panel)).toBeVisible()

  const style = (locator, property) => locator.evaluate((element, property) => getComputedStyle(element)[property], property)
  const swatch = (id, chip = 0) => row(panel, id).locator('.dsh-uml-legend-mark > span').nth(chip)
  // 质量色: the fill ramp's three sample stops, worst first.
  expect(await Promise.all([0, 1, 2].map(n => style(swatch('quality-ramp', n), 'backgroundColor'))))
    .toEqual(['rgb(74, 40, 24)', 'rgb(61, 58, 24)', 'rgb(30, 74, 56)'])
  await expect(row(panel, 'quality-ramp')).toContainText('Quality color')
  // 违规关系色, 依赖链高亮 and 无数据.
  expect(await style(swatch('violation'), 'borderTopColor')).toBe('rgb(196, 42, 36)')
  expect(await style(swatch('dependency-chain'), 'borderTopColor')).toBe('rgb(62, 152, 236)')
  expect(await style(swatch('no-data'), 'backgroundColor')).toBe('rgb(196, 42, 36)')
  // 关系淡化 is the plain relation color, faded by the frame's dim alpha.
  expect(await style(swatch('dimmed'), 'borderTopColor')).toBe(await style(swatch('relation'), 'borderTopColor'))
  expect(Number(await style(swatch('dimmed'), 'opacity'))).toBeCloseTo(70 / 255, 3)

  // 普通关系色 is the host's own token, so compare it with the pixels the
  // canvas painted for a plain relation in the same frame.
  const frame = await producerFrame(panel, { root: lane.projects.inspector, home: lane.home })
  const edge = frame.edges.find(item => item.kind === 'stroke' && item.stroke.role === 'relation')
  expect(edge, 'the diagram draws a plain relation').toBeTruthy()
  // Every point of the edge's route is on the line the canvas painted, so one
  // of them carries the relation color (antialiasing and the boxes' edges make
  // a single probe unreliable).
  const route = edge.points.map(([x, y]) => toScreen(frame, x, y))
  const probes = route.slice(1).flatMap((to, index) => {
    const from = route[index]
    // The route's end points sit inside the boxes, so probe the interior.
    return [0.2, 0.4, 0.5, 0.6, 0.8].map(part =>
      [from[0] + (to[0] - from[0]) * part, from[1] + (to[1] - from[1]) * part])
  })
  const pixels = await panel.canvas.evaluate((canvas, pts) => {
    const ctx = canvas.getContext('2d')
    const scale = canvas.width / canvas.getBoundingClientRect().width
    return pts.map(([x, y]) => Array.from(ctx.getImageData(Math.floor(x * scale), Math.floor(y * scale), 1, 1).data))
  }, probes)
  const expected = (await style(swatch('relation'), 'borderTopColor'))
    .match(/\d+/g).map(Number)
  const match = pixels.find(pixel => same(pixel, expected, 24))
  expect(match, `a plain relation on canvas is the legend's color ${expected}; probes ${JSON.stringify(pixels.slice(0, 8))} of ${pixels.length}, edge ${JSON.stringify(edge.points)}`).toBeTruthy()
  await lane.screenshot(panel, 't50-legend-colors')
  expect(panel.problems).toEqual([])
})

test('a language switch rewords the legend rows from the resident', async () => {
  const panel = await lane.openPanel('shop')
  await narrowSidebar(panel)
  await moreButton(panel).click()
  await moreMenu(panel).getByRole('menuitem', { name: 'Legend', exact: true }).click()
  await expect(row(panel, 'dependency-chain')).toContainText('Dependency chain')

  await switchLanguage(panel.page, '中文')
  // The panel's own title follows the locale; the rows follow the frame the
  // resident re-sends, typeset in the language it was told (#44, #50).
  await expect(legend(panel)).toHaveAccessibleName('图例')
  await expect(row(panel, 'dependency-chain')).toContainText('依赖链高亮')
  await expect(row(panel, 'class-box')).toContainText('类框')
  await expect(row(panel, 'no-data')).toContainText('无数据')
  await lane.screenshot(panel, 't50-legend-chinese')

  await switchLanguage(panel.page, 'English')
  await expect(legend(panel)).toHaveAccessibleName('Legend')
  await expect(row(panel, 'dependency-chain')).toContainText('Dependency chain')
  expect(panel.problems).toEqual([])
})
