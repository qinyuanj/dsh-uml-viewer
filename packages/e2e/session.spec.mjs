import { test, expect } from '@playwright/test'
import { launchViewerLane } from './lane.mjs'
import { producerFrame, toScreen } from './canvas.mjs'

/**
 * The panel equivalent of scripts/drivers/session.edn + session_restore.edn:
 * the host persists the resident's view state, so both a Web reopen and a real
 * DSH host restart restore focus, camera and selection without another
 * uml_ir/uml_display. Discussion context is covered by the proposal session.
 */
test('restores focus, selection and camera after reopen and after a DSH restart', async ({ browser }) => {
  const lane = await launchViewerLane(browser)
  try {
    let deep = await lane.openPanel('deep')
    const root = await producerFrame(deep, { root: lane.projects.deep, home: lane.home })
    const core = root.world.find(item => item.id === ':core' && (item.kind === 'class' || item.kind === 'package'))
    await lane.point(deep, toScreen(root, core.rect.x + core.rect.w / 2, core.rect.y + core.rect.h / 2), 2)
    await expect(deep.summary).toHaveAttribute('data-depth', 'drilled')
    const drilled = await producerFrame(deep, { root: lane.projects.deep, home: lane.home, focus: ':core' })
    const order = drilled.world.find(item => item.kind === 'class' && item.id === ':core.order')
    await lane.point(deep, toScreen(drilled, order.rect.x + order.rect.w / 2, order.rect.y + order.rect.h / 2))
    await expect(deep.summary).toHaveAttribute('data-selected', ':core.order')
    await lane.wheel(deep, -2)
    await lane.pan(deep)
    await expect.poll(async () => deep.summary.innerText()).not.toBe(deep.before)
    const before = await deep.summary.innerText()

    deep = await lane.reopenPanel(deep)
    await expect(deep.summary).toHaveText(before, { timeout: 30_000 })
    await expect(deep.summary).toHaveAttribute('data-depth', 'drilled')
    await expect(deep.summary).toHaveAttribute('data-selected', ':core.order')

    deep = await lane.restart(deep)
    // No prompt follows the restart: the persisted view itself is displayed.
    await expect(deep.summary).toHaveText(before, { timeout: 30_000 })
    await expect(deep.summary).toHaveAttribute('data-depth', 'drilled')
    await expect(deep.summary).toHaveAttribute('data-selected', ':core.order')
    expect(deep.problems).toEqual([])
  } finally {
    await lane.close()
  }
})

test('restores the proposal discussion context after a DSH restart', async ({ browser }) => {
  const lane = await launchViewerLane(browser)
  try {
    let ledger = await lane.openPanel('ledger')
    await expect(ledger.summary).toHaveAttribute('data-context', 'proposal')
    await expect(ledger.summary).toHaveAttribute('data-view', 'proposal')
    const before = await ledger.summary.innerText()
    ledger = await lane.restart(ledger)
    await expect(ledger.summary).toHaveText(before, { timeout: 30_000 })
    await expect(ledger.summary).toContainText('Discussion: Ledger proposal')
    await expect(ledger.summary).toHaveAttribute('data-context', 'proposal')
    await expect(ledger.summary).toHaveAttribute('data-view', 'proposal')
    expect(ledger.problems).toEqual([])
  } finally {
    await lane.close()
  }
})
