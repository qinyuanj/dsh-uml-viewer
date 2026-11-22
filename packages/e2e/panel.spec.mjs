import { test, expect } from '@playwright/test'
import { launchViewerLane } from './lane.mjs'
import { producerFrame, toScreen } from './canvas.mjs'

const topBar = panel => panel.panel.locator('.dsh-uml-viewer-topbar')
const statusBar = panel => panel.panel.locator('.dsh-uml-viewer-statusbar')
const moreButton = panel => topBar(panel).getByRole('button', { name: 'More actions', exact: true })
const searchButton = panel => topBar(panel).getByRole('button', { name: 'Search classes (Ctrl+F)', exact: true })

async function focused(panel) {
  return await panel.page.evaluate(() => {
    const active = document.activeElement
    return active instanceof HTMLElement
      ? { className: active.className, tag: active.tagName, label: active.getAttribute('aria-label') }
      : null
  })
}

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

test('lays out the top bar, the canvas and the status bar in the narrow sidebar (#46)', async ({ browser }) => {
  const lane = await launchViewerLane(browser)
  try {
    const panel = await lane.openPanel('deep')
    const panelBox = await panel.panel.boundingBox()
    const [bar, canvas, status] = await Promise.all([
      topBar(panel).boundingBox(),
      panel.canvas.boundingBox(),
      statusBar(panel).boundingBox(),
    ])
    expect(bar, 'top bar').toBeTruthy()
    expect(canvas, 'canvas').toBeTruthy()
    expect(status, 'status bar').toBeTruthy()
    expect(bar.height).toBeGreaterThan(20)
    expect(bar.y).toBeGreaterThanOrEqual(panelBox.y - 1)
    expect(bar.y + bar.height).toBeLessThanOrEqual(panelBox.y + panelBox.height + 1)
    expect(bar.x).toBeGreaterThanOrEqual(panelBox.x - 1)
    expect(bar.x + bar.width).toBeLessThanOrEqual(panelBox.x + panelBox.width + 1)
    expect(canvas.y).toBeGreaterThanOrEqual(bar.y + bar.height - 1)
    expect(canvas.y + canvas.height).toBeLessThanOrEqual(status.y + 1)
    expect(canvas.height).toBeGreaterThan(150)
    expect(status.y + status.height).toBeLessThanOrEqual(panelBox.y + panelBox.height + 1)

    await expect(topBar(panel).getByRole('navigation', { name: 'Breadcrumb path' })).toBeVisible()
    await expect(topBar(panel).getByRole('button', { name: 'Discussion context: Source diagram', exact: true }))
      .toHaveAttribute('aria-haspopup', 'menu')
    await expect(topBar(panel).locator('.dsh-uml-viewer-switcher-name')).toHaveText('Source diagram')
    await expect(searchButton(panel)).toBeVisible()
    await expect(moreButton(panel)).toBeVisible()

    const root = await producerFrame(panel, { root: lane.projects.deep, home: lane.home })
    const core = root.world.find(item => item.id === ':core' && (item.kind === 'class' || item.kind === 'package'))
    expect(core, 'the root has a drillable core component').toBeTruthy()
    await lane.point(panel, toScreen(root, core.rect.x + core.rect.w / 2, core.rect.y + core.rect.h / 2), 2)
    await expect(panel.summary).toHaveAttribute('data-depth', 'drilled', { timeout: 30_000 })
    const path = topBar(panel).getByRole('navigation', { name: 'Breadcrumb path' })
    await path.getByRole('button').first().focus()
    await panel.page.keyboard.press('Tab')
    expect((await focused(panel))?.className).toContain('dsh-uml-viewer-switcher-button')
    await panel.page.keyboard.press('Tab')
    expect((await focused(panel))?.className).toContain('dsh-uml-viewer-search-open')
    await panel.page.keyboard.press('Tab')
    expect((await focused(panel))?.className).toContain('dsh-uml-viewer-more')

    const tokenBackground = token => panel.page.evaluate(name => {
      const probe = document.createElement('div')
      probe.style.background = `var(${name})`
      document.body.appendChild(probe)
      const value = getComputedStyle(probe).backgroundColor
      probe.remove()
      return value
    }, token)
    for (const dark of [false, true]) {
      await panel.page.evaluate(mode => document.body.toggleAttribute('data-ds-dark-theme', mode), dark)
      const mode = dark ? 'dark' : 'light'
      const [bar, status, token] = await Promise.all([
        topBar(panel).evaluate(element => getComputedStyle(element).backgroundColor),
        statusBar(panel).evaluate(element => getComputedStyle(element).backgroundColor),
        tokenBackground('--dsw-alias-bg-layer-2'),
      ])
      expect(bar, `top bar background in the ${mode} theme`).toBe(token)
      expect(status, `status bar background in the ${mode} theme`).toBe(token)
      expect(bar, `top bar surface in the ${mode} theme`).not.toBe('rgba(0, 0, 0, 0)')
    }
    await panel.page.evaluate(() => document.body.removeAttribute('data-ds-dark-theme'))

    const menu = topBar(panel).locator('.dsh-uml-viewer-more-menu')
    expect(await menu.getAttribute('role')).toBeNull()
    await moreButton(panel).click()
    await expect(menu).toHaveAttribute('role', 'menu')
    await expect(menu).toHaveAttribute('aria-label', 'More actions')
    const fit = menu.getByRole('menuitem', { name: /Fit view/ })
    await expect(fit).toBeVisible()
    await expect(fit).toHaveAttribute('aria-keyshortcuts', 'Ctrl+0')
    await expect(menu.locator('.dsh-uml-viewer-more-shortcut').first()).toHaveText('Ctrl+0')
    await expect(menu.getByRole('menuitemcheckbox', { name: 'Remove arrows', exact: true }))
      .toHaveAttribute('aria-checked', 'false')
    await panel.page.keyboard.press('Escape')
    expect(await menu.getAttribute('role')).toBeNull()
    await expect(menu).toBeHidden()
    expect(panel.problems).toEqual([])
  } finally {
    await lane.close()
  }
})

test('shows the canvas status in the bottom status bar before any display (#46)', async ({ browser }) => {
  const lane = await launchViewerLane(browser)
  try {
    const shop = await lane.openPanel('shop', { display: false })
    expect(await shop.summary.getAttribute('data-status')).toBe('waiting-display')
    await expect(statusBar(shop)).toBeVisible()
    await expect(statusBar(shop)).toHaveAttribute('aria-label', 'Panel status')
    await expect(statusBar(shop)).toContainText('Waiting for :display to load the diagram')
    await expect(statusBar(shop)).toHaveAttribute('data-code', 'waiting-display')
    expect(shop.problems).toEqual([])
  } finally {
    await lane.close()
  }
})
