import { test, expect } from '@playwright/test'
import { producerFrame, screenRect, toScreen } from './canvas.mjs'
import { launchViewerLane } from './lane.mjs'

/**
 * Switch the DSH language the way a user does: Settings → General → Language.
 * Settings lives in the sidebar's account menu ("Account menu"/"账号菜单");
 * the General page ("General"/"通用设置") holds the Language row, whose
 * selector is the menu button next to the "Language"/"语言" title.
 */
async function switchLanguage(page, label) {
  await page.getByRole('button', { name: /^(Account menu|账号菜单)$/ }).first().click()
  await page.locator('[role^="menuitem"]').filter({ hasText: /^(Settings|设置)/ }).first().click()
  await page.getByText(/^(General|通用设置)$/).first().click()
  const row = page.getByText(/^(Language|语言)$/).first().locator('xpath=ancestor::*[.//button[@aria-haspopup="menu"]][1]')
  await row.locator('button[aria-haspopup="menu"]').click()
  await page.locator('[role^="menuitem"]').filter({ hasText: new RegExp(`^${label}$`) }).first().click()
  await page.keyboard.press('Escape')
}

// The inspector (deleted by #47, not localized) has its own 精简 button.
const declutter = panel => panel.panel.locator('button.dsh-uml-viewer-declutter')
// The element menu carries `role="menu"` only while it is open: the host reads
// any such role in the document as an open modal and blocks the tab refresh
// command with it (#43), so a closed menu is found by its class instead.
const elementMenu = panel => panel.panel.locator('.dsh-uml-quality-menu')
const menuItems = panel => elementMenu(panel).locator('[role="menuitem"]').allTextContents()
/**
 * A canvas point on a class box, read from the producer once in English (the
 * summary the oracle parses is localized, and the view does not change when the
 * language does). Right-clicking there opens the element menu.
 */
async function classPoint(panel, lane, root) {
  const frame = await producerFrame(panel, { root, home: lane.home })
  const item = frame.world.find(entry => entry.kind === 'class' && entry.shape !== 'oval')
  expect(item, 'the frame has a class box').toBeTruthy()
  return toScreen(frame, item.rect.x + item.rect.w / 2, item.rect.y + 10)
}

test('panel DOM text and accessible names follow a DSH language switch without reopening the tab (#42)', async ({ browser }) => {
  const lane = await launchViewerLane(browser)
  try {
    const shop = await lane.openPanel('shop')
    const at = await classPoint(shop, lane, lane.projects.shop)
    await switchLanguage(shop.page, '中文')
    await expect(shop.panel.getByRole('searchbox', { name: '搜索类' })).toBeVisible()
    await expect(shop.canvas).toHaveAttribute('aria-label', 'UML 图画布')
    await expect(shop.summary).toContainText('讨论上下文：源码图')
    await expect(elementMenu(shop)).toHaveAttribute('aria-label', '元素请求')
    expect(await menuItems(shop)).toEqual(['刷新 CRAP', '差量 mutation', '全量 mutation（--mutate-all）', '隐藏'])
    await shop.canvas.click({ position: { x: at[0], y: at[1] }, button: 'right' })
    await expect(shop.panel.getByRole('menu', { name: '元素请求' })).toHaveCount(1)
    await shop.page.keyboard.press('Escape')
    await expect(declutter(shop)).toHaveAccessibleName(/^精简：/)
    // #45: the top bar's breadcrumb path is our markup, so its name follows too.
    await expect(shop.panel.getByRole('navigation', { name: '面包屑路径' })).toBeVisible()

    await switchLanguage(shop.page, 'English')
    // The same mount: still the shop frame, relabelled in place.
    await expect(shop.summary).toHaveAttribute('data-kind', 'frame')
    await expect(shop.panel.getByRole('searchbox', { name: 'Search classes' })).toBeVisible()
    await expect(shop.canvas).toHaveAttribute('aria-label', 'UML diagram canvas')
    await expect(shop.summary).toContainText('Discussion: source diagram')
    await expect(elementMenu(shop)).toHaveAttribute('aria-label', 'Element requests')
    expect(await menuItems(shop)).toEqual(['Refresh CRAP', 'Differential mutation', 'Full mutation (--mutate-all)', 'Hide'])
    await shop.canvas.click({ position: { x: at[0], y: at[1] }, button: 'right' })
    await expect(shop.panel.getByRole('menu', { name: 'Element requests' })).toHaveCount(1)
    await shop.page.keyboard.press('Escape')
    await expect(declutter(shop)).toHaveAccessibleName(/^Declutter: /)
    await expect(shop.panel.getByRole('navigation', { name: 'Breadcrumb path' })).toBeVisible()
    await lane.screenshot(shop, 'locale-english')
  } finally {
    await lane.close()
  }
})

// The class card, the 无数据 marker and the source error are laid out by the
// resident, so only the language the panel passes decides their wording (#44).
const CJK = /[\u3400-\u9fff]/

async function openCatalog(panel) {
  const frame = await producerFrame(panel, { root: lane.projects.inspector, home: lane.home })
  const item = frame.world.find(entry => entry.kind === 'class' && entry.id === ':catalog')
  expect(item, 'the producer frame has the catalog module').toBeTruthy()
  const rect = screenRect(frame, item.rect)
  await panel.canvas.dblclick({ position: { x: rect.x + 12, y: rect.y + 12 } })
  const card = panel.panel.getByRole('dialog').first()
  await expect(card).toBeVisible()
  return card
}

let lane

test.beforeAll(async ({ browser }) => { lane = await launchViewerLane(browser) })
test.afterAll(async () => { await lane?.close() })

test('the class card is worded by the resident in the DSH language, and switches in place (#44)', async () => {
  const panel = await lane.openPanel('inspector')
  const card = await openCatalog(panel)
  // DSH starts in English: every resident row is English, with no Chinese left.
  const english = await card.innerText()
  expect(english).toContain('Member')
  expect(english).not.toMatch(CJK)

  await switchLanguage(panel.page, '中文')
  await expect.poll(async () => card.innerText()).toContain('成员')
  expect(await card.innerText()).toMatch(CJK)

  // The same open card follows the switch back, without reopening the tab.
  await switchLanguage(panel.page, 'English')
  await expect.poll(async () => card.innerText()).toContain('Member')
  expect(await card.innerText()).not.toMatch(CJK)
  await lane.screenshot(panel, 'locale-resident-card')
})
