import { test, expect } from '@playwright/test'
import fs from 'node:fs/promises'
import path from 'node:path'
import { launchViewerLane } from './lane.mjs'
import { producerFrame, toScreen } from './canvas.mjs'

/**
 * #53: the request queue in the status bar. Three admitted requests show the
 * current one plus 「还有 N 条」 with the whole queue behind a click; a refused
 * request stays as a failed entry until the user dismisses it, and a dismissal
 * is the host's list — a page refresh does not bring the entry back, and no new
 * message reaches the conversation.
 *
 * The scripted companion only acknowledges a panel request and runs no tool, so
 * every admitted request stays queued: running, done and the completed fade are
 * #52's transitions (the fade itself is covered in the client's statusbar
 * tests). What this spec proves is the browser-visible queue and dismissal.
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
const more = panel => statusBar(panel).locator('.dsh-uml-viewer-request-more')
const queue = panel => statusBar(panel).getByRole('list', { name: 'Request queue' })
const items = panel => queue(panel).getByRole('listitem')

test('the queue shows how many wait behind the current request, and a dismissed failure stays gone', async ({ browser }) => {
  const lane = await launchViewerLane(browser, { reply: acknowledging })
  try {
    let a = await lane.openPanel('deep')
    const frame = await producerFrame(a, { root: lane.projects.deep, home: lane.home })
    // One request is the whole queue: nothing to open yet.
    await expect(more(a)).toBeHidden()

    await clickRefresh(a, frame, ':core')
    await expect(statusBar(a)).toContainText('Refresh CRAP :core: Queued')
    await expect(statusBar(a)).toHaveAttribute('data-request-count', '1')
    await expect(more(a)).toBeHidden()

    await clickRefresh(a, frame, ':core')
    await expect(statusBar(a)).toHaveAttribute('data-request-count', '2')
    await expect(more(a)).toHaveText('1 more')
    await clickRefresh(a, frame, ':api')
    await expect(statusBar(a)).toHaveAttribute('data-request-count', '3')
    // The current entry is the queue head; the others are behind it.
    await expect(statusBar(a)).toContainText('Refresh CRAP :core: Queued')
    await expect(statusBar(a)).toHaveAttribute('data-request', 'queued')
    await expect(more(a)).toHaveText('2 more')
    await expect(more(a)).toHaveAttribute('aria-label', 'Show the other 2 requests')
    await expect(more(a)).toHaveAttribute('aria-expanded', 'false')

    // The whole queue opens on the click, oldest first.
    await more(a).click()
    await expect(more(a)).toHaveAttribute('aria-expanded', 'true')
    await expect(items(a)).toHaveCount(3)
    await expect(items(a).nth(0)).toHaveText('Refresh CRAP :core: Queued')
    await expect(items(a).nth(1)).toHaveText('Refresh CRAP :core: Queued')
    await expect(items(a).nth(2)).toHaveText('Refresh CRAP :api: Queued')
    // A queued request is still coming: it is not the user's to throw away.
    await expect(queue(a).getByRole('button')).toHaveCount(0)
    await lane.screenshot(a, 't53-queue-open')
    await more(a).click()
    await expect(queue(a)).toHaveCount(0)

    // The resident can no longer resolve :api's source file, so the request is
    // refused before it reaches the conversation: a failed entry, shown first.
    const messages = () => a.page.getByText('UML viewer panel request:', { exact: false })
    const admitted = await messages().count()
    await fs.rm(path.join(lane.projects.deep, 'src', 'api.ts'))
    await clickRefresh(a, frame, ':api')
    await expect(statusBar(a)).toHaveAttribute('data-request', 'failed')
    await expect(statusBar(a)).toHaveAttribute('data-request-count', '4')
    await expect(statusBar(a)).toContainText(/Refresh CRAP: Failed — .*api\.ts/)
    await expect(messages()).toHaveCount(admitted)
    // The failure stays — past the completed fade's ten seconds — until the
    // user dismisses it.
    await a.page.waitForTimeout(10_500)
    await expect(statusBar(a)).toHaveAttribute('data-request', 'failed')

    // The list offers the dismissal on the failure alone.
    await more(a).click()
    await expect(items(a)).toHaveCount(4)
    const failed = items(a).nth(3)
    await expect(failed).toHaveAttribute('data-state', 'failed')
    await expect(failed).toContainText(/Refresh CRAP: Failed — .*api\.ts/)
    const dismiss = failed.getByRole('button', { name: /^Dismiss Refresh CRAP: Failed/ })
    await expect(dismiss).toHaveCount(1)
    await expect(queue(a).getByRole('button')).toHaveCount(1)
    await lane.screenshot(a, 't53-queue-failed')
    await dismiss.click()
    // The host drops it: the list shrinks and the failure is gone everywhere.
    await expect(items(a)).toHaveCount(3)
    await expect(statusBar(a)).toHaveAttribute('data-request-count', '3')
    await expect(statusBar(a)).not.toContainText('Failed')
    await expect(statusBar(a)).toContainText('Refresh CRAP :core: Queued')
    expect(await messages().count()).toBe(admitted)
    await lane.screenshot(a, 't53-queue-dismissed')

    // A page refresh: the host no longer holds the dismissed entry, so it does
    // not come back.
    a = await lane.reopenPanel(a)
    await expect(statusBar(a)).toHaveAttribute('data-request-count', '3')
    await expect(statusBar(a)).toContainText('Refresh CRAP :core: Queued')
    await expect(statusBar(a)).not.toContainText('Failed')
    await expect(more(a)).toHaveText('2 more')
    await more(a).click()
    await expect(items(a)).toHaveCount(3)
    await expect(items(a).filter({ hasText: 'Failed' })).toHaveCount(0)
    expect(a.problems).toEqual([])
    await lane.screenshot(a, 't53-queue-after-refresh')
  } finally { await lane.close() }
})
