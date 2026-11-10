import { test, expect } from '@playwright/test'
import { launchViewerLane } from './lane.mjs'
import { producerFrame, toScreen } from './canvas.mjs'
import { exposeTarget } from './gestures.mjs'
import { calls, qualityFixture, qualityReply } from './quality-lane.mjs'

/**
 * The selection action bar (#48, ADR 0015): selecting a class or a component
 * shows a bar fixed above the status bar with the element's name and what can
 * be done to it; a foreign package shows its name alone. Its requests are the
 * element menu's, which stays an equivalent entry. Real DSH Web, real resident;
 * the quality fixture and scripted LLM are `quality-lane.mjs`.
 */

const center = (frame, item) => toScreen(frame, item.rect.x + item.rect.w / 2, item.rect.y + item.rect.h / 2)

/** A canvas point inside the view frame but on no element, away from the bar. */
function blankPoint(frame) {
  const { width, height } = frame.viewport
  const boxes = frame.world.filter(item => item.kind === 'class').map(item => {
    const [x, y] = toScreen(frame, item.rect.x, item.rect.y)
    return { x, y, w: item.rect.w * frame.camera.zoom, h: item.rect.h * frame.camera.zoom }
  })
  for (let y = 40; y < height * 0.6; y += 10) {
    for (let x = 30; x < width - 30; x += 10) {
      if (boxes.every(box => x < box.x - 12 || x > box.x + box.w + 12 || y < box.y - 12 || y > box.y + box.h + 12)) return [x, y]
    }
  }
  throw new Error('no blank canvas point')
}

test('a component’s bar sends a visible panel request, keeps full mutation secondary and stays put through pan and zoom', async ({ browser }) => {
  const lane = await launchViewerLane(browser, { prepareProjects: qualityFixture, reply: qualityReply })
  try {
    const deep = await lane.openPanel('deep')
    const bar = deep.panel.getByRole('toolbar', { name: 'Selection actions', exact: true })
    await expect(bar).toBeHidden()
    const frame = await producerFrame(deep, { root: lane.projects.deep, home: lane.home })
    const core = frame.world.find(item => item.kind === 'class' && item.id === ':core')
    expect(core, 'the root draws the core component').toBeTruthy()
    await lane.point(deep, center(frame, core))
    await expect(deep.summary).toHaveAttribute('data-selected', ':core')
    await expect(bar).toBeVisible()
    await expect(bar).toHaveAttribute('data-kind', 'component')
    await expect(bar.locator('.dsh-uml-viewer-action-name')).toHaveText('Core')
    const visible = bar.locator('button:visible')
    await expect(visible).toHaveCount(5)
    for (const name of ['Drill in', 'Refresh CRAP', 'Differential mutation', 'Hide', 'More requests']) {
      await expect(bar.getByRole('button', { name, exact: true })).toBeVisible()
    }
    // Full mutation lives only in the secondary menu, which is a menu only while open.
    await expect(bar.getByText('Full mutation (--mutate-all)', { exact: true })).toBeHidden()
    await expect(deep.page.locator('[role="menu"]')).toHaveCount(0)
    await bar.getByRole('button', { name: 'More requests', exact: true }).click()
    const secondary = bar.getByRole('menu', { name: 'More requests', exact: true })
    await expect(secondary.getByRole('menuitem')).toHaveText(['Full mutation (--mutate-all)'])
    await deep.page.keyboard.press('Escape')
    await expect(deep.page.locator('[role="menu"]')).toHaveCount(0)
    await expect(bar).toBeVisible()
    await lane.screenshot(deep, 't48-action-bar-component')

    // Pan and zoom move the diagram, never the bar; the selection stays.
    const fixed = await bar.boundingBox()
    const canvas = await deep.canvas.boundingBox()
    const [bx, by] = blankPoint(frame)
    await deep.page.mouse.move(canvas.x + bx, canvas.y + by)
    await deep.page.mouse.down()
    await deep.page.mouse.move(canvas.x + bx - 80, canvas.y + by - 40, { steps: 8 })
    await deep.page.mouse.up()
    await deep.page.mouse.move(canvas.x + canvas.width / 2, canvas.y + canvas.height / 3)
    await deep.page.mouse.wheel(0, 100)
    await deep.page.keyboard.down('Control')
    await deep.page.mouse.wheel(0, -100)
    await deep.page.keyboard.up('Control')
    await expect.poll(async () => (await deep.summary.innerText())).not.toContain(`Camera: ${frame.camera.x.toFixed(3)}, ${frame.camera.y.toFixed(3)}, ${frame.camera.zoom.toFixed(3)}.`)
    await expect(deep.summary).toHaveAttribute('data-selected', ':core')
    expect(await bar.boundingBox()).toEqual(fixed)

    // The right-click menu keeps the same four requests.
    const moved = await producerFrame(deep, { root: lane.projects.deep, home: lane.home })
    const coreNow = moved.world.find(item => item.kind === 'class' && item.id === ':core')
    const [cx, cy] = toScreen(moved, coreNow.rect.x + coreNow.rect.w / 2, coreNow.rect.y + 10)
    await deep.canvas.click({ position: { x: cx, y: cy }, button: 'right' })
    const menu = deep.panel.getByRole('menu', { name: 'Element requests', exact: true })
    await expect(menu.getByRole('menuitem')).toHaveText(['Refresh CRAP', 'Differential mutation', 'Full mutation (--mutate-all)', 'Hide'])
    await deep.page.keyboard.press('Escape')
    await expect(deep.page.locator('[role="menu"]')).toHaveCount(0)

    // Refresh CRAP from the bar: the request shows up in the conversation.
    await bar.getByRole('button', { name: 'Refresh CRAP', exact: true }).click()
    await expect(deep.page.getByText('UML viewer panel request:\n{:op :refresh-crap :target {:id :core :kind :component} :files ["src/core/order.ts" "src/core/price.ts"]}', { exact: true }).last()).toBeVisible()
    await expect(deep.page.getByText('Finished refresh-crap for :core: quality, IR, display.', { exact: true }).last()).toBeVisible()
    expect(await calls(lane.projects.deep)).toEqual([{ operation: 'crap', args: [] }])
    expect(deep.problems).toEqual([])
  } finally { await lane.close() }
})

