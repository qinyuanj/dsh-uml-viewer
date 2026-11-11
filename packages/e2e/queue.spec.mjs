import { test, expect } from '@playwright/test'
import fs from 'node:fs/promises'
import path from 'node:path'
import { launchViewerLane } from './lane.mjs'
import { producerFrame, toScreen } from './canvas.mjs'

/**
 * #53: the request queue in the status bar, under #52's turn-end rule. Three
 * admitted requests show the current one plus 「还有 N 条」 with the whole queue
 * behind a click; a refused request stays as a failed entry until the user
 * dismisses it, and a dismissal is the host's list — a page refresh does not
 * bring the entry back, and no new message reaches the conversation.
 *
 * A request only stays unfinished while the companion has not finished its
 * turn (#52), so the scripted companion holds its answer to the first panel
 * request open: the turn that consumed it is still running while the test
 * queues the others, which is the busy-companion state story 37 describes.
 * The running/done transitions are #52's (see status.spec.mjs); the completed
 * fade has its own browser case here, with the ten-second path still covered
 * by the client's fake-timer statusbar tests. The rest proves the
 * browser-visible queue and dismissal.
 */

/**
 * The scripted companion: the first panel request's answer waits on `hold`,
 * every later one is acknowledged at once.
 * @param hold resolves the held answer.
 */
function acknowledging(hold) {
  let held = false
  return request => {
    if (!request.tools?.length) return null
    const last = request.messages?.at(-1)
    const text = (last?.content ?? []).find(part => part.type === 'text' && part.text?.startsWith('UML viewer panel request:'))?.text
    if (!text) return null
    const reply = { text: `Acknowledged ${/:op :([a-z-]+)/.exec(text)?.[1]}.` }
    if (held) return reply
    held = true
    return hold.then(() => reply)
  }
}

/**
 * F33's browser case: the first request runs IR and display, so it completes;
 * the second is held before any tool call, so its turn stays open and the bar
 * can show a queued request without a second display hiding the first one.
 */
