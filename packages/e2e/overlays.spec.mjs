import { test, expect } from '@playwright/test'
import { launchViewerLane } from './lane.mjs'
import { producerFrame, screenRect, toScreen } from './canvas.mjs'
import { TARGET_LINE, TARGET_TEXT } from './inspector-project.mjs'
import { chooseContext, switcherButton } from './switcher.mjs'
import { toggleFullscreen } from './lifecycle.mjs'

// Browser counterparts of the deleted source_jump, source_window,
// detail_scroll, review_fixes and inspector window drivers (#30); the inspector itself was deleted in #47.
// Every model comes from the real IR/resident lane.
let lane

test.beforeAll(async ({ browser }) => { lane = await launchViewerLane(browser) })
test.afterAll(async () => { await lane?.close() })

const card = panel => panel.panel.getByRole('dialog', { name: 'Class card Catalog', exact: true })
const sources = panel => panel.panel.locator('.dsh-uml-source[role="dialog"]')
/** The class card / source drawer (#49): there is never more than one. */
const drawers = panel => panel.panel.locator('.dsh-uml-drawer')
const layout = panel => panel.panel.getAttribute('data-layout')

async function openCatalog(panel) {
  const frame = await producerFrame(panel, { root: lane.projects.inspector, home: lane.home })
  const item = frame.world.find(item => item.kind === 'class' && item.id === ':catalog')
  expect(item, 'real producer frame contains the catalog module').toBeTruthy()
  const rect = screenRect(frame, item.rect)
  // Near the title's left edge.
  await panel.canvas.dblclick({ position: { x: rect.x + 12, y: rect.y + 12 } })
  await expect(card(panel)).toBeVisible()
  return card(panel)
}

async function metrics(scroll) {
  return scroll.evaluate(element => ({
    top: element.scrollTop, left: element.scrollLeft,
    max: element.scrollHeight - element.clientHeight,
    horizontal: element.scrollWidth - element.clientWidth,
    width: element.clientWidth,
  }))
}

/** A card row, scrolled into the drawer's viewport first. */
async function clickRow(row) {
  await row.scrollIntoViewIfNeeded()
  const visible = await row.evaluate(element => {
    const row = element.getBoundingClientRect()
    const viewport = element.closest('.dsh-uml-card-scroll').getBoundingClientRect()
    return { x: Math.min(row.right, viewport.right) - row.left - 12, y: row.height / 2 }
  })
  await row.click({ position: visible })
}

/**
 * Narrow the sidebar to about 400px with the host's own width handle when the
 * default column is wider; the panel's container width is what the layout reads.
 */
async function narrowSidebar(panel, width = 400) {
  const current = await panel.panel.evaluate(element => element.clientWidth)
  if (current <= width + 40) return current
  const box = await panel.panel.boundingBox()
  // The resize handle sits on the column's left edge.
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
    // No handle on this host build: a narrower window gives a narrower column.
    const viewport = panel.page.viewportSize()
    await panel.page.setViewportSize({ width: Math.max(800, Math.round(viewport.width - (box.width - width) * 2)), height: viewport.height })
  }
  return panel.panel.evaluate(element => element.clientWidth)
}

async function cardNotch(panel, scroll, direction) {
  const before = await metrics(scroll)
  const expected = Math.max(0, Math.min(before.max, before.top + direction * 48))
  const box = await scroll.boundingBox()
  await panel.page.mouse.move(box.x + box.width / 2, box.y + box.height / 2)
  await panel.page.mouse.wheel(0, direction * 100)
  await expect.poll(async () => (await metrics(scroll)).top).toBe(expected)
}

