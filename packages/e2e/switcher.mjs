/**
 * The top bar's discussion-context switcher (#47), driven the way a user
 * drives it: open the dropdown, pick a row. The rows are the source diagram,
 * each proposal and 「新建提案」 (English names in the default lane).
 */
import { expect } from '@playwright/test'

/** The switcher's trigger: its visible text is the discussion context's name. */
export const switcherButton = panel => panel.panel.locator('.dsh-uml-viewer-switcher-button')

/** The open dropdown; a closed one has no `menu` role. */
export const switcherMenu = (panel, label = 'Discussion contexts') =>
  panel.panel.getByRole('menu', { name: label, exact: true })

/** Open the dropdown and return its menu. */
export async function openSwitcher(panel, label) {
  const menu = switcherMenu(panel, label)
  if (!(await menu.isVisible())) await switcherButton(panel).click()
  await expect(menu).toBeVisible()
  return menu
}

/** Choose the source diagram or a proposal by its row name. */
export async function chooseContext(panel, name, label) {
  const menu = await openSwitcher(panel, label)
  await menu.getByRole('menuitemradio', { name, exact: true }).click()
  await expect(switcherMenu(panel, label)).toHaveCount(0)
}

/** The switcher's last row: ask the companion for a new proposal. */
export async function newProposal(panel, name = 'New proposal', label) {
  const menu = await openSwitcher(panel, label)
  await menu.getByRole('menuitem', { name, exact: true }).click()
  await expect(switcherMenu(panel, label)).toHaveCount(0)
}

/** The proposal rows the switcher lists now, by name. */
export async function listedContexts(panel, label) {
  const menu = await openSwitcher(panel, label)
  const names = await menu.getByRole('menuitemradio').allInnerTexts()
  await panel.page.keyboard.press('Escape')
  await expect(switcherMenu(panel, label)).toHaveCount(0)
  return names.map(name => name.replace(/^✓/, '').trim())
}