function completingThenHolding(hold) {
  const seen = new Set()
  return request => {
    if (!request.tools?.length) return null
    const messages = request.messages ?? []
    let index = -1
    for (let i = 0; i < messages.length; i += 1) {
      for (const part of messages[i].content ?? []) {
        if (part.type === 'text' && part.text?.startsWith('UML viewer panel request:')) index = i
      }
    }
    if (index < 0) return null
    const prompt = (messages[index].content ?? []).filter(part => part.type === 'text').map(part => part.text ?? '').join('\n')
    seen.add(prompt)
    if (seen.size > 1) return hold.then(() => ({ text: 'Acknowledged the next request.' }))
    const tail = messages.slice(index)
    const used = name => tail.some(message => (message.content ?? [])
      .some(part => part.type === 'tool_use' && part.name === name))
    if (!used('uml_ir')) return { tool: { name: 'uml_ir', input: {} } }
    if (!used('uml_display')) return { tool: { name: 'uml_display', input: { path: 'diagram.edn' } } }
    return { text: 'Showed the regenerated diagram.' }
  }
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
  // The companion's answer to the first panel request waits until the test is
  // done with the queue, so that turn — and the requests behind it — stay open.
  let release = () => {}
  const hold = new Promise(resolve => { release = resolve })
  const lane = await launchViewerLane(browser, { reply: acknowledging(hold) })
  try {
    let a = await lane.openPanel('deep')
    const frame = await producerFrame(a, { root: lane.projects.deep, home: lane.home })
    // One request is the whole queue: nothing to open yet.
    await expect(more(a)).toBeHidden()

    await clickRefresh(a, frame, ':core')
    await expect(statusBar(a)).toContainText('Refresh CRAP :core: Queued')
    await expect(more(a)).toBeHidden()

    await clickRefresh(a, frame, ':core')
    await expect(more(a)).toHaveText('1 more')
    await clickRefresh(a, frame, ':api')
    // The current entry is the queue head; the others are behind it.
    await expect(statusBar(a)).toContainText('Refresh CRAP :core: Queued')
    await expect(more(a)).toHaveText('2 more')
    await expect(more(a)).toHaveAttribute('aria-label', 'Show the other 2 requests')
    await expect(more(a)).toHaveAttribute('aria-expanded', 'false')

    // The whole queue opens on the click, oldest first. The list is outside
    // the short aria-live region so screen readers do not re-read every row.
    await more(a).click()
    await expect(more(a)).toHaveAttribute('aria-expanded', 'true')
    await expect(more(a)).toHaveAttribute('aria-controls', /dsh-uml-viewer-request-list-/)
    await expect(queue(a).locator('xpath=ancestor::*[@aria-live="polite"]')).toHaveCount(0)
    await expect(items(a)).toHaveCount(3)
    await expect(items(a).nth(0)).toHaveText('Refresh CRAP :core: Queued')
    await expect(items(a).nth(1)).toHaveText('Refresh CRAP :core: Queued')
    await expect(items(a).nth(2)).toHaveText('Refresh CRAP :api: Queued')
    // A queued request is still coming: it is not the user's to throw away.
    await expect(queue(a).getByRole('button')).toHaveCount(0)
    await lane.screenshot(a, 't53-queue-open')
    // Esc, then an outside press, close the openable queue (F35).
    await a.page.keyboard.press('Escape')
    await expect(queue(a)).toHaveCount(0)
    await expect(more(a)).toHaveAttribute('aria-expanded', 'false')
    await more(a).click()
    await expect(queue(a)).toBeVisible()
    await a.canvas.click({ position: { x: 5, y: 5 } })
    await expect(queue(a)).toHaveCount(0)

    // The resident can no longer resolve :api's source file, so the request is
    // refused before it reaches the conversation: a failed entry, shown first.
    const messages = () => a.page.getByText('UML viewer panel request:', { exact: false })
    const admitted = await messages().count()
    await fs.rm(path.join(lane.projects.deep, 'src', 'api.ts'))
    await clickRefresh(a, frame, ':api')
    await expect(statusBar(a)).toContainText(/Refresh CRAP :api: Failed — .*api\.ts/)
    await expect(more(a)).toHaveText('3 more')
    await expect(messages()).toHaveCount(admitted)
    // The failure stays — past the completed fade's ten seconds — until the
    // user dismisses it.
    await a.page.waitForTimeout(10_500)
    await expect(statusBar(a)).toContainText(/Refresh CRAP :api: Failed/)

    // The list offers the dismissal on the failure alone.
    await more(a).click()
    await expect(items(a)).toHaveCount(4)
    const failed = items(a).nth(3)
    await expect(failed).toContainText(/Refresh CRAP :api: Failed — .*api\.ts/)
    const dismiss = failed.getByRole('button', { name: /^Dismiss Refresh CRAP :api: Failed/ })
    await expect(dismiss).toHaveCount(1)
    await expect(queue(a).getByRole('button')).toHaveCount(1)
    await lane.screenshot(a, 't53-queue-failed')
    await dismiss.click()
    // The host drops it: the list shrinks and the failure is gone everywhere.
    await expect(items(a)).toHaveCount(3)
    await expect(more(a)).toHaveText('2 more')
    await expect(statusBar(a)).not.toContainText('Failed')
    await expect(statusBar(a)).toContainText('Refresh CRAP :core: Queued')
    expect(await messages().count()).toBe(admitted)
    await lane.screenshot(a, 't53-queue-dismissed')

    // A page refresh: the host no longer holds the dismissed entry, so it does
    // not come back.
    a = await lane.reopenPanel(a)
    await expect(statusBar(a)).toContainText('Refresh CRAP :core: Queued')
    await expect(statusBar(a)).not.toContainText('Failed')
    await expect(more(a)).toHaveText('2 more')
    await more(a).click()
    await expect(items(a)).toHaveCount(3)
    await expect(items(a).filter({ hasText: 'Failed' })).toHaveCount(0)
    expect(a.problems).toEqual([])
    await lane.screenshot(a, 't53-queue-after-refresh')
  } finally { release(); await lane.close() }
})

test('a completed request fades as soon as the next request starts (F33)', async ({ browser }) => {
  let release = () => {}
  const hold = new Promise(resolve => { release = resolve })
  const lane = await launchViewerLane(browser, { reply: completingThenHolding(hold) })
  try {
    const a = await lane.openPanel('deep')
    const frame = await producerFrame(a, { root: lane.projects.deep, home: lane.home })
    await clickRefresh(a, frame, ':core')
    // A real uml_display success makes the first request Done.
    await expect(statusBar(a)).toContainText('Refresh CRAP :core: Done')
    await lane.screenshot(a, 't53-queue-done')

    // The next request starts while the companion holds its answer open: the
    // completed entry fades immediately, leaving the queued request visible.
    const again = await producerFrame(a, { root: lane.projects.deep, home: lane.home })
    await clickRefresh(a, again, ':api')
    await expect(statusBar(a)).toContainText('Refresh CRAP :api: Queued')
    await expect(statusBar(a)).not.toContainText('Done')
    await expect(more(a)).toBeHidden()
    await lane.screenshot(a, 't53-queue-fade-on-next-request')
    expect(a.problems).toEqual([])
  } finally { release(); await lane.close() }
})
