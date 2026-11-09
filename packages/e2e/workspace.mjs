import { expect } from '@playwright/test'

/** Select through DSH's chooser before composing the first prompt. */
export async function chooseWorkspace(page, project) {
  const chooser = page.getByRole('button', { name: 'Choose workspace' })
  const conversation = page.locator('[data-conversation-session]:visible').first()
  const previousWorkspace = await chooser.innerText()
  const previousSession = await conversation.getAttribute('data-conversation-session')
  await chooser.click()
  await page.locator('[role="menu"], [role="dialog"]').getByText(project, { exact: true }).last().click()
  await expect.poll(() => chooser.innerText(), { timeout: 30_000 }).toContain(project)
  if (previousWorkspace.trim() !== project) {
    // The label is optimistic. The replacement conversation is the boundary
    // that proves the first prompt will use the newly selected workspace.
    await expect.poll(async () => {
      const current = await conversation.getAttribute('data-conversation-session')
      return Boolean(current && current !== previousSession)
    }, { timeout: 30_000 }).toBe(true)
  }
}
