import fs from 'node:fs/promises'
import path from 'node:path'
import { expect } from '@playwright/test'
import { SCRATCH, bootDshWeb, dismissOverlays, packageInputs, startDshWeb } from './dsh-web.mjs'
import { startMockLlm } from './mock-llm.mjs'
import { makeProjects } from './projects.mjs'

/**
 * Reusable #22–#29 browser lane: one scratch host/mock model, one browser
 * context per DSH session, and only UI-level panel reads and gestures.
 */
export async function launchViewerLane(browser, options = {}) {
  const packages = packageInputs(options)
  await fs.mkdir(SCRATCH, { recursive: true })
  const home = await fs.mkdtemp(path.join(SCRATCH, 'run-'))
  const projects = await makeProjects(home)
  await options.prepareProjects?.(projects)
  const mock = await startMockLlm({ log: path.join(home, 'mock-requests') })
  let installed
  let host
  try {
    installed = await startDshWeb({ home, projects, packages })
    host = await bootDshWeb({ ...installed, home, mockUrl: mock.url })
  } catch (error) {
    await mock.stop()
    throw error
  }
  const pages = []
  let closing
  const close = () => closing ??= (async () => {
    for (const page of pages.splice(0).reverse()) await page.context().close().catch(() => {})
    host.stop()
    await host.exited.catch(() => {})
    await mock.stop()
  })()
  const uninstall = async () => {
    await close()
    installed.uninstall()
  }

  async function openPanel(project) {
    const context = await browser.newContext({ viewport: { width: 1440, height: 1000 } })
    const page = await context.newPage()
    pages.push(page)
    const problems = []
    page.on('pageerror', error => problems.push(error.message))
    try {
      await page.goto(host.url, { waitUntil: 'domcontentloaded', timeout: 60_000 })
      await dismissOverlays(page)
      if (project !== 'shop') {
        await page.getByRole('button', { name: 'New session', exact: true }).first().click()
        await page.getByRole('button', { name: 'Choose workspace' }).click()
        await page.locator('[role="menu"], [role="dialog"]').getByText(project, { exact: true }).last().click()
        await expect(page.getByRole('button', { name: 'Choose workspace' })).toContainText(project)
      }
      const composer = page.locator('[data-composer-input]').first()
      await composer.click({ timeout: 30_000 })
      await composer.fill(`Open the ${project} sample project`)
      await composer.press('Enter')
      const sessionNode = page.locator('[data-conversation-session]').first()
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
      const expand = page.locator('[data-sidebar-right-expand]').first()
      await expand.click({ timeout: 30_000 })
      const uml = page.getByText('UML', { exact: true }).last()
      if (await uml.count()) await uml.click().catch(() => {})
      const panel = page.locator('.dsh-uml-viewer-panel').last()
      await expect(panel).toBeVisible({ timeout: 30_000 })
      const canvas = panel.locator('canvas')
      const summary = panel.locator('.dsh-uml-viewer-summary')
      await expect(canvas).toBeVisible()
      await expect(summary).toHaveAttribute('data-kind', 'frame', { timeout: 30_000 })
      await composer.click()
      await composer.fill('Render the sample diagram')
      await composer.press('Enter')
      await expect(page.getByText('The isolated mock model displayed the generated IR.', { exact: false }).last())
        .toBeVisible({ timeout: 45_000 })
      await expect(summary).toContainText(`${project[0].toUpperCase()}${project.slice(1)}`, { timeout: 45_000 })
      if (project === 'ledger') await expect(summary).toHaveAttribute('data-view', 'proposal')
      expect(problems).toEqual([])
      return { page, panel, canvas, summary, before: await summary.innerText(), problems }
    } catch (error) {
      await fs.writeFile(path.join(home, `${project}-failure.txt`), `${error.stack}\n\n${await page.locator('body').innerText().catch(() => '')}\n\n${host.output()}`)
      await page.screenshot({ path: path.join(home, `${project}-failure.png`), fullPage: true }).catch(() => {})
      throw error
    }
  }

  async function pan(panel) {
    const box = await panel.canvas.boundingBox()
    if (!box) throw new Error('panel canvas has no box')
    await panel.page.mouse.move(box.x + box.width / 2, box.y + box.height / 2)
    await panel.page.mouse.down()
    await panel.page.mouse.move(box.x + box.width / 2 + 80, box.y + box.height / 2 + 30, { steps: 6 })
    await panel.page.mouse.up()
  }

  /** Real wheel notches over the canvas centre; positive `notches` zoom out. */
  async function wheel(panel, notches) {
    const box = await panel.canvas.boundingBox()
    if (!box) throw new Error('panel canvas has no box')
    await panel.page.mouse.move(box.x + box.width / 2, box.y + box.height / 2)
    for (let i = 0; i < Math.abs(notches); i += 1) {
      await panel.page.mouse.wheel(0, Math.sign(notches) * 100)
      await panel.page.waitForTimeout(30)
    }
  }

  async function sendPrompt(panel, text) {
    const composer = panel.page.locator('[data-composer-input]').first()
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

  return { home, host, mock, projects, packages, openPanel, pan, wheel, sendPrompt, screenshot, close, uninstall }
}
