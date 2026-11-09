import { test, expect } from '@playwright/test'
import { launchViewerLane } from './lane.mjs'
import { inkCount, producerFrame, sample, screenRect, toScreen } from './canvas.mjs'

/**
 * The panel equivalents of scripts/drivers/navigation.edn and search.edn.
 * Drill-in/back/P, selection and search are answered by the real resident;
 * only pan/zoom/hover are local. The dependency-port assertion reads the
 * browser's own pixels at the producer frame's port geometry.
 */
test('drills in and back, selects a dependency chain, searches and returns with P', async ({ browser }) => {
  const lane = await launchViewerLane(browser)
  try {
    const deep = await lane.openPanel('deep')
    const root = await producerFrame(deep, { root: lane.projects.deep, home: lane.home })
    const core = root.world.find(item => item.id === ':core' && (item.kind === 'class' || item.kind === 'package'))
    expect(core, 'the root has a drillable core component').toBeTruthy()
    await lane.point(deep, toScreen(root, core.rect.x + core.rect.w / 2, core.rect.y + core.rect.h / 2), 2)
    await expect(deep.summary).toHaveAttribute('data-depth', 'drilled', { timeout: 30_000 })

    const drilled = await producerFrame(deep, { root: lane.projects.deep, home: lane.home, focus: ':core' })
    expect(drilled.world.some(item => item.kind === 'class' && item.id === ':core.order')).toBe(true)
    const port = drilled.world.find(item => item.kind === 'port')
    expect(port, 'drill-in exposes dependencies outside the focus as ports').toBeTruthy()
    const background = (await sample(deep, [[drilled.viewport.width - 6, drilled.viewport.height - 6]]))[0]
    // In-browser assertion, not merely a JSON oracle: the real Canvas inks the
    // producer's port rectangle (its outline, fill and label) after drill-in.
    expect(await inkCount(deep, screenRect(drilled, port.rect), background, 12)).toBeGreaterThan(10)
    await lane.screenshot(deep, 'deep-drilled-port')

    const order = drilled.world.find(item => item.kind === 'class' && item.id === ':core.order')
    await lane.point(deep, toScreen(drilled, order.rect.x + order.rect.w / 2, order.rect.y + order.rect.h / 2))
    await expect(deep.summary).toHaveAttribute('data-selected', ':core.order')
    expect(Number(await deep.summary.getAttribute('data-chain'))).toBeGreaterThan(0)
    expect(Number(await deep.summary.getAttribute('data-dimmed'))).toBeGreaterThan(0)

    // Escape is back one level, not a local scene transformation.
    await deep.canvas.focus()
    await deep.page.keyboard.press('Escape')
    await expect(deep.summary).toHaveAttribute('data-depth', 'root')
    await deep.panel.getByRole('button', { name: 'Namespace tree (P)' }).waitFor()
    await lane.point(deep, toScreen(root, core.rect.x + core.rect.w / 2, core.rect.y + core.rect.h / 2), 2)
    await expect(deep.summary).toHaveAttribute('data-depth', 'drilled')
    await deep.canvas.focus()
    await deep.page.keyboard.press('p')
    await expect(deep.summary).toHaveAttribute('data-depth', 'root')

    // Ctrl+F, Chinese text input, same-name results, Down/Enter locate.
    await deep.canvas.focus()
    await deep.page.keyboard.press('Control+f')
    const search = deep.panel.getByRole('searchbox', { name: 'Search classes' })
    await expect(search).toBeFocused()
    await search.fill('解析')
    const options = deep.panel.getByRole('option')
    await expect(options).toHaveCount(2)
    await deep.page.keyboard.insertText('器')
    await expect(search).toHaveValue('解析器')
    await expect(options).toHaveCount(2)
    const labels = await options.allInnerTexts()
    expect(labels.some(label => label.includes('alpha.deep'))).toBe(true)
    expect(labels.some(label => label.includes('beta'))).toBe(true)
    await deep.page.keyboard.press('ArrowDown')
    await expect(options.first()).toHaveAttribute('aria-selected', 'true')
    await deep.page.keyboard.press('Enter')
    await expect(deep.summary).toHaveAttribute('data-depth', 'drilled')
    expect(await deep.summary.getAttribute('data-selected')).toContain('解析器')
    // The first Escape closes search without navigating back; P then returns.
    await deep.page.keyboard.press('Escape')
    await expect(deep.summary).toHaveAttribute('data-search', 'closed')
    await expect(deep.summary).toHaveAttribute('data-depth', 'drilled')
    await deep.canvas.focus()
    await deep.page.keyboard.press('p')
    await expect(deep.summary).toHaveAttribute('data-depth', 'root')
    expect(deep.problems).toEqual([])
  } finally {
    await lane.close()
  }
})

test('expands the same session into the root-scoped main page and back', async ({ browser }) => {
  const lane = await launchViewerLane(browser)
  try {
    const shop = await lane.openPanel('shop')
    await shop.panel.getByRole('button', { name: 'Expand to main area' }).click()
    const main = shop.page.locator('.dsh-uml-viewer-panel[data-surface="main"]')
    await expect(main).toBeVisible({ timeout: 30_000 })
    await expect(main).toHaveAttribute('data-session-id', shop.sessionId)
    await expect(main.locator('.dsh-uml-viewer-summary')).toContainText('Shop')
    await expect(main.locator('canvas')).toBeVisible()
    await expect(shop.page.locator('.dsh-uml-viewer-panel[data-surface="sidebar"]')).toBeHidden()
    await main.getByRole('button', { name: 'Return to conversation' }).click()
    await expect(main).toBeHidden()
    await expect(shop.panel).toBeVisible()
    await expect(shop.summary).toContainText('Shop')
    expect(shop.problems).toEqual([])
  } finally {
    await lane.close()
  }
})
