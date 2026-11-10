/**
 * Switch the DSH language through the real host UI
 * (Account menu → Settings → General → Language). Shared by the locale,
 * legend and tool-view specs (#42).
 */
export async function switchLanguage(page, label) {
  await page.getByRole('button', { name: /^(Account menu|账号菜单)$/ }).first().click()
  await page.locator('[role^="menuitem"]').filter({ hasText: /^(Settings|设置)/ }).first().click()
  await page.getByText(/^(General|通用设置)$/).first().click()
  const row = page.getByText(/^(Language|语言)$/).first().locator('xpath=ancestor::*[.//button[@aria-haspopup="menu"]][1]')
  await row.locator('button[aria-haspopup="menu"]').click()
  await page.locator('[role^="menuitem"]').filter({ hasText: new RegExp(`^${label}$`) }).first().click()
  await page.keyboard.press('Escape')
}