test('no inspector: the switcher changes the discussion, P returns to the source tree root (#47)', async () => {
  const panel = await lane.openPanel('inspector')
  // The inspector, its summary line and its P hint button are gone.
  await expect(panel.panel.getByRole('complementary')).toHaveCount(0)
  await expect(panel.panel.getByText('检查器')).toHaveCount(0)
  await expect(panel.panel.locator('[data-summary-kind]')).toHaveCount(0)
  await expect(switcherButton(panel)).toHaveText(/Source diagram/)
  await openCatalog(panel)
  await card(panel).getByRole('button', { name: 'Close class card' }).click()
  await expect(card(panel)).toHaveCount(0)

  await chooseContext(panel, 'Inspector proposal')
  await expect(panel.summary).toHaveAttribute('data-view', 'proposal')
  await expect(panel.summary).toHaveAttribute('data-context', 'proposal')
  await expect(panel.summary).toContainText('Inspector proposal')
  await expect(switcherButton(panel)).toHaveText(/Inspector proposal/)
  // The switcher hands the keyboard back to the canvas; P there is the same
  // as choosing the source diagram: the discussion and the tree root.
  await panel.page.keyboard.press('p')
  await expect(panel.summary).toHaveAttribute('data-view', 'source')
  await expect(panel.summary).toHaveAttribute('data-context', 'real')
  await expect(panel.summary).toHaveAttribute('data-depth', 'root')
  await expect(switcherButton(panel)).toHaveText(/Source diagram/)
  await openCatalog(panel)
  await expect(panel.panel.locator('.dsh-uml-overlay-failure')).toHaveCount(0)
  expect(panel.problems).toEqual([])
})

test('in a narrow sidebar the class card is a bottom drawer over the lower half; wheel steps clamp and it scrolls sideways', async () => {
  const panel = await lane.openPanel('inspector')
  const width = await narrowSidebar(panel)
  expect(width, 'the sidebar is about 400px wide').toBeLessThan(520)
  await expect(panel.panel).toHaveAttribute('data-layout', 'drawer')
  const dialog = await openCatalog(panel)
  await expect(drawers(panel)).toHaveCount(1)
  // The drawer takes the lower half of the canvas; the diagram shows above it.
  const area = await panel.panel.locator('.dsh-uml-viewer-canvas-area').boundingBox()
  const drawer = await dialog.boundingBox()
  expect(Math.abs(drawer.y + drawer.height - (area.y + area.height))).toBeLessThanOrEqual(2)
  expect(Math.abs(drawer.height - area.height / 2)).toBeLessThanOrEqual(2)
  expect(Math.abs(drawer.width - area.width)).toBeLessThanOrEqual(2)
  const canvas = await panel.canvas.boundingBox()
  expect(drawer.y - canvas.y, 'the upper half of the diagram stays visible').toBeGreaterThan(canvas.height * 0.4)
  await lane.screenshot(panel, 't49-drawer-narrow')

  const scroll = dialog.locator('.dsh-uml-card-scroll')
  const cameraBefore = await panel.summary.innerText()
  expect((await metrics(scroll)).horizontal, 'the 560px column plan scrolls sideways in a narrow drawer').toBeGreaterThan(50)
  expect((await metrics(scroll)).max).toBeGreaterThan(192)
  for (let n = 0; n < 4; n += 1) await cardNotch(panel, scroll, 1)
  expect((await metrics(scroll)).top).toBe(192)
  const maximum = (await metrics(scroll)).max
  for (let n = 0; n < Math.ceil(maximum / 48) + 2; n += 1) await cardNotch(panel, scroll, 1)
  expect((await metrics(scroll)).top).toBe(maximum)
  for (let n = 0; n < Math.ceil(maximum / 48) + 2; n += 1) await cardNotch(panel, scroll, -1)
  expect((await metrics(scroll)).top).toBe(0)
  // Native keyboard scrolling must update the resident too, so the next wheel
  // starts at the native offset instead of jumping back to one notch.
  await scroll.focus()
  await panel.page.keyboard.press('End')
  await expect.poll(async () => (await metrics(scroll)).top).toBe(maximum)
  await cardNotch(panel, scroll, -1)
  expect((await metrics(scroll)).top).toBe(maximum - 48)
  await panel.page.keyboard.press('Home')
  await expect.poll(async () => (await metrics(scroll)).top).toBe(0)
  await cardNotch(panel, scroll, 1)
  expect((await metrics(scroll)).top).toBe(48)
  expect(await panel.summary.innerText(), 'card wheel does not zoom the diagram').toBe(cameraBefore)
  await dialog.getByRole('button', { name: 'Close class card' }).click()
  await expect(drawers(panel)).toHaveCount(0)
  expect(panel.problems).toEqual([])
})

