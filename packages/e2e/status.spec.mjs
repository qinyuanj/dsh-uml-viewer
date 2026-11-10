import { test, expect } from '@playwright/test'
import { launchViewerLane } from './lane.mjs'
import { producerFrame, toScreen } from './canvas.mjs'

/**
 * #52 request status B in the browser: the companion's `uml_*` calls take an
 * admitted request from queued to done, and a turn that ends while the request
 * is unfinished fails it with the reason the host gives. The transition table
 * itself is covered at the host seam (tests/request-status.seam.test.ts); the
 * spec's browser evidence is one success and one failure (#41, Testing
 * Decisions), so the validResult-false failure lives in requests.spec.mjs.
 */

/** No tool call for the first panel request; IR and display for the second. */
function companion() {
  let unfinished = true
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
    const tail = messages.slice(index)
    const used = name => tail.some(message => (message.content ?? []).some(part => part.type === 'tool_use' && part.name === name))
    if (unfinished) {
      unfinished = false
      // The turn ends right here: no IR, no display.
      return { text: 'Could not start on this request.' }
    }
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

test('a request the companion completes is done; one its turn leaves unfinished fails, and both survive a page refresh', async ({ browser }) => {
  const lane = await launchViewerLane(browser, { reply: companion() })
  try {
    let a = await lane.openPanel('deep')
    const frame = await producerFrame(a, { root: lane.projects.deep, home: lane.home })
    // Nothing requested yet: the bar says nothing about requests.
    await expect(statusBar(a)).not.toContainText('Refresh CRAP')

    // The companion ends its turn without an IR or a display: the request the
    // turn consumed did not finish, so it fails with that reason.
    await clickRefresh(a, frame, ':core')
    await expect(statusBar(a)).toHaveAttribute('data-request', 'failed')
    await expect(statusBar(a)).toContainText('Refresh CRAP :core: Failed — the companion ended this turn without completing the display')
    await lane.screenshot(a, 't52-turn-ended')
    // The host holds the list in memory: a refreshed page shows the failure again.
    a = await lane.reopenPanel(a)
    await expect(statusBar(a)).toHaveAttribute('data-request', 'failed')

    // The next request goes all the way: IR, then the display that completes it.
    const again = await producerFrame(a, { root: lane.projects.deep, home: lane.home })
    await clickRefresh(a, again, ':core')
    await expect(statusBar(a)).toContainText('Refresh CRAP :core: Done')
    await expect(statusBar(a)).toHaveAttribute('data-request', 'done')
    expect(a.problems).toEqual([])
    await lane.screenshot(a, 't52-status-done')
  } finally { await lane.close() }
})
