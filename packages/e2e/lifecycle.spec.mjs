import { test, expect } from '@playwright/test'
import { launchViewerLane } from './lane.mjs'
import { closeTab, collapseSidebar, expandSidebar, refreshTab, residents, toggleFullscreen } from './lifecycle.mjs'

/**
 * The panel lives on the host tab's lifecycle (ADR 0015, #43). Closing the
 * UML tab is the explicit close (D13) and stops the resident; collapsing the
 * sidebar or switching fullscreen only withdraws, so the resident and the
 * diagram are still there on return. The tab's refresh command and **R** both
 * reload. The panel has no `Expand`, `Conversation` or `Close viewer` of its own.
 */
test.skip(process.platform !== 'linux', 'Resident processes are observed through /proc.')

test('collapse and fullscreen keep the resident; refresh and R reload; closing the tab stops it', async ({ browser }) => {
  const lane = await launchViewerLane(browser)
  try {
    let shop = await lane.openPanel('shop')
    const project = lane.projects.shop
    const running = residents(project)
    expect(running).toHaveLength(1)
    for (const name of ['Expand to main area', 'Return to conversation', 'Close viewer', 'Open viewer']) {
      await expect(shop.panel.getByRole('button', { name, exact: true })).toHaveCount(0)
    }
    await lane.pan(shop)
    await expect.poll(() => shop.summary.innerText()).not.toBe(shop.before)
    const view = await shop.summary.innerText()

    // The host tab's refresh command and R are the same reload.
    const reloads = () => shop.sent.filter(action => action === 'reload').length
    const before = reloads()
    await refreshTab(shop)
    await expect.poll(reloads).toBeGreaterThan(before)
    const afterRefresh = reloads()
    await expect(shop.page.getByText('This page cannot be refreshed', { exact: true })).toHaveCount(0)
    await shop.canvas.focus()
    await shop.page.keyboard.press('r')
    await expect.poll(reloads).toBeGreaterThan(afterRefresh)
    await expect(shop.summary).toHaveText(view)

    // Collapsing withdraws: the resident stays and the diagram is there on return.
    await collapseSidebar(shop)
    await expect(shop.panel).toBeHidden()
    await shop.page.waitForTimeout(1000)
    expect(residents(project)).toEqual(running)
    await expandSidebar(shop)
    await expect(shop.panel).toBeVisible()
    await expect(shop.summary).toHaveText(view)
    expect(residents(project)).toEqual(running)

    // Fullscreen is the host's full-width form; it is not a close either.
    await toggleFullscreen(shop, true)
    await expect(shop.canvas).toBeVisible()
    await expect(shop.summary).toHaveText(view)
    await toggleFullscreen(shop, false)
    await expect(shop.summary).toHaveText(view)
    expect(residents(project)).toEqual(running)

    // Closing the UML tab stops the resident now, not after the grace.
    await closeTab(shop)
    await expect(shop.page.locator('.dsh-uml-viewer-panel')).toHaveCount(0)
    await expect.poll(() => residents(project), { timeout: 5000 }).toEqual([])

    // Reopening restores the saved view without another IR or display.
    shop = await lane.reopenTab(shop)
    await expect(shop.summary).toHaveText(view, { timeout: 30_000 })
    const fresh = residents(project)
    expect(fresh).toHaveLength(1)
    expect(fresh).not.toEqual(running)
    expect(shop.problems).toEqual([])
  } finally {
    await lane.close()
  }
})

test('reopening a closed UML tab restores the proposal discussion context', async ({ browser }) => {
  const lane = await launchViewerLane(browser)
  try {
    let ledger = await lane.openPanel('ledger')
    await expect(ledger.summary).toHaveAttribute('data-context', 'proposal')
    await lane.pan(ledger)
    await expect.poll(() => ledger.summary.innerText()).not.toBe(ledger.before)
    const view = await ledger.summary.innerText()
    expect(view).toContain('Discussion: Ledger proposal.')
    await closeTab(ledger)
    await expect.poll(() => residents(lane.projects.ledger), { timeout: 5000 }).toEqual([])
    ledger = await lane.reopenTab(ledger)
    await expect(ledger.summary).toHaveText(view, { timeout: 30_000 })
    await expect(ledger.summary).toHaveAttribute('data-context', 'proposal')
    expect(ledger.problems).toEqual([])
  } finally {
    await lane.close()
  }
})
