import { test, expect } from '@playwright/test'
import { launchViewerLane } from './lane.mjs'
import { producerFrame, screenRect } from './canvas.mjs'
import { TARGET_LINE, TARGET_TEXT } from './inspector-project.mjs'

// Browser counterparts of source_jump, source_window, detail_scroll and
// inspector drivers. Every model comes from the real IR/resident lane.
let lane

test.beforeAll(async ({ browser }) => { lane = await launchViewerLane(browser) })
test.afterAll(async () => { await lane?.close() })

const card = panel => panel.panel.getByRole('dialog', { name: '类卡片 Catalog', exact: true })
const sources = panel => panel.panel.locator('.dsh-uml-source[role="dialog"]')

async function openCatalog(panel) {
  const frame = await producerFrame(panel, { root: lane.projects.inspector, home: lane.home })
  const item = frame.world.find(item => item.kind === 'class' && item.id === ':catalog')
  expect(item, 'real producer frame contains the catalog module').toBeTruthy()
  const rect = screenRect(frame, item.rect)
  // Near the title's left edge, away from the inspector over the canvas right.
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

/** Native CSS resize handle, using the same mouse gesture as a user. */
async function resizeWindow(panel, window, width, height) {
  const before = await window.boundingBox()
  expect(before).toBeTruthy()
  await panel.page.mouse.move(before.x + before.width - 3, before.y + before.height - 3)
  await panel.page.mouse.down()
  await panel.page.mouse.move(before.x + width - 3, before.y + height - 3, { steps: 8 })
  await panel.page.mouse.up()
  await expect.poll(async () => Math.round((await window.boundingBox()).height)).toBe(height)
}

/** Click the exposed right edge when a small source window overlaps the card. */
async function clickRow(row) {
  await row.scrollIntoViewIfNeeded()
  const visible = await row.evaluate(element => {
    const row = element.getBoundingClientRect()
    const viewport = element.closest('.dsh-uml-card-scroll').getBoundingClientRect()
    return { x: Math.min(row.right, viewport.right) - row.left - 12, y: row.height / 2 }
  })
  await row.click({ position: visible })
}

async function cardNotch(panel, scroll, direction) {
  const before = await metrics(scroll)
  const expected = Math.max(0, Math.min(before.max, before.top + direction * 48))
  const box = await scroll.boundingBox()
  await panel.page.mouse.move(box.x + box.width / 2, box.y + box.height / 2)
  await panel.page.mouse.wheel(0, direction * 100)
  await expect.poll(async () => (await metrics(scroll)).top).toBe(expected)
}

test('inspector follows class selection, switches proposal, and P returns to the namespace tree', async () => {
  const panel = await lane.openPanel('inspector')
  const inspector = panel.panel.getByRole('complementary', { name: '检查器' })
  await expect(inspector).toBeVisible()
  const summary = inspector.locator('[data-summary-kind]')
  await expect(summary).toHaveAttribute('data-summary-kind', 'empty')
  await expect(summary).toContainText('单击类或命名空间查看摘要')
  await expect(inspector.getByRole('button', { name: 'Inspector', exact: true })).toHaveAttribute('aria-pressed', 'true')
  await openCatalog(panel)
  await expect(summary).toHaveAttribute('data-summary-kind', 'class')
  await expect(summary).toContainText('Catalog')
  await expect(summary).toContainText('无数据')
  await card(panel).getByRole('button', { name: '关闭类卡片' }).click()
  await expect(card(panel)).toHaveCount(0)

  const proposal = inspector.getByRole('button', { name: 'Inspector proposal', exact: true })
  await proposal.click()
  await expect(panel.summary).toHaveAttribute('data-view', 'proposal')
  await expect(proposal).toHaveAttribute('aria-pressed', 'true')
  await expect(panel.summary).toContainText('Inspector proposal')
  // Focus stays on the proposal button after choosing it. P must work from
  // the inspector as naturally as from the canvas, without touching chat.
  await panel.page.keyboard.press('p')
  await expect(panel.summary).toHaveAttribute('data-view', 'source')
  await expect(proposal).toHaveAttribute('aria-pressed', 'false')
  await expect(inspector.getByRole('button', { name: 'Inspector', exact: true })).toHaveAttribute('aria-pressed', 'true')
  await openCatalog(panel)
  await expect(panel.panel.locator('.dsh-uml-overlay-failure')).toHaveCount(0)
  expect(panel.problems).toEqual([])
})

test('class card wheel steps clamp at both ends, keep inspector visible, and resize columns', async () => {
  const panel = await lane.openPanel('inspector')
  const dialog = await openCatalog(panel)
  const panelWidth = await panel.panel.evaluate(element => element.clientWidth)
  const narrowWidth = Math.max(220, Math.min(400, Math.floor(panelWidth * 0.65)))
  await resizeWindow(panel, dialog, narrowWidth, 420)
  const scroll = dialog.locator('.dsh-uml-card-scroll')
  const inspector = panel.panel.locator('.dsh-uml-inspector')
  const proposal = inspector.getByRole('button', { name: 'Inspector proposal', exact: true })
  const proposalBefore = await proposal.boundingBox()
  const cameraBefore = await panel.summary.innerText()
  expect((await metrics(scroll)).horizontal, '560px minimum plan overflows a narrow card').toBeGreaterThan(100)
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
  await expect(proposal).toBeInViewport()
  expect(await proposal.boundingBox()).toEqual(proposalBefore)
  expect(await panel.summary.innerText(), 'card wheel does not zoom the diagram').toBe(cameraBefore)

  await resizeWindow(panel, dialog, Math.min(900, panelWidth - 24), 420)
  await expect.poll(async () => {
    const size = await metrics(scroll)
    const plan = await scroll.locator('.dsh-uml-card-plan').boundingBox()
    return Math.abs(plan.width - Math.max(560, size.width - 16))
  }).toBeLessThanOrEqual(1)
  const wide = await metrics(scroll)
  await lane.screenshot(panel, 't27-card')
  expect(wide.width).toBeGreaterThan(narrowWidth)
  if (wide.width >= 560) expect(wide.horizontal).toBe(0)
  await dialog.getByRole('button', { name: '关闭类卡片' }).click()
  await expect(dialog).toHaveCount(0)
  expect(panel.problems).toEqual([])
})

test('module source, exact member line, native copy, same-file reuse and independent source windows', async () => {
  const panel = await lane.openPanel('inspector')
  await panel.page.context().grantPermissions(['clipboard-read', 'clipboard-write'])
  const dialog = await openCatalog(panel)
  await dialog.locator('[data-card-module]').click()
  await expect(sources(panel)).toHaveCount(1)
  const source = sources(panel).first()
  await expect(source).toHaveAttribute('aria-label', /源码窗口 .*catalog\.ts$/)
  // The whole 84-line model fits inside one bounded DOM window (overscan included).
  await expect(source.locator('.dsh-uml-source-scroll')).toHaveAttribute('data-source-rows', '84')
  await expect(source.locator('[data-source-line]')).toHaveCount(84)
  await expect(source.locator('[data-marked=true]')).toHaveCount(0)
  expect((await metrics(source.locator('.dsh-uml-source-scroll'))).top).toBe(0)
  const original = await source.elementHandle()
  // Expose card rows without closing this file, so reuse is observable.
  await resizeWindow(panel, source, 260, 180)
  await clickRow(dialog.locator('[data-member="target"]'))
  await expect(sources(panel)).toHaveCount(1)
  expect(await source.evaluate((element, previous) => element === previous, original)).toBe(true)
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

  // The relationship row performs resident Locate and opens the other card.
  await clickRow(dialog.locator('[data-rel=":companion"]').first())
  const otherCard = panel.panel.getByRole('dialog', { name: '类卡片 Companion', exact: true })
  await expect(otherCard).toBeVisible()
  await clickRow(otherCard.locator('[data-card-module]'))
  await expect(sources(panel)).toHaveCount(2)
  await lane.screenshot(panel, 't27-source-windows')
  const second = sources(panel).filter({ has: panel.page.locator('[data-source-line="1"] code', { hasText: 'second file' }) })
  await expect(second).toHaveAttribute('aria-label', /companion\.ts$/)
  await second.getByRole('button', { name: '关闭源码窗口' }).click()
  await expect(sources(panel)).toHaveCount(1)
  await expect(sources(panel).first().locator(`[data-source-line="${TARGET_LINE}"][data-marked=true]`)).toBeInViewport()
  await sources(panel).first().getByRole('button', { name: '关闭源码窗口' }).click()
  await expect(sources(panel)).toHaveCount(0)
  await otherCard.getByRole('button', { name: '关闭类卡片' }).click()
  await expect(otherCard).toHaveCount(0)
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

test('inspector, card and source chrome follow host light and dark theme tokens', async () => {
  const panel = await lane.openPanel('inspector')
  const dialog = await openCatalog(panel)
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
      [panel.panel.locator('.dsh-uml-inspector'), 'backgroundColor', '--dsw-alias-bg-layer-3'],
      [dialog, 'backgroundColor', '--dsw-alias-bg-layer-2'],
      [dialog, 'color', '--dsw-alias-label-primary'],
      [source, 'backgroundColor', '--dsw-alias-bg-layer-2'],
      [source.locator('[data-marked=true]'), 'backgroundColor', '--dsw-alias-state-business-tertiary'],
      [source.locator('.dsh-uml-source-gutter').first(), 'backgroundColor', '--dsw-alias-bg-layer-3'],
      [source.locator('.dsh-uml-source-gutter').first(), 'color', '--dsw-alias-label-tertiary'],
      [source.locator('[data-token=keyword]').first(), 'color', '--dsw-alias-state-business-primary'],
      [source.locator('[data-token=string]').first(), 'color', '--dsw-alias-state-warn-primary'],
      [source.locator('[data-token=comment]').first(), 'color', '--dsw-alias-label-tertiary'],
      [dialog, 'borderTopColor', '--dsw-alias-border-l2'],
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
  expect(styles[0][0], 'inspector background switches with host theme').not.toBe(styles[1][0])
  expect(panel.problems).toEqual([])
})
