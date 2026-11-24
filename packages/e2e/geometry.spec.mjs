import { test, expect } from '@playwright/test'
import fs from 'node:fs/promises'
import path from 'node:path'
import { SCRATCH } from './dsh-web.mjs'
import { launchViewerLane } from './lane.mjs'

// #140: the panel's top bar must start below the DSH tab strip, and the More
// popover must hang off its trigger inside the panel instead of covering the
// host chrome. These are geometry facts, so they are measured in the browser.
const EVIDENCE = path.join(SCRATCH, 'evidence')

function measure() {
  const box = element => {
    if (!element) return null
    const r = element.getBoundingClientRect()
    return { top: r.top, bottom: r.bottom, left: r.left, right: r.right, width: r.width, height: r.height }
  }
  const panel = document.querySelector('.dsh-uml-viewer-panel')
  const pane = panel?.closest('[data-dockkit-pane]')
  const strip = pane?.querySelector('[data-dockkit-strip][role="tablist"]') ?? null
  const topbar = panel?.querySelector('.dsh-uml-viewer-topbar') ?? null
  const trigger = panel?.querySelector('.dsh-uml-viewer-more') ?? null
  const menu = panel?.querySelector('.dsh-uml-viewer-more-menu') ?? null
  return {
    strip: box(strip),
    panel: box(panel),
    topbar: box(topbar),
    trigger: box(trigger),
    menu: menu && !menu.hidden ? box(menu) : null,
    toolbarHeight: panel ? Number.parseFloat(getComputedStyle(panel).getPropertyValue('--dsh-uml-toolbar-height')) : Number.NaN,
    topbarOffsetHeight: topbar ? topbar.offsetHeight : Number.NaN,
    viewport: { width: window.innerWidth, height: window.innerHeight },
  }
}

test('the top bar and the More popover stay clear of the DSH tab strip (#140)', async ({ browser }) => {
  test.setTimeout(300_000)
  const lane = await launchViewerLane(browser)
  const observed = []
  try {
    const panel = await lane.openPanel('shop')
    const page = panel.page
    const more = page.locator('.dsh-uml-viewer-more').first()
    for (const width of [1440, 900, 757, 500]) {
      await page.setViewportSize({ width, height: 1000 })
      await page.waitForTimeout(500)
      await more.click()
      await page.waitForTimeout(250)
      const open = await page.evaluate(measure)
      observed.push({ width, ...open })
      await fs.mkdir(EVIDENCE, { recursive: true })
      await page.screenshot({ path: path.join(EVIDENCE, `issue140-${width}.png`) })
      await page.keyboard.press('Escape')
      await page.waitForTimeout(150)
    }
  } finally {
    await lane.close()
  }
  for (const at of observed) {
    const { width, strip, panel: pane, topbar, trigger, menu, toolbarHeight, topbarOffsetHeight, viewport } = at
    expect(strip, `${width}px: the DSH tab strip is measurable`).not.toBeNull()
    expect(pane).not.toBeNull()
    expect(topbar).not.toBeNull()
    expect(trigger).not.toBeNull()
    // the plugin's own toolbar begins where the host chrome ends, not on top of it
    expect(topbar.top, `${width}px: top bar starts below the DSH tab strip`).toBeGreaterThanOrEqual(strip.bottom - 0.5)
    expect(pane.top, `${width}px: the panel slot starts below the DSH tab strip`).toBeGreaterThanOrEqual(strip.bottom - 0.5)
    // the overlay insets follow the bar that actually rendered
    expect(toolbarHeight, `${width}px: --dsh-uml-toolbar-height matches the rendered bar`).toBeCloseTo(topbarOffsetHeight, 1)
    expect(menu, `${width}px: the More menu is open and measurable`).not.toBeNull()
    // anchored to the trigger, below it, never up over the tab strip
    expect(menu.top, `${width}px: the popover hangs below its trigger`).toBeGreaterThanOrEqual(trigger.bottom - 0.5)
    expect(menu.top, `${width}px: the popover never reaches the DSH tab strip`).toBeGreaterThanOrEqual(strip.bottom - 0.5)
    expect(menu.top - trigger.bottom, `${width}px: the popover sits next to its trigger, not across the panel`).toBeLessThanOrEqual(8)
    expect(Math.abs(menu.right - trigger.right), `${width}px: the popover's right edge follows the trigger`).toBeLessThanOrEqual(8)
    expect(menu.left, `${width}px: the popover starts left of the trigger's right edge`).toBeLessThan(trigger.right)
    expect(menu.left, `${width}px: the popover stays inside the panel`).toBeGreaterThanOrEqual(pane.left - 0.5)
    expect(menu.right, `${width}px: the popover stays inside the panel`).toBeLessThanOrEqual(pane.right + 0.5)
    expect(menu.bottom, `${width}px: the popover stays inside the window`).toBeLessThanOrEqual(viewport.height + 0.5)
  }
})
