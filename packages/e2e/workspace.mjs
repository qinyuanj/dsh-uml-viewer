import { expect } from '@playwright/test'

/** Select through DSH's chooser before composing the first prompt. */
export async function chooseWorkspace(page, project) {
  const chooser = page.getByRole('button', { name: 'Choose workspace' })
  const conversation = page.locator('[data-conversation-session]:visible').first()
  // Until "New session" mounts its draft the chip shows its placeholder, and
  // DSH may mount that draft in the last used workspace. Wait for the draft
  // (getAttribute waits for the element) before reading the chip, so both
  // describe the same conversation; the reverse order raced in CI14.
  const previousSession = await conversation.getAttribute('data-conversation-session')
  const previousWorkspace = await chooser.innerText()
  await chooser.click()
  await page.locator('[role="menu"], [role="dialog"]').getByText(project, { exact: true }).last().click()
  await expect.poll(() => chooser.innerText(), { timeout: 30_000 }).toContain(project)
  if (previousWorkspace.trim() !== project) {
    // The label is optimistic. The replacement conversation is the boundary
    // that proves the first prompt will use the newly selected workspace.
    await expect.poll(async () => {
      const current = await conversation.getAttribute('data-conversation-session')
      return Boolean(current && current !== previousSession)
    }, {
      timeout: 30_000,
      message: `choosing ${project} replaces the draft conversation (chip was "${previousWorkspace.trim()}", session ${previousSession})`,
    }).toBe(true)
  }
}
