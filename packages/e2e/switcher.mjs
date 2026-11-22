import { expect } from '@playwright/test'

export const switcherButton = panel => panel.panel.locator('.dsh-uml-viewer-switcher-button')

export const switcherMenu = (panel, label = 'Discussion contexts') =>
  panel.panel.getByRole('menu', { name: label, exact: true })

export async function openSwitcher(panel, label) {
  const menu = switcherMenu(panel, label)
  if (!(await menu.isVisible())) await switcherButton(panel).click()
  await expect(menu).toBeVisible()
  return menu
}

export async function chooseContext(panel, name, label) {
  const menu = await openSwitcher(panel, label)
  await menu.getByRole('menuitemradio', { name, exact: true }).click()
  await expect(switcherMenu(panel, label)).toHaveCount(0)
}

export async function newProposal(panel, name = 'New proposal', label) {
  const menu = await openSwitcher(panel, label)
  await menu.getByRole('menuitem', { name, exact: true }).click()
  await expect(switcherMenu(panel, label)).toHaveCount(0)
}

export async function listedContexts(panel, label) {
  const menu = await openSwitcher(panel, label)
  const names = await menu.getByRole('menuitemradio').allInnerTexts()
  await panel.page.keyboard.press('Escape')
  await expect(switcherMenu(panel, label)).toHaveCount(0)
  return names.map(name => name.replace(/^✓/, '').trim())
}