test('a class’s bar opens its card; a foreign package shows its name alone', async ({ browser }) => {
  const lane = await launchViewerLane(browser)
  try {
    const atlas = await lane.openPanel('atlas')
    const bar = atlas.panel.getByRole('toolbar', { name: 'Selection actions', exact: true })
    // The floating inspector covers the canvas's right edge; pan a target out
    // from under it only when it is covered.
    const target = find => exposeTarget(atlas, async () => {
      const frame = await producerFrame(atlas, { root: lane.projects.atlas, home: lane.home })
      const item = frame.world.find(find)
      expect(item, 'atlas draws the target').toBeTruthy()
      return { frame, item, point: center(frame, item) }
    })

    const { point: gatewayAt } = await target(item => item.kind === 'class' && item.id === ':gateway')
    await lane.point(atlas, gatewayAt)
    await expect(atlas.summary).toHaveAttribute('data-selected', ':gateway')
    await expect(bar).toHaveAttribute('data-kind', 'class')
    await expect(bar.locator('.dsh-uml-viewer-action-name')).toHaveText('Gateway')
    const view = await atlas.summary.innerText()
    await bar.getByRole('button', { name: 'Open class card', exact: true }).click()
    const card = atlas.panel.getByRole('dialog', { name: 'Class card Gateway', exact: true })
    await expect(card).toBeVisible()
    // Opening the card from the bar does not move the view.
    await expect(atlas.summary).toHaveText(view)
    await lane.screenshot(atlas, 't48-action-bar-class-card')
    await card.getByRole('button', { name: 'Close class card', exact: true }).click()
    await expect(card).toBeHidden()

    // Escape clears the selection at the root, and the bar goes with it.
    await atlas.canvas.focus()
    await atlas.page.keyboard.press('Escape')
    await expect(atlas.summary).toHaveAttribute('data-selected', '')
    await expect(bar).toBeHidden()

    const { item: react, point: reactAt } = await target(item => item.kind === 'class' && item.shape === 'oval')
    await lane.point(atlas, reactAt)
    await expect(atlas.summary).toHaveAttribute('data-selected', react.id)
    await expect(bar).toBeVisible()
    await expect(bar).toHaveAttribute('data-kind', 'foreign')
    await expect(bar.locator('.dsh-uml-viewer-action-name')).toHaveText('react')
    await expect(bar.getByRole('button')).toHaveCount(0)
    await lane.screenshot(atlas, 't48-action-bar-foreign')
    expect(atlas.problems).toEqual([])
  } finally { await lane.close() }
})