test('a member source replaces the card in the same drawer at its line; back returns to the card', async () => {
  const panel = await lane.openPanel('inspector')
  await panel.page.context().grantPermissions(['clipboard-read', 'clipboard-write'])
  const dialog = await openCatalog(panel)
  await dialog.locator('[data-card-module]').click()
  await expect(sources(panel)).toHaveCount(1)
  await expect(card(panel)).toHaveCount(0)
  await expect(drawers(panel)).toHaveCount(1)
  const source = sources(panel).first()
  await expect(source).toHaveAttribute('aria-label', /Source window .*catalog\.ts$/)
  // The 84-line model renders through the bounded row window from line 1;
  // a half-height drawer may leave the last rows to the overscan of a scroll.
  await expect(source.locator('.dsh-uml-source-scroll')).toHaveAttribute('data-source-rows', '84')
  await expect(source.locator('[data-source-line="1"]')).toBeInViewport()
  expect(await source.locator('[data-source-line]').count()).toBeLessThanOrEqual(84)
  await expect(source.locator('[data-marked=true]')).toHaveCount(0)
  expect((await metrics(source.locator('.dsh-uml-source-scroll'))).top).toBe(0)

  await source.getByRole('button', { name: 'Back to class card' }).click()
  await expect(card(panel)).toBeVisible()
  await expect(sources(panel)).toHaveCount(0)
  await expect(drawers(panel)).toHaveCount(1)

  await clickRow(card(panel).locator('[data-member="target"]'))
  await expect(sources(panel)).toHaveCount(1)
  await expect(card(panel)).toHaveCount(0)
  await expect(drawers(panel)).toHaveCount(1)
  await expect(source).toHaveAttribute('aria-label', /catalog\.ts:82$/)
  const target = source.locator(`[data-source-line="${TARGET_LINE}"][data-marked=true]`)
  await expect(source.locator('[data-marked=true]')).toHaveCount(1)
  await expect(target.locator('code')).toHaveText(TARGET_TEXT)
  await expect(target).toBeInViewport()
  await expect.poll(async () => (await metrics(source.locator('.dsh-uml-source-scroll'))).top).toBeGreaterThan(0)
  expect(await target.evaluate(row => {
    const viewport = row.closest('.dsh-uml-source-scroll').getBoundingClientRect()
    const rect = row.getBoundingClientRect()
    return rect.top >= viewport.top && rect.bottom <= viewport.bottom
  }), 'line 82 is inside its scrolled source viewport').toBe(true)
  await expect(target.locator('.dsh-uml-source-gutter')).toHaveCSS('user-select', 'none')
  await lane.screenshot(panel, 't49-drawer-source')
  const keyword = await target.locator('code').getByText('function', { exact: true }).boundingBox()
  expect(keyword).toBeTruthy()
  // A real drag selects the word's exact glyph range. Native Windows word
  // double-click includes a trailing space, unlike the driver's exact drag.
  await panel.page.mouse.move(keyword.x + 0.1, keyword.y + keyword.height / 2)
  await panel.page.mouse.down()
  await panel.page.mouse.move(keyword.x + keyword.width - 0.1, keyword.y + keyword.height / 2, { steps: 8 })
  await panel.page.mouse.up()
  expect(await panel.page.evaluate(() => getSelection().toString())).toBe('function')
  await panel.page.keyboard.press(process.platform === 'darwin' ? 'Meta+c' : 'Control+c')
  await expect.poll(() => panel.page.evaluate(() => navigator.clipboard.readText())).toBe('function')

  await source.getByRole('button', { name: 'Back to class card' }).click()
  await expect(card(panel)).toBeVisible()
  // The relationship row performs resident Locate; its card takes the drawer.
  await clickRow(card(panel).locator('[data-rel=":companion"]').first())
  const otherCard = panel.panel.getByRole('dialog', { name: 'Class card Companion', exact: true })
  await expect(otherCard).toBeVisible()
  await expect(drawers(panel)).toHaveCount(1)
  await clickRow(otherCard.locator('[data-card-module]'))
  await expect(sources(panel).first()).toHaveAttribute('aria-label', /companion\.ts$/)
  await expect(drawers(panel)).toHaveCount(1)
  // Closing the source closes the whole drawer.
  await sources(panel).first().getByRole('button', { name: 'Close source window' }).click()
  await expect(drawers(panel)).toHaveCount(0)
  expect(panel.problems).toEqual([])
})

