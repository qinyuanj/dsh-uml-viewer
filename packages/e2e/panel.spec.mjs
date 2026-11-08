import { test, expect } from '@playwright/test'
import { launchViewerLane } from './lane.mjs'

test('opens real project diagrams and isolates two sessions', async ({ browser }) => {
  const lane = await launchViewerLane(browser)
  try {
    const shop = await lane.openPanel('shop')
    await expect(shop.panel).toBeVisible()
    await expect(shop.canvas).toBeVisible()
    await expect(shop.summary).toContainText('Shop')
    await expect(shop.summary).toContainText('Discussion: source diagram')
    await expect(shop.summary).toHaveAttribute('data-context', 'real')
    await expect(shop.summary).toHaveAttribute('data-view', 'source')

    const ledger = await lane.openPanel('ledger')
    await expect(ledger.summary).toContainText('Ledger')
    await expect(ledger.summary).toContainText('Discussion: Ledger proposal')
    await expect(ledger.summary).toHaveAttribute('data-context', 'proposal')
    await expect(ledger.summary).toHaveAttribute('data-view', 'proposal')
    await expect(shop.summary).toContainText('Shop')
    await expect(shop.summary).not.toContainText('Ledger')
    await expect(shop.summary).toHaveAttribute('data-context', 'real')

    await lane.pan(shop)
    await expect(shop.summary).not.toHaveText(shop.before)
    await expect(ledger.summary).toHaveText(ledger.before)
    await lane.screenshot(shop, 'shop-after-pan')
    await lane.screenshot(ledger, 'ledger-unchanged')
  } finally {
    await lane.close()
  }
})
