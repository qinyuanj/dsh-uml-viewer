import fs from 'node:fs/promises'
import path from 'node:path'
import { expect } from '@playwright/test'
import { SCRATCH, bootDshWeb, dismissOverlays, packageInputs, startDshWeb } from './dsh-web.mjs'
import { startMockLlm } from './mock-llm.mjs'
import { makeProjects } from './projects.mjs'
import { isolateScratch } from './scratch-scope.mjs'
import { chooseWorkspace } from './workspace.mjs'
import { bindRefresh, recordActions } from './lifecycle.mjs'

/**
 * Reusable #22–#29 browser lane: one scratch host/mock model, one browser
 * context per DSH session, and only UI-level panel reads and gestures.
 * `reopen` reloads the Web client; `restart` stops and boots the DSH host on
 * the same scratch home, so saved session state is exercised for real.
 */
export async function launchViewerLane(browser, options = {}) {
  const packages = packageInputs(options)
  // CI14: run homes must not resolve into the checkout around the scratch.
  isolateScratch(SCRATCH)
  const home = await fs.mkdtemp(path.join(SCRATCH, 'run-'))
  const projects = await makeProjects(home)
  await options.prepareProjects?.(projects)
  const mock = await startMockLlm({ log: path.join(home, 'mock-requests'), reply: options.reply })
  let installed
  let host
  try {
    installed = await startDshWeb({ home, projects, packages })
    host = await bootDshWeb({ ...installed, home, mockUrl: mock.url })
  } catch (error) {
    await mock.stop()
    throw error
  }
  // Boot output includes the scratch host's token URL. Retain routing facts
  // without persisting URL query/fragment credentials in textual diagnostics.
  const redact = text => text.replace(/(https?:\/\/[^\s?#]+)[?#][^\s]*/g, '$1[redacted]')
  const pages = []
  const records = new Map()
  const diagnostics = path.join(SCRATCH, 'diagnostics', path.basename(home))
  await fs.mkdir(diagnostics, { recursive: true })

  // Observe the real DOM and transport at the disputed boundaries; never use
  // these snapshots as an assertion or as a substitute for panel readiness.
  async function capturePage(page, phase) {
    const record = records.get(page)
    try {
      const state = await page.evaluate(() => {
        const box = element => {
          const r = element.getBoundingClientRect()
          return { x: r.x, y: r.y, width: r.width, height: r.height }
        }
        return {
          body: document.body.innerText,
          sessions: [...document.querySelectorAll('[data-conversation-session]')].map(el => ({ id: el.getAttribute('data-conversation-session'), box: box(el) })),
          panels: [...document.querySelectorAll('.dsh-uml-viewer-panel')].map(el => ({
            session: el.getAttribute('data-session-id'), box: box(el),
            summary: el.querySelector('.dsh-uml-viewer-summary')?.outerHTML,
            canvases: [...el.querySelectorAll('canvas')].map(canvas => ({ box: box(canvas), width: canvas.width, height: canvas.height })),
          })),
          pointer: window.__umlE2ePointerEvents ?? [],
        }
      })
      state.body = redact(state.body)
      await fs.writeFile(path.join(diagnostics, `page-${record.number}-${phase}.json`), JSON.stringify({
        phase, project: record.project, stage: record.stage, mockRequests: mock.requests.length,
        transportProblems: record.transportProblems.map(redact),
        problems: record.problems.map(redact), ...state,
      }, null, 2))
      await page.screenshot({ path: path.join(diagnostics, `page-${record.number}-${phase}.png`), fullPage: true, timeout: 5000 })
      console.log(`[panel-e2e] ${path.basename(home)} page ${record.number} ${record.project ?? ''} ${phase}: ${mock.requests.length} mock requests; ${record.problems.length} browser errors`)
    } catch (error) {
      console.warn(`[panel-e2e] diagnostic snapshot ${phase}: ${error.message}`)
    }
  }
  let closing
  const close = () => closing ??= (async () => {
    for (const page of pages.splice(0).reverse()) {
      if (!page.isClosed()) await capturePage(page, 'close')
      await page.context().close().catch(() => {})
    }
    await fs.writeFile(path.join(diagnostics, 'host.log'), redact(host.output()))
      .catch(error => console.warn(`[panel-e2e] host log: ${error.message}`))
    host.stop()
    await host.exited.catch(() => {})
    await mock.stop()
  })()
  const uninstall = async () => {
    await close()
    installed.uninstall()
  }

  async function newPage() {
    // `deviceScaleFactor` emulates a high-DPI display (the deleted dpi drivers, #30).
    const scale = options.deviceScaleFactor ? { deviceScaleFactor: options.deviceScaleFactor } : {}
    const context = await browser.newContext({ viewport: { width: 1440, height: 1000 }, ...scale })
    const page = await context.newPage()
    pages.push(page)
    const problems = []
    // Attached before the page loads, so the Remote carrier is seen from its start.
    records.set(page, { number: records.size + 1, problems, transportProblems: [], sent: recordActions(page) })
    await bindRefresh(page)
    page.on('pageerror', error => problems.push(error.message))
    page.on('console', message => { if (message.type() === 'error') records.get(page).transportProblems.push(`console: ${message.text()}`) })
    page.on('requestfailed', request => records.get(page).transportProblems.push(`request: ${request.method()} ${request.url().split('?')[0]} ${request.failure()?.errorText}`))
    await page.addInitScript(() => {
      window.__umlE2ePointerEvents = []
      for (const type of ['pointerdown', 'pointerup']) document.addEventListener(type, event => {
        const target = event.target
        window.__umlE2ePointerEvents.push({ type, x: event.clientX, y: event.clientY,
          target: target instanceof Element ? target.tagName + '.' + target.className : String(target) })
        if (window.__umlE2ePointerEvents.length > 12) window.__umlE2ePointerEvents.shift()
      }, true)
    })
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
    return { page, ...opened, sessionId, before: await opened.summary.innerText(), problems, sent: records.get(page)?.sent ?? [] }
  }

  async function openPanel(project, options = {}) {
    const { page, problems } = await newPage()
    records.get(page).project = project
    try {
      await page.goto(host.url, { waitUntil: 'domcontentloaded', timeout: 60_000 })
      await dismissOverlays(page)
      {
        const newSession = page.getByRole('button', { name: 'New session', exact: true }).first()
        // A late Preview Notice can arrive after the initial quiet period.
        await newSession.click({ timeout: 2000 }).catch(async () => {
          await dismissOverlays(page)
          await newSession.click({ timeout: 30_000 })
        })
        await chooseWorkspace(page, project)
      }
      const composer = page.locator('[data-composer-input][contenteditable="true"]:visible').last()
      await composer.click({ timeout: 30_000 })
      records.get(page).stage = 'submit-open-prompt'
      await composer.fill(`Open the ${project} sample project`)
      await composer.press('Enter')
      const sessionNode = page.locator('[data-conversation-session]:visible').first()
      await expect(sessionNode).toBeVisible({ timeout: 30_000 })
      records.get(page).stage = 'wait-generated-response'
      await expect(page.getByText('Generated the sample project IR.', { exact: true }).last()).toBeVisible()
      if (project === 'ledger' || options.state) {
        const sessionId = await sessionNode.getAttribute('data-conversation-session')
        if (!sessionId) throw new Error('the DSH conversation exposes no session id')
        const stateDirectory = path.join(home, 'plugin-state', 'sessions')
        await fs.mkdir(stateDirectory, { recursive: true })
        await fs.writeFile(path.join(stateDirectory, `${sessionId}.json`), JSON.stringify({
          schema: 'dsh-uml-viewer/session-state',
          version: 1,
          diagram: path.join(projects[project], 'diagram.edn'),
          context: project === 'ledger' ? {
            kind: 'proposal',
            proposalId: ':p-ledger',
            name: 'Ledger proposal',
            edn: '{:context :proposal :name "Ledger proposal" :proposal-id :p-ledger}',
          } : { kind: 'real', edn: '{:context :real}' },
          focus: [],
          proposalId: project === 'ledger' ? ':p-ledger' : null,
          openLayer: null,
          declutter: null,
          camera: { x: 0, y: 0, zoom: 1 },
          selected: null,
          detail: null,
          ...options.state,
        }))
      }
      records.get(page).stage = 'open-sidebar'
      const opened = await openSidebar(page)
      // A panel opened before any display shows the waiting canvas, which is
      // how the status bar's canvas-status line is observed (#46).
      if (options.display === false) {
        await expect(opened.summary).toHaveAttribute('data-status', 'waiting-display', { timeout: 30_000 })
        records.get(page).stage = 'panel-waiting'
        return await result(page, problems, opened)
      }
      await composer.click()
      records.get(page).stage = 'submit-display-prompt'
      await composer.fill('Render the sample diagram')
      await composer.press('Enter')
      await expect(page.getByText('The isolated mock model displayed the generated IR.', { exact: false }).last())
        .toBeVisible({ timeout: 45_000 })
      await expect(opened.summary).toContainText(`${project[0].toUpperCase()}${project.slice(1)}`, { timeout: 45_000 })
      if (project === 'ledger') await expect(opened.summary).toHaveAttribute('data-view', 'proposal')
      records.get(page).stage = 'panel-opened'
      return await result(page, problems, opened)
    } catch (error) {
      await capturePage(page, 'open-failed')
      await fs.writeFile(path.join(home, `${project}-failure.txt`), `${error.stack}\\n\\n${await page.locator('body').innerText().catch(() => '')}\\n\\n${redact(host.output())}`)
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

  /** Open the UML tab again in the same page, e.g. after the user closed it. */
  async function reopenTab(panel) {
    const opened = await openSidebar(panel.page)
    return result(panel.page, panel.problems, opened)
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
    home, get host() { return host }, mock, projects, packages,
    openPanel, reopenPanel, reopenTab, restart, pan, wheel, point, sendPrompt, screenshot, close, uninstall,
    installPlatform: file => installed.installPlatform(file),
    // The isolated host's own stdout/stderr, redacted and bounded. Activation
    // warnings and errors reach it and never the page, the session or the DOM.
    hostOutput: (limit = 8000) => redact(host.output()).slice(-limit),
    // A replacement may re-register the sidebar contribution. Let an automatic
    // remount win; otherwise select the real UML tab, never a synthetic mount.
    // A panel that stays closed after either is a failed acceptance, not retried.
    reattachPanel: async panel => {
      const framed = () => expect(panel.summary).toHaveAttribute('data-kind', 'frame', { timeout: 10_000 })
      try { await framed() } catch {
        const expand = panel.page.locator('[data-sidebar-right-expand]').first()
        if (await expand.isVisible().catch(() => false)) await expand.click({ timeout: 30_000 })
        const uml = panel.page.getByText('UML', { exact: true }).last()
        if (await uml.count()) await uml.click().catch(() => {})
        await framed()
      }
      return result(panel.page, panel.problems, panel)
    },
  }
}