test('Esc closes the drawer first, from the source or the canvas, and only then goes back a level', async () => {
  const deep = await lane.openPanel('deep')
  const root = await producerFrame(deep, { root: lane.projects.deep, home: lane.home })
  const core = root.world.find(item => item.id === ':core' && (item.kind === 'class' || item.kind === 'package'))
  await lane.point(deep, toScreen(root, core.rect.x + core.rect.w / 2, core.rect.y + core.rect.h / 2), 2)
  await expect(deep.summary).toHaveAttribute('data-depth', 'drilled', { timeout: 30_000 })
  const drilled = await producerFrame(deep, { root: lane.projects.deep, home: lane.home, focus: ':core' })
  const order = drilled.world.find(item => item.kind === 'class' && item.id === ':core.order')
  const at = toScreen(drilled, order.rect.x + 12, order.rect.y + 12)
  const orderCard = deep.panel.getByRole('dialog', { name: /^Class card / })

  // From the canvas: the first Esc closes the card, the depth stays.
  await lane.point(deep, at, 2)
  await expect(orderCard).toBeVisible()
  await deep.canvas.focus()
  await deep.page.keyboard.press('Escape')
  await expect(drawers(deep)).toHaveCount(0)
  await expect(deep.summary).toHaveAttribute('data-depth', 'drilled')

  // From the source shown in the drawer: Esc closes the whole drawer.
  await lane.point(deep, at, 2)
  await expect(orderCard).toBeVisible()
  await clickRow(orderCard.locator('[data-card-module]'))
  await expect(sources(deep)).toHaveCount(1)
  await expect(sources(deep).locator('.dsh-uml-source-scroll')).toBeFocused()
  await deep.page.keyboard.press('Escape')
  await expect(drawers(deep)).toHaveCount(0)
  await expect(deep.summary).toHaveAttribute('data-depth', 'drilled')
  await expect(deep.canvas).toBeFocused()
  // With nothing left to close, Esc goes back a level.
  await deep.page.keyboard.press('Escape')
  await expect(deep.summary).toHaveAttribute('data-depth', 'root')
  expect(deep.problems).toEqual([])
})

test('from 900px of panel width the class card docks on the right beside the diagram, and below it returns to the drawer', async () => {
  const panel = await lane.openPanel('inspector')
  await narrowSidebar(panel)
  await expect(panel.panel).toHaveAttribute('data-layout', 'drawer')
  const dialog = await openCatalog(panel)
  // The host's fullscreen form widens the same tab; nothing remounts.
  await toggleFullscreen(panel, true)
  await expect.poll(() => panel.panel.evaluate(element => element.clientWidth)).toBeGreaterThanOrEqual(900)
  await expect(panel.panel).toHaveAttribute('data-layout', 'docked')
  await expect(dialog).toBeVisible()
  await expect(drawers(panel)).toHaveCount(1)
  const area = await panel.panel.locator('.dsh-uml-viewer-canvas-area').boundingBox()
  await expect.poll(async () => {
    const box = await dialog.boundingBox()
    return [Math.round(box.x + box.width - (area.x + area.width)), Math.round(box.y - area.y), Math.round(box.height - area.height)]
  }, { message: 'the docked card runs down the right edge of the canvas' }).toEqual([0, 0, 0])
  const docked = await dialog.boundingBox()
  expect(docked.width, 'the diagram keeps the larger share beside the card').toBeLessThan(area.width / 2)
  expect(docked.width, 'the docked column keeps a readable minimum').toBeGreaterThanOrEqual(419)
  await lane.screenshot(panel, 't49-docked')

  // Below 900px the card is the bottom drawer again.
  await toggleFullscreen(panel, false)
  await expect(panel.panel).toHaveAttribute('data-layout', 'drawer')
  await expect.poll(async () => {
    const box = await dialog.boundingBox()
    const canvas = await panel.panel.locator('.dsh-uml-viewer-canvas-area').boundingBox()
    return Math.round(box.width - canvas.width)
  }).toBe(0)
  await dialog.getByRole('button', { name: 'Close class card' }).click()
  await expect(drawers(panel)).toHaveCount(0)
  expect(panel.problems).toEqual([])
})

