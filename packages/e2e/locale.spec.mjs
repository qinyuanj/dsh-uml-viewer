import { test, expect } from '@playwright/test'
import { producerFrame, screenRect, toScreen } from './canvas.mjs'
import { launchViewerLane } from './lane.mjs'
import { switchLanguage } from './locale-switch.mjs'

// The top bar's 「更多」 menu and the status bar are the panel's own DOM (#46),
// so they follow the language too.
const moreButton = panel => panel.panel.locator('.dsh-uml-viewer-more')
const moreMenu = panel => panel.panel.locator('.dsh-uml-viewer-more-menu')
const statusBar = panel => panel.panel.locator('.dsh-uml-viewer-statusbar')
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
    // The search button is the top bar's control now (#46); its box exists
    // while it is open, and both names follow the language.
    const searchButton = shop.panel.getByRole('button', { name: '搜索类（Ctrl+F）' })
    await expect(searchButton).toBeVisible()
    await searchButton.click()
    await expect(shop.panel.getByRole('searchbox', { name: '搜索类' })).toBeVisible()
    await shop.page.keyboard.press('Escape')
    await expect(shop.panel.getByRole('searchbox')).toHaveCount(0)
    await expect(shop.canvas).toHaveAttribute('aria-label', 'UML 图画布')
    await expect(shop.summary).toContainText('讨论上下文：源码图')
    // The discussion-context switcher (#47) speaks the same language.
    const switcher = shop.panel.getByRole('button', { name: '讨论上下文：源码图', exact: true })
    await expect(switcher).toHaveAttribute('title', '讨论上下文：源码图。选择要讨论的图')
    await switcher.click()
    const contexts = shop.panel.getByRole('menu', { name: '讨论上下文', exact: true })
    await expect(contexts.getByRole('menuitemradio', { name: '源码图', exact: true })).toHaveAttribute('aria-checked', 'true')
    await expect(contexts.getByRole('menuitemradio', { name: 'Shop proposal', exact: true })).toBeVisible()
    await expect(contexts.getByRole('menuitem', { name: '新建提案', exact: true })).toBeVisible()
    await shop.page.keyboard.press('Escape')
    await expect(contexts).toHaveCount(0)
    await expect(elementMenu(shop)).toHaveAttribute('aria-label', '元素请求')
    expect(await menuItems(shop)).toEqual(['刷新 CRAP', '差量 mutation', '全量 mutation（--mutate-all）', '隐藏'])
    await shop.canvas.click({ position: { x: at[0], y: at[1] }, button: 'right' })
    await expect(shop.panel.getByRole('menu', { name: '元素请求' })).toHaveCount(1)
    await shop.page.keyboard.press('Escape')
    await expect(statusBar(shop)).toHaveAttribute('aria-label', '面板状态')
    await expect(moreButton(shop)).toHaveAccessibleName('更多操作')
    await moreButton(shop).click()
    await expect(moreMenu(shop)).toHaveAttribute('aria-label', '更多操作')
    await expect(moreMenu(shop).getByRole('menuitem', { name: /适配视图/ })).toHaveAttribute('aria-keyshortcuts', 'Ctrl+0')
    await expect(moreMenu(shop).getByRole('menuitemcheckbox', { name: '移除箭头', exact: true })).toBeVisible()
    await shop.page.keyboard.press('Escape')
    // #45: the top bar's breadcrumb path is our markup, so its name follows too.
    await expect(shop.panel.getByRole('navigation', { name: '面包屑路径' })).toBeVisible()

    await switchLanguage(shop.page, 'English')
    // The same mount: still the shop frame, relabelled in place.
    await expect(shop.summary).toHaveAttribute('data-kind', 'frame')
    const searchButtonEn = shop.panel.getByRole('button', { name: 'Search classes (Ctrl+F)' })
    await expect(searchButtonEn).toBeVisible()
    await searchButtonEn.click()
    await expect(shop.panel.getByRole('searchbox', { name: 'Search classes' })).toBeVisible()
    await shop.page.keyboard.press('Escape')
    await expect(shop.panel.getByRole('searchbox')).toHaveCount(0)
    await expect(shop.canvas).toHaveAttribute('aria-label', 'UML diagram canvas')
    await expect(shop.summary).toContainText('Discussion: source diagram')
    await expect(elementMenu(shop)).toHaveAttribute('aria-label', 'Element requests')
    expect(await menuItems(shop)).toEqual(['Refresh CRAP', 'Differential mutation', 'Full mutation (--mutate-all)', 'Hide'])
    await shop.canvas.click({ position: { x: at[0], y: at[1] }, button: 'right' })
    await expect(shop.panel.getByRole('menu', { name: 'Element requests' })).toHaveCount(1)
    await shop.page.keyboard.press('Escape')
    await expect(statusBar(shop)).toHaveAttribute('aria-label', 'Panel status')
    await expect(shop.panel.getByRole('button', { name: 'Discussion context: Source diagram', exact: true })).toBeVisible()
    await expect(moreButton(shop)).toHaveAccessibleName('More actions')
    await moreButton(shop).click()
    await expect(moreMenu(shop)).toHaveAttribute('aria-label', 'More actions')
    await expect(moreMenu(shop).getByRole('menuitem', { name: /Fit view/ })).toContainText('Ctrl+0')
    await expect(moreMenu(shop).getByRole('menuitemcheckbox', { name: 'Remove arrows', exact: true })).toBeVisible()
    await shop.page.keyboard.press('Escape')
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

/** The canvas status the resident sends (#45) is the bar's text, in the DSH language (#46). */
test('the status bar carries the canvas status and follows the language (#46)', async () => {
  const waiting = await lane.openPanel('shop', { display: false })
  await expect(statusBar(waiting)).toContainText('Waiting for :display to load the diagram…')
  await expect(statusBar(waiting)).toHaveAttribute('data-code', 'waiting-display')
  await switchLanguage(waiting.page, '中文')
  await expect(statusBar(waiting)).toContainText('等待 :display 命令加载图示…')
  await expect(statusBar(waiting)).toHaveAttribute('aria-label', '面板状态')
  await switchLanguage(waiting.page, 'English')
  await expect(statusBar(waiting)).toContainText('Waiting for :display to load the diagram…')
  expect(waiting.problems).toEqual([])
})

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
