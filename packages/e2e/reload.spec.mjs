import fs from 'node:fs/promises'
import path from 'node:path'
import { test, expect } from '@playwright/test'
import { launchViewerLane } from './lane.mjs'
import { producerFrame, toScreen } from './canvas.mjs'

/**
 * The panel equivalents of the deleted standalone.edn and metrics.edn
 * window drivers (dsh-uml-viewer#30): the resident polls the displayed
 * diagram and the project's `.metrics/` snapshots and publishes an
 * unprompted frame on a change (docs/resident-protocol.md, 热重载). No
 * prompt, `uml_ir` or `uml_display` runs between the edit and the check.
 */
let lane

test.beforeAll(async ({ browser }) => { lane = await launchViewerLane(browser) })
test.afterAll(async () => { await lane?.close() })

test('an edited diagram.edn hot-reloads into the open panel', async () => {
  const shop = await lane.openPanel('shop')
  await expect(shop.summary).toContainText('Diagram: Shop.')
  const diagram = path.join(lane.projects.shop, 'diagram.edn')
  const text = await fs.readFile(diagram, 'utf8')
  expect(text).toContain(':title "Shop"')
  await fs.writeFile(diagram, text.replace(':title "Shop"', ':title "Shop Reloaded"'))
  await expect(shop.summary).toContainText('Diagram: Shop Reloaded.', { timeout: 10_000 })
  expect(shop.problems).toEqual([])
})

test('a metric-only snapshot edit reloads quality without rewriting the IR', async () => {
  const shop = await lane.openPanel('shop')
  const root = lane.projects.shop
  const diagram = path.join(root, 'diagram.edn')
  const frame = await producerFrame(shop, { root, home: lane.home })
  const orders = frame.world.find(item => item.id === ':orders' && (item.kind === 'class' || item.kind === 'package'))
  expect(orders, 'the shop root shows the orders module').toBeTruthy()
  await lane.point(shop, toScreen(frame, orders.rect.x + orders.rect.w / 2, orders.rect.y + orders.rect.h / 2))
  await expect(shop.summary).toHaveAttribute('data-selected', ':orders')
  const summary = shop.panel.getByRole('complementary', { name: '检查器' }).locator('[data-summary-kind]')
  await expect(summary).toHaveAttribute('data-summary-kind', 'class')
  const before = await summary.innerText()
  const irStamp = (await fs.stat(diagram)).mtimeMs

  // First snapshot: the class gains CRAP data.
  const crap = path.join(root, '.metrics', 'crap.edn')
  await fs.mkdir(path.dirname(crap), { recursive: true })
  const snapshot = crap1 => `{:entries [
    {:name "Order" :namespace "shop.orders" :complexity 1 :coverage 100.0 :crap ${crap1}}]}`
  await fs.writeFile(crap, snapshot('1.0'))
  await expect.poll(() => summary.innerText(), { timeout: 10_000 }).not.toBe(before)
  const first = await summary.innerText()
  expect(first).toContain('Crap')

  // Equal-byte-length edit (1.0 -> 9.0): the stamp carries nanoseconds and
  // size, so a same-size rewrite must still publish a new frame.
  await fs.writeFile(crap, snapshot('9.0'))
  await expect.poll(() => summary.innerText(), { timeout: 10_000 }).not.toBe(first)
  expect(await summary.innerText()).toContain('9')
  expect((await fs.stat(diagram)).mtimeMs, 'a metric reload never rewrites the IR').toBe(irStamp)
  expect(shop.problems).toEqual([])
})