/** Compare computed CSS colors with independently resolved host theme tokens. */
async function tokenColor(locator, property, token) {
  return locator.evaluate((element, { property, token }) => {
    const probe = document.createElement('span')
    probe.style.color = `var(${token})`
    element.append(probe)
    const expected = getComputedStyle(probe).color
    const actual = getComputedStyle(element)[property]
    const defined = getComputedStyle(element).getPropertyValue(token).trim()
    probe.remove()
    return { expected, actual, defined }
  }, { property, token })
}

test('card and source chrome follow host light and dark theme tokens', async () => {
  const panel = await lane.openPanel('inspector')
  const dialog = await openCatalog(panel)
  const cardColors = []
  for (const dark of [false, true]) {
    await panel.page.evaluate(dark => {
      if (dark) document.body.setAttribute('data-ds-dark-theme', '')
      else document.body.removeAttribute('data-ds-dark-theme')
    }, dark)
    for (const [property, token] of [['backgroundColor', '--dsw-alias-bg-layer-2'], ['color', '--dsw-alias-label-primary'], ['borderTopColor', '--dsw-alias-border-l2']]) {
      await expect.poll(async () => {
        const value = await tokenColor(dialog, property, token)
        return value.defined !== '' && value.actual === value.expected
      }, { message: `${dark ? 'dark' : 'light'} card ${property} resolves ${token}` }).toBe(true)
      cardColors.push((await tokenColor(dialog, property, token)).actual)
    }
  }
  expect(cardColors.slice(0, 3), 'host theme changes the card surface').not.toEqual(cardColors.slice(3))
  // The source replaces the card in the drawer, so the source surfaces are
  // checked on their own.
  await clickRow(dialog.locator('[data-member="target"]'))
  const source = sources(panel).first()
  await expect(source).toBeVisible()
  await expect(source.locator(`[data-source-line="${TARGET_LINE}"][data-marked=true]`)).toBeInViewport()
  const styles = []
  for (const dark of [false, true]) {
    await panel.page.evaluate(dark => {
      if (dark) document.body.setAttribute('data-ds-dark-theme', '')
      else document.body.removeAttribute('data-ds-dark-theme')
    }, dark)
    const surfaces = [
      [source, 'backgroundColor', '--dsw-alias-bg-layer-2'],
      [source.locator('[data-marked=true]'), 'backgroundColor', '--dsw-alias-state-business-tertiary'],
      [source.locator('.dsh-uml-source-gutter').first(), 'backgroundColor', '--dsw-alias-bg-layer-3'],
      [source.locator('.dsh-uml-source-gutter').first(), 'color', '--dsw-alias-label-tertiary'],
      [source.locator('[data-token=keyword]').first(), 'color', '--dsw-alias-state-business-primary'],
      [source.locator('[data-token=string]').first(), 'color', '--dsw-alias-state-warn-primary'],
      [source.locator('[data-token=comment]').first(), 'color', '--dsw-alias-label-tertiary'],
      [source, 'borderTopColor', '--dsw-alias-border-l2'],
    ]
    const colors = []
    for (const [element, property, token] of surfaces) {
      await expect.poll(async () => {
        const value = await tokenColor(element, property, token)
        return value.defined !== '' && value.actual === value.expected
      }, { message: `${dark ? 'dark' : 'light'} ${property} resolves ${token}` }).toBe(true)
      colors.push((await tokenColor(element, property, token)).actual)
    }
    styles.push(colors)
  }
  expect(styles[0], 'host theme changes overlay surfaces and text').not.toEqual(styles[1])
  expect(styles[0][0], 'source background switches with host theme').not.toBe(styles[1][0])
  expect(panel.problems).toEqual([])
})
