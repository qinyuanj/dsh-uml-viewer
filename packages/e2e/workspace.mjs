import { expect } from '@playwright/test'

export async function chooseWorkspace(page, project) {
  const chooser = page.getByRole('button', { name: 'Choose workspace' })
  const conversation = page.locator('[data-conversation-session]:visible').first()
  // Until "New session" mounts its draft the chip shows its placeholder, and
  const previousSession = await conversation.getAttribute('data-conversation-session')
  const previousWorkspace = await chooser.innerText()
  await chooser.click()
  await page.locator('[role="menu"], [role="dialog"]').getByText(project, { exact: true }).last().click()
  await expect.poll(() => chooser.innerText(), { timeout: 30_000 }).toContain(project)
  if (previousWorkspace.trim() !== project) {
    await expect.poll(async () => {
      const current = await conversation.getAttribute('data-conversation-session')
      return Boolean(current && current !== previousSession)
    }, {
      timeout: 30_000,
      message: `choosing ${project} replaces the draft conversation (chip was "${previousWorkspace.trim()}", session ${previousSession})`,
    }).toBe(true)
  }
}
