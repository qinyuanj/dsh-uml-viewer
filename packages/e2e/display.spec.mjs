import { test, expect } from '@playwright/test'
import { launchViewerLane } from './lane.mjs'
import { panelViewport, producerFrame, toScreen } from './canvas.mjs'

test('scale-2 canvas rasterizes at device pixels, hits at CSS points and follows a resized page', async ({ browser }) => {
  const lane = await launchViewerLane(browser, { deviceScaleFactor: 2 })
  try {
    const shop = await lane.openPanel('shop')
    const backing = () => shop.canvas.evaluate(canvas => ({
      ratio: canvas.width / canvas.getBoundingClientRect().width,
      width: canvas.width, height: canvas.height, dpr: window.devicePixelRatio,
    }))
    const before = await panelViewport(shop)
    await expect.poll(async () => (await backing()).width).toBe(Math.round(before.width * 2))
    const first = await backing()
    expect(first.dpr).toBe(2)
    expect(first.height).toBe(Math.round(before.height * 2))
    expect(first.ratio).toBeCloseTo(2, 1)

    const frame = await producerFrame(shop, { root: lane.projects.shop, home: lane.home })
    const orders = frame.world.find(item => item.id === ':orders' && (item.kind === 'class' || item.kind === 'package'))
    expect(orders, 'the shop root shows the orders module').toBeTruthy()
    await lane.point(shop, toScreen(frame, orders.rect.x + orders.rect.w / 2, orders.rect.y + orders.rect.h / 2))
    await expect(shop.summary).toHaveAttribute('data-selected', ':orders')

    await shop.page.setViewportSize({ width: 1100, height: 700 })
    await expect.poll(async () => JSON.stringify(await panelViewport(shop))).not.toBe(JSON.stringify(before))
    const after = await panelViewport(shop)
    await expect.poll(async () => (await backing()).width).toBe(Math.round(after.width * 2))
    expect((await backing()).height).toBe(Math.round(after.height * 2))
    await lane.screenshot(shop, 'shop-dpi2-resized')
    expect(shop.problems).toEqual([])
  } finally {
    await lane.close()
  }
})
