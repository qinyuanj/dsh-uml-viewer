import { test, expect } from '@playwright/test'
import fs from 'node:fs/promises'
import path from 'node:path'
import { launchViewerLane } from './lane.mjs'
import { producerFrame, toScreen } from './canvas.mjs'

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
  let release = () => {}
  const hold = new Promise(resolve => { release = resolve })
  const lane = await launchViewerLane(browser, { reply: acknowledging(hold) })
  try {
    let a = await lane.openPanel('deep')
    const frame = await producerFrame(a, { root: lane.projects.deep, home: lane.home })
    await expect(more(a)).toBeHidden()

    await clickRefresh(a, frame, ':core')
    await expect(statusBar(a)).toContainText('Refresh CRAP :core: Queued')
    await expect(more(a)).toBeHidden()

    await clickRefresh(a, frame, ':core')
    await expect(more(a)).toHaveText('1 more')
    await clickRefresh(a, frame, ':api')
    await expect(statusBar(a)).toContainText('Refresh CRAP :core: Queued')
    await expect(more(a)).toHaveText('2 more')
    await expect(more(a)).toHaveAttribute('aria-label', 'Show the other 2 requests')
    await expect(more(a)).toHaveAttribute('aria-expanded', 'false')

    await more(a).click()
    await expect(more(a)).toHaveAttribute('aria-expanded', 'true')
    await expect(more(a)).toHaveAttribute('aria-controls', /dsh-uml-viewer-request-list-/)
    await expect(queue(a).locator('xpath=ancestor::*[@aria-live="polite"]')).toHaveCount(0)
    await expect(items(a)).toHaveCount(3)
    await expect(items(a).nth(0)).toHaveText('Refresh CRAP :core: Queued')
    await expect(items(a).nth(1)).toHaveText('Refresh CRAP :core: Queued')
    await expect(items(a).nth(2)).toHaveText('Refresh CRAP :api: Queued')
    await expect(queue(a).getByRole('button')).toHaveCount(0)
    await lane.screenshot(a, 't53-queue-open')
    await a.page.keyboard.press('Escape')
    await expect(queue(a)).toHaveCount(0)
    await expect(more(a)).toHaveAttribute('aria-expanded', 'false')
    await more(a).click()
    await expect(queue(a)).toBeVisible()
    await a.canvas.click({ position: { x: 5, y: 5 } })
    await expect(queue(a)).toHaveCount(0)

    const messages = () => a.page.getByText('UML viewer panel request:', { exact: false })
    await expect(messages()).toHaveCount(1)
    const admitted = await messages().count()
    await fs.rm(path.join(lane.projects.deep, 'src', 'api.ts'))
    await clickRefresh(a, frame, ':api')
    await expect(statusBar(a)).toContainText(/Refresh CRAP :api: Failed — .*api\.ts/)
    await expect(more(a)).toHaveText('3 more')
    await expect(messages()).toHaveCount(admitted)
    await a.page.waitForTimeout(10_500)
    await expect(statusBar(a)).toContainText(/Refresh CRAP :api: Failed/)

    await more(a).click()
    await expect(items(a)).toHaveCount(4)
    const failed = items(a).nth(3)
    await expect(failed).toContainText(/Refresh CRAP :api: Failed — .*api\.ts/)
    const dismiss = failed.getByRole('button', { name: /^Dismiss Refresh CRAP :api: Failed/ })
    await expect(dismiss).toHaveCount(1)
    await expect(queue(a).getByRole('button')).toHaveCount(1)
    await lane.screenshot(a, 't53-queue-failed')
    await dismiss.click()
    await expect(items(a)).toHaveCount(3)
    await expect(more(a)).toHaveText('2 more')
    await expect(statusBar(a)).not.toContainText('Failed')
    await expect(statusBar(a)).toContainText('Refresh CRAP :core: Queued')
    await expect(messages()).toHaveCount(admitted)
    await lane.screenshot(a, 't53-queue-dismissed')

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
    await expect(statusBar(a)).toContainText('Refresh CRAP :core: Done')
    await lane.screenshot(a, 't53-queue-done')

    const again = await producerFrame(a, { root: lane.projects.deep, home: lane.home })
    await clickRefresh(a, again, ':api')
    await expect(statusBar(a)).toContainText('Refresh CRAP :api: Queued')
    await expect(statusBar(a)).not.toContainText('Done')
    await expect(more(a)).toBeHidden()
    await lane.screenshot(a, 't53-queue-fade-on-next-request')
    expect(a.problems).toEqual([])
  } finally { release(); await lane.close() }
})
