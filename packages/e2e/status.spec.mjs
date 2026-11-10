import { test, expect } from '@playwright/test'
import fs from 'node:fs/promises'
import path from 'node:path'
import { launchViewerLane } from './lane.mjs'
import { producerFrame, toScreen } from './canvas.mjs'

/**
 * #51 request status A: a panel request is visible on the panel's status bar
 * as soon as it is admitted, a refused one shows why, and a page refresh keeps
 * the unfinished entries (the host holds them in memory per session).
 *
 * The scripted companion only acknowledges a panel request and runs no tool,
 * so an admitted request stays queued: running, done and failed-after-run are
 * #52's transitions.
 */
function acknowledging(request) {
  if (!request.tools?.length) return null
  const last = request.messages?.at(-1)
  const text = (last?.content ?? []).find(part => part.type === 'text' && part.text?.startsWith('UML viewer panel request:'))?.text
  if (!text) return null
  return { text: `Acknowledged ${/:op :([a-z-]+)/.exec(text)?.[1]}.` }
}

async function clickRefresh(panel, frame, id) {
  const item = frame.world.find(item => item.id === id && (item.kind === 'class' || item.kind === 'package'))
  expect(item, `frame contains ${id}`).toBeTruthy()
  const [x, y] = toScreen(frame, item.rect.x + item.rect.w / 2, item.rect.y + 10)
  await panel.canvas.click({ position: { x, y }, button: 'right' })
  await panel.panel.getByRole('menu', { name: 'Element requests' }).getByRole('menuitem', { name: 'Refresh CRAP', exact: true }).click()
}

const statusBar = panel => panel.panel.locator('.dsh-uml-viewer-statusbar')

test('a refresh is queued at once, a refused one says why, and a page refresh keeps the queued one', async ({ browser }) => {
  const lane = await launchViewerLane(browser, { reply: acknowledging })
  try {
    let a = await lane.openPanel('deep')
    const frame = await producerFrame(a, { root: lane.projects.deep, home: lane.home })
    // Nothing requested yet: the bar says nothing about requests.
    await expect(statusBar(a)).not.toContainText('Refresh CRAP')

    await clickRefresh(a, frame, ':core')
    await expect(statusBar(a)).toContainText('Refresh CRAP :core: Queued')
    await expect(statusBar(a)).toHaveAttribute('data-request', 'queued')
    await expect(a.page.getByText('Acknowledged refresh-crap.', { exact: true }).last()).toBeVisible()
    const messages = () => a.page.getByText('UML viewer panel request:', { exact: false })
    const admitted = await messages().count()

    // The resident can no longer resolve :api's source file: the request is
    // refused before it reaches the conversation, and the bar says why.
    await fs.rm(path.join(lane.projects.deep, 'src', 'api.ts'))
    await clickRefresh(a, frame, ':api')
    await expect(statusBar(a)).toHaveAttribute('data-request', 'failed')
    await expect(statusBar(a)).toContainText(/Refresh CRAP: Failed — .*api\.ts/)
    await expect(messages()).toHaveCount(admitted)
    await lane.screenshot(a, 't51-status-failed')

    // A page refresh: the host still holds this session's list, so both
    // unfinished entries come back. The bar's current entry is the unseen
    // failure (#53's rule), and the queued one still waits behind it.
    a = await lane.reopenPanel(a)
    await expect(statusBar(a)).toContainText(/Refresh CRAP: Failed — .*api\.ts/)
    await expect(statusBar(a)).toHaveAttribute('data-request', 'failed')
    await expect(statusBar(a)).toHaveAttribute('data-request-count', '2')
    await expect(a.panel.getByRole('button', { name: 'Show the other 1 requests' })).toHaveText('1 more')
    expect(a.problems).toEqual([])
    await lane.screenshot(a, 't51-status-after-refresh')
  } finally { await lane.close() }
})
