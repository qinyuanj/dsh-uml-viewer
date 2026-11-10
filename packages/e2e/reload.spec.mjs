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
  const orders = frame.world.find(item => item.id === ':orders' && item.kind === 'class')
  expect(orders, 'the shop root shows the orders module').toBeTruthy()
  // The class card carries the class's quality figures since the inspector's
  // summary line was deleted (#47); it reloads with every new frame.
  const [x, y] = toScreen(frame, orders.rect.x + 12, orders.rect.y + 12)
  await shop.canvas.dblclick({ position: { x, y } })
  const card = shop.panel.getByRole('dialog', { name: /^Class card / })
  await expect(card).toBeVisible()
  const before = await card.innerText()
  const irStamp = (await fs.stat(diagram)).mtimeMs

  // First snapshot: the class gains CRAP data.
  const crap = path.join(root, '.metrics', 'crap.edn')
  await fs.mkdir(path.dirname(crap), { recursive: true })
  const snapshot = crap1 => `{:entries [
    {:name "Order" :namespace "shop.orders" :complexity 1 :coverage 100.0 :crap ${crap1}}]}`
  await fs.writeFile(crap, snapshot('1.0'))
  await expect.poll(() => card.innerText(), { timeout: 10_000 }).not.toBe(before)
  const first = await card.innerText()
  expect(first).toContain('1.0')

  // Equal-byte-length edit (1.0 -> 9.0): the stamp carries nanoseconds and
  // size, so a same-size rewrite must still publish a new frame.
  await fs.writeFile(crap, snapshot('9.0'))
  await expect.poll(() => card.innerText(), { timeout: 10_000 }).not.toBe(first)
  expect(await card.innerText()).toContain('9.0')
  expect((await fs.stat(diagram)).mtimeMs, 'a metric reload never rewrites the IR').toBe(irStamp)
  expect(shop.problems).toEqual([])
})
