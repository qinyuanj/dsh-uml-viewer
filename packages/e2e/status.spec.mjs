import { test, expect } from '@playwright/test'
import { launchViewerLane } from './lane.mjs'
import { producerFrame, toScreen } from './canvas.mjs'

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

test('a request the companion completes is done; one its turn leaves unfinished fails, and a refreshed page shows the unseen failure', async ({ browser }) => {
  const lane = await launchViewerLane(browser, { reply: companion() })
  try {
    let a = await lane.openPanel('deep')
    const frame = await producerFrame(a, { root: lane.projects.deep, home: lane.home })
    await expect(statusBar(a)).not.toContainText('Refresh CRAP')

    await clickRefresh(a, frame, ':core')
    await expect(statusBar(a)).toContainText('Refresh CRAP :core: Failed — the companion ended this turn without completing the display')
    await lane.screenshot(a, 't52-turn-ended')

    const again = await producerFrame(a, { root: lane.projects.deep, home: lane.home })
    await clickRefresh(a, again, ':core')
    await expect(statusBar(a)).toContainText('Refresh CRAP :core: Done')

    a = await lane.reopenPanel(a)
    await expect(statusBar(a)).toContainText('Refresh CRAP :core: Failed — the companion ended this turn without completing the display')
    await expect(a.panel.getByRole('button', { name: 'Show the other 1 requests' })).toHaveText('1 more')
    expect(a.problems).toEqual([])
    await lane.screenshot(a, 't52-status-after-refresh')
  } finally { await lane.close() }
})
