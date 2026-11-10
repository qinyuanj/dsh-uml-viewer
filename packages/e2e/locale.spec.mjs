import { test, expect } from '@playwright/test'
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
const menuItems = panel => panel.panel.locator('[role="menu"] [role="menuitem"]').allTextContents()

test('panel DOM text and accessible names follow a DSH language switch without reopening the tab (#42)', async ({ browser }) => {
  const lane = await launchViewerLane(browser)
  try {
    const shop = await lane.openPanel('shop')
    await switchLanguage(shop.page, '中文')
    await expect(shop.panel.getByRole('searchbox', { name: '搜索类' })).toBeVisible()
    await expect(shop.canvas).toHaveAttribute('aria-label', 'UML 图画布')
    await expect(shop.summary).toContainText('讨论上下文：源码图')
    await expect(shop.panel.getByRole('menu', { name: '元素请求', includeHidden: true })).toHaveCount(1)
    expect(await menuItems(shop)).toEqual(['刷新 CRAP', '差量 mutation', '全量 mutation（--mutate-all）', '隐藏'])
    await expect(declutter(shop)).toHaveAccessibleName(/^精简：/)

    await switchLanguage(shop.page, 'English')
    // The same mount: still the shop frame, relabelled in place.
    await expect(shop.summary).toHaveAttribute('data-kind', 'frame')
    await expect(shop.panel.getByRole('searchbox', { name: 'Search classes' })).toBeVisible()
    await expect(shop.canvas).toHaveAttribute('aria-label', 'UML diagram canvas')
    await expect(shop.summary).toContainText('Discussion: source diagram')
    await expect(shop.panel.getByRole('menu', { name: 'Element requests', includeHidden: true })).toHaveCount(1)
    expect(await menuItems(shop)).toEqual(['Refresh CRAP', 'Differential mutation', 'Full mutation (--mutate-all)', 'Hide'])
    await expect(declutter(shop)).toHaveAccessibleName(/^Declutter: /)
    await lane.screenshot(shop, 'locale-english')
  } finally {
    await lane.close()
  }
})
