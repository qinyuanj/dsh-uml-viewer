import fs from 'node:fs/promises'
import path from 'node:path'
import { expect } from '@playwright/test'
import { SCRATCH, bootDshWeb, dismissOverlays, startDshWeb } from './dsh-web.mjs'
import { startMockLlm } from './mock-llm.mjs'
import { makeProjects } from './projects.mjs'

/**
 * Reusable #22–#29 browser lane: one scratch host/mock model, one browser
 * context per DSH session, and only UI-level panel reads and gestures.
 * `reopen` reloads the Web client; `restart` stops and boots the DSH host on
 * the same scratch home, so saved session state is exercised for real.
 */
export async function launchViewerLane(browser) {
  await fs.mkdir(SCRATCH, { recursive: true })
  const home = await fs.mkdtemp(path.join(SCRATCH, 'run-'))
  const projects = await makeProjects(home)
  const mock = await startMockLlm({ log: path.join(home, 'mock-requests') })
  const installed = await startDshWeb({ home, projects })
  let host = await bootDshWeb({ ...installed, home, mockUrl: mock.url })
  const pages = []
  const close = async () => {
    for (const page of pages.splice(0).reverse()) await page.context().close().catch(() => {})
    host.stop()
    await host.exited.catch(() => {})
    await mock.stop()
  }

  async function newPage() {
    const context = await browser.newContext({ viewport: { width: 1440, height: 1000 } })
    const page = await context.newPage()
    pages.push(page)
    const problems = []
    page.on('pageerror', error => problems.push(error.message))
    // On a busy Windows host the preview notice can arrive after the initial
    // quiet window. Dismiss it through its real button whenever it obstructs
    // a later user action, rather than sleeping or forcing clicks through it.
    await page.addLocatorHandler(page.getByRole('dialog', { name: 'Preview Notice' }), async dialog => {
      await dialog.getByRole('button', { name: 'Continue', exact: true }).click()
    })
    return { page, problems }
  }

  async function openSidebar(page) {
    const existing = page.locator('.dsh-uml-viewer-panel').last()
    if (!await existing.isVisible().catch(() => false)) {
      const expand = page.locator('[data-sidebar-right-expand]').first()
      if (await expand.isVisible().catch(() => false)) await expand.click({ timeout: 30_000 })
      const uml = page.getByText('UML', { exact: true }).last()
      if (await uml.count()) await uml.click().catch(() => {})
    }
    const panel = page.locator('.dsh-uml-viewer-panel').last()
    await expect(panel).toBeVisible({ timeout: 30_000 })
    const canvas = panel.locator('canvas')
    const summary = panel.locator('.dsh-uml-viewer-summary')
    await expect(canvas).toBeVisible()
    await expect(summary).toHaveAttribute('data-kind', 'frame', { timeout: 30_000 })
    return { panel, canvas, summary }
  }

  async function result(page, problems, opened) {
    const sessionId = await page.locator('[data-conversation-session]:visible').first().getAttribute('data-conversation-session')
    if (!sessionId) throw new Error('the DSH conversation exposes no session id')
    expect(problems).toEqual([])
    return { page, ...opened, sessionId, before: await opened.summary.innerText(), problems }
  }

  async function openPanel(project) {
    const { page, problems } = await newPage()
    try {
      await page.goto(host.url, { waitUntil: 'domcontentloaded', timeout: 60_000 })
      await dismissOverlays(page)
      {
        await page.getByRole('button', { name: 'New session', exact: true }).first().click()
        await page.getByRole('button', { name: 'Choose workspace' }).click()
        await page.locator('[role="menu"], [role="dialog"]').getByText(project, { exact: true }).last().click()
        await expect(page.getByRole('button', { name: 'Choose workspace' })).toContainText(project)
        // Workspace selection replaces the new-session composer asynchronously;
        // the composer assertion below proves which session receives the prompt.
      }
      const composer = page.locator('[data-composer-input][contenteditable="true"]:visible').last()
      await composer.click({ timeout: 30_000 })
      if (project !== 'shop') {
        const chosen = await page.getByRole('button', { name: 'Choose workspace' }).innerText()
        if (!chosen.toLowerCase().includes(project)) {
          throw new Error(`workspace selection did not settle: expected ${project}, got ${chosen}`)
        }
      }
      await composer.fill(`Open the ${project} sample project`)
      await composer.press('Enter')
      const sessionNode = page.locator('[data-conversation-session]:visible').first()
      await expect(sessionNode).toBeVisible({ timeout: 30_000 })
      await expect(page.getByText('Generated the sample project IR.', { exact: true }).last()).toBeVisible()
      if (project === 'ledger') {
        const sessionId = await sessionNode.getAttribute('data-conversation-session')
        if (!sessionId) throw new Error('the DSH conversation exposes no session id')
        const stateDirectory = path.join(home, 'plugin-state', 'sessions')
        await fs.mkdir(stateDirectory, { recursive: true })
        await fs.writeFile(path.join(stateDirectory, `${sessionId}.json`), JSON.stringify({
          schema: 'dsh-uml-viewer/session-state',
          version: 1,
          diagram: path.join(projects.ledger, 'diagram.edn'),
          context: {
            kind: 'proposal',
            proposalId: ':p-ledger',
            name: 'Ledger proposal',
            edn: '{:context :proposal :name "Ledger proposal" :proposal-id :p-ledger}',
          },
          focus: [],
          proposalId: ':p-ledger',
          openLayer: null,
          declutter: null,
          camera: { x: 0, y: 0, zoom: 1 },
          selected: null,
          detail: null,
        }))
      }
      const opened = await openSidebar(page)
      await composer.click()
      await composer.fill('Render the sample diagram')
      await composer.press('Enter')
      await expect(page.getByText('The isolated mock model displayed the generated IR.', { exact: false }).last())
        .toBeVisible({ timeout: 45_000 })
      await expect(opened.summary).toContainText(`${project[0].toUpperCase()}${project.slice(1)}`, { timeout: 45_000 })
      if (project === 'ledger') await expect(opened.summary).toHaveAttribute('data-view', 'proposal')
      return await result(page, problems, opened)
    } catch (error) {
      await fs.writeFile(path.join(home, `${project}-failure.txt`), `${error.stack}\\n\\n${await page.locator('body').innerText().catch(() => '')}\\n\\n${host.output()}`)
      await page.screenshot({ path: path.join(home, `${project}-failure.png`), fullPage: true }).catch(() => {})
      throw error
    }
  }

  /** Reopen the same conversation's panel without running IR or display again. */
  async function reopenPanel(panel) {
    const { page, problems } = panel
    await page.reload({ waitUntil: 'domcontentloaded', timeout: 60_000 })
    await dismissOverlays(page)
    await expect(page.locator('[data-conversation-session]:visible').first()).toHaveAttribute('data-conversation-session', panel.sessionId)
    const opened = await openSidebar(page)
    return result(page, problems, opened)
  }

  /** Stop the isolated host, boot it on the same home, and reopen that session. */
  async function restart(panel) {
    const sessionId = panel.sessionId
    await panel.page.context().close().catch(() => {})
    host.stop()
    await host.exited.catch(() => {})
    host = await bootDshWeb({ ...installed, home, mockUrl: mock.url })
    const { page, problems } = await newPage()
    await page.goto(host.url, { waitUntil: 'domcontentloaded', timeout: 60_000 })
    await dismissOverlays(page)
    // A fresh browser context boots DSH's new-session page. Reopen the saved
    // conversation through its workspace-tree row, as a user does.
    const row = page.locator(`[data-row-key="session:${sessionId}"]`)
    await expect(row).toBeVisible({ timeout: 30_000 })
    await row.click()
    await expect(page.locator('[data-conversation-session]:visible').first()).toHaveAttribute('data-conversation-session', sessionId, { timeout: 30_000 })
    const opened = await openSidebar(page)
    return result(page, problems, opened)
  }

  async function pan(panel) {
    const box = await panel.canvas.boundingBox()
    if (!box) throw new Error('panel canvas has no box')
    await panel.page.mouse.move(box.x + box.width / 2, box.y + box.height / 2)
    await panel.page.mouse.down()
    await panel.page.mouse.move(box.x + box.width / 2 + 80, box.y + box.height / 2 + 30, { steps: 6 })
    await panel.page.mouse.up()
  }

  /** Real Ctrl+wheel notches over the canvas centre; positive `notches` zoom out. */
  async function wheel(panel, notches) {
    const box = await panel.canvas.boundingBox()
    if (!box) throw new Error('panel canvas has no box')
    await panel.page.mouse.move(box.x + box.width / 2, box.y + box.height / 2)
    await panel.page.keyboard.down('Control')
    try {
      for (let i = 0; i < Math.abs(notches); i += 1) {
        // Browser deltaY positive is down, hence zoom out with Control.
        await panel.page.mouse.wheel(0, Math.sign(notches) * 100)
        await panel.page.waitForTimeout(30)
      }
    } finally {
      await panel.page.keyboard.up('Control')
    }
  }

  /** Click or double-click a canvas-local CSS point. */
  async function point(panel, [x, y], clicks = 1) {
    const box = await panel.canvas.boundingBox()
    if (!box) throw new Error('panel canvas has no box')
    await panel.page.mouse.click(box.x + x, box.y + y, { clickCount: clicks })
  }

  async function sendPrompt(panel, text) {
    const composer = panel.page.locator('[data-composer-input][contenteditable="true"]:visible').last()
    await composer.click()
    await composer.fill(text)
    await composer.press('Enter')
  }

  async function screenshot(panel, name) {
    const directory = path.join(SCRATCH, 'evidence')
    await fs.mkdir(directory, { recursive: true })
    const file = path.join(directory, `${name}.png`)
    await panel.page.screenshot({ path: file, fullPage: true })
    return file
  }

  return {
    home, get host() { return host }, mock, projects,
    openPanel, reopenPanel, restart, pan, wheel, point, sendPrompt, screenshot, close,
  }
}
