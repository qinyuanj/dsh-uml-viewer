import { test, expect } from '@playwright/test'
import { launchViewerLane } from './lane.mjs'
import { panDeltaBounds } from './canvas-probes.mjs'
import { inkCount, panelCamera, producerFrame, sample, screenRect, toScreen } from './canvas.mjs'
import { exposeTarget } from './gestures.mjs'
import { chooseContext } from './switcher.mjs'

/**
 * The panel equivalents of the deleted window drivers navigation.edn and
 * search.edn (scripts/drivers, removed in dsh-uml-viewer#30).
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

    // At the root Escape has no level to leave and no search to close, so it
    // clears the selection: the chain is search → back → clear (#46). This runs
    // first, while the diagram is unobstructed and the camera is the fit.
    const target = root.world
      .filter(item => item.kind === 'class')
      .map(item => {
        const [x, y] = toScreen(root, item.rect.x + item.rect.w / 2, item.rect.y + item.rect.h / 2)
        const inside = x > 24 && y > 24 && x < root.viewport.width - 24 && y < root.viewport.height - 24
        return inside ? { id: item.id, x, y, area: item.rect.w * item.rect.h } : null
      })
      .filter(Boolean)
      .sort((a, b) => b.area - a.area)[0]
    expect(target, 'the root view draws a class inside the canvas').toBeTruthy()
    await lane.point(deep, [target.x, target.y])
    await expect(deep.summary).toHaveAttribute('data-selected', target.id)
    await deep.canvas.focus()
    await deep.page.keyboard.press('Escape')
    await expect(deep.summary).toHaveAttribute('data-selected', '')
    await expect(deep.summary).toHaveAttribute('data-depth', 'root')

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
    // The toolbar's `Namespace tree` button is gone (#46): P is the only entry,
    // and the breadcrumb path names where the view is.
    await expect(deep.panel.getByRole('button', { name: 'Namespace tree (P)' })).toHaveCount(0)
    await lane.point(deep, toScreen(root, core.rect.x + core.rect.w / 2, core.rect.y + core.rect.h / 2), 2)
    await expect(deep.summary).toHaveAttribute('data-depth', 'drilled')
    await deep.canvas.focus()
    await deep.page.keyboard.press('p')
    await expect(deep.summary).toHaveAttribute('data-depth', 'root')
    // The switcher's source-diagram row is the same as P (#47).
    await lane.point(deep, toScreen(root, core.rect.x + core.rect.w / 2, core.rect.y + core.rect.h / 2), 2)
    await expect(deep.summary).toHaveAttribute('data-depth', 'drilled')
    await chooseContext(deep, 'Source diagram')
    await expect(deep.summary).toHaveAttribute('data-depth', 'root')
    await expect(deep.summary).toHaveAttribute('data-context', 'real')

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

/**
 * The top bar's breadcrumb path (#45): the state event carries the tree path,
 * every level above the current one returns straight to it, and a proposal's
 * path starts at the proposal rather than at the source diagram.
 */
test('returns to any breadcrumb level of the source tree and of a proposal', async ({ browser }) => {
  const lane = await launchViewerLane(browser)
  const proposalContext = {
    context: { kind: 'proposal', proposalId: ':p-levels', name: 'Levels proposal',
      edn: '{:context :proposal :name "Levels proposal" :proposal-id :p-levels}' },
    proposalId: ':p-levels',
  }
  /** Double-click a box of the view the producer describes, once it is on the canvas. */
  const drill = async (panel, view) => {
    const measure = async () => {
      const frame = await producerFrame(panel, {
        root: lane.projects.levels, home: lane.home,
        ...(view.focus ? { focus: view.focus } : {}),
        ...(view.extra ? { extra: view.extra } : {}),
      })
      const box = frame.world.find(entry => entry.id === view.id
        && (entry.kind === 'class' || entry.kind === 'package'))
      expect(box, `the view shows ${view.id}`).toBeTruthy()
      return { frame, point: toScreen(frame, box.rect.x + box.rect.w / 2, box.rect.y + box.rect.h / 2) }
    }
    const measured = await exposeTarget(panel, measure)
    await lane.point(panel, measured.point, 2)
  }
  const atRoot = panel => expect(panel.summary).toHaveAttribute('data-depth', 'root')
  const current = path => path.locator('[aria-current="page"]')
  try {
    const levels = await lane.openPanel('levels')
    const path = levels.panel.getByRole('navigation', { name: 'Breadcrumb path' })
    // The path starts at the diagram; the level shown is not a button.
    await expect(path).toContainText('Levels')
    await expect(path.getByRole('button')).toHaveCount(0)

    // Two namespace levels down: root → core → core.inner.
    await drill(levels, { id: ':core' })
    await expect(path.getByRole('button', { name: 'Go to Levels', exact: true })).toBeVisible()
    await drill(levels, { id: ':core.inner', focus: ':core' })
    await expect(levels.summary).toHaveAttribute('data-depth', 'drilled')
    await expect(current(path)).toHaveText('inner')

    // The middle level: one click back to `core`, from two levels below it.
    await path.getByRole('button', { name: 'Go to core', exact: true }).click()
    await expect(current(path)).toHaveText('core')
    const atCore = await producerFrame(levels, { root: lane.projects.levels, home: lane.home, focus: ':core' })
    const inner = atCore.world.find(entry => entry.id === ':core.inner' && entry.kind === 'class')
    expect(inner, 'the core level is the one drawn').toBeTruthy()
    const background = (await sample(levels, [[atCore.viewport.width - 6, atCore.viewport.height - 6]]))[0]
    expect(await inkCount(levels, screenRect(atCore, inner.rect), background, 12), 'the core level is drawn').toBeGreaterThan(10)

    // The root: the whole diagram again.
    await path.getByRole('button', { name: 'Go to Levels', exact: true }).click()
    await atRoot(levels)

    // The proposal's path is the proposal's, with its layer and nested group.
    const proposal = await lane.openPanel('levels', { state: proposalContext })
    await expect(proposal.summary).toHaveAttribute('data-view', 'proposal')
    const proposalPath = proposal.panel.getByRole('navigation', { name: 'Breadcrumb path' })
    await expect(current(proposalPath)).toHaveText('Levels proposal')
    const inProposal = '{:proposal true :proposal-id :p-levels}'
    await drill(proposal, { id: ':proposal.domain', extra: inProposal })
    await drill(proposal, { id: ':core-group', extra: '{:proposal true :proposal-id :p-levels :open-layer :proposal.domain}' })
    await expect(proposal.summary).toHaveAttribute('data-depth', 'drilled')
    await expect(current(proposalPath)).toHaveText('Core')

    // Back to the layer, then to the proposal's root: never to the source tree.
    await proposalPath.getByRole('button', { name: 'Go to Domain', exact: true }).click()
    await expect(current(proposalPath)).toHaveText('Domain')
    await proposalPath.getByRole('button', { name: 'Go to Levels proposal', exact: true }).click()
    await atRoot(proposal)
    await expect(proposal.summary).toHaveAttribute('data-view', 'proposal')
    expect(levels.problems).toEqual([])
    expect(proposal.problems).toEqual([])
  } finally {
    await lane.close()
  }
})

/** A canvas point no producer world item covers (navigation.edn's blank drag). */
function blankPoint(frame) {
  const inside = ([x, y]) => frame.world.some(item => item.rect && (() => {
    const [sx, sy] = toScreen(frame, item.rect.x, item.rect.y)
    const z = frame.camera.zoom
    return x >= sx && y >= sy && x <= sx + item.rect.w * z && y <= sy + item.rect.h * z
  })())
  for (let y = frame.viewport.height - 30; y > 80; y -= 20) {
    // Both ends of the 100px leftward drag must remain on the canvas.
    for (let x = 130; x < frame.viewport.width - 140; x += 20) {
      if (!inside([x, y]) && !inside([x - 100, y - 50])) return [x, y]
    }
  }
  throw new Error('no blank canvas point for the drag')
}

/** World point under a canvas-local CSS point for a summary camera. */
const world = (camera, [x, y]) => [camera.x + x / camera.zoom, camera.y + y / camera.zoom]

/**
 * The rest of the deleted navigation.edn window driver (dsh-uml-viewer#30):
 * cursor-anchored Ctrl+wheel zoom, blank-canvas drag pan, plain wheel pan,
 * Ctrl+0 fit and the five-step declutter cycle that keeps the view.
 */
test('anchors Ctrl+wheel zoom, pans by drag and wheel, fits with Ctrl+0 and toggles 移除箭头 in place', async ({ browser }) => {
  const lane = await launchViewerLane(browser)
  try {
    const deep = await lane.openPanel('deep')
    const fitted = await panelCamera(deep)
    const box = await deep.canvas.boundingBox()

    // One Ctrl+wheel notch at a cursor point zooms by 1.1 and keeps that
    // point's world position.
    const anchor = [Math.round(box.width * 0.3), Math.round(box.height * 0.4)]
    await deep.page.mouse.move(box.x + anchor[0], box.y + anchor[1])
    await deep.page.keyboard.down('Control')
    await deep.page.mouse.wheel(0, -100)
    await deep.page.keyboard.up('Control')
    await expect.poll(async () => (await panelCamera(deep)).zoom).toBeCloseTo(fitted.zoom * 1.1, 2)
    const zoomed = await panelCamera(deep)
    const [ax, ay] = world(fitted, anchor)
    const [bx, by] = world(zoomed, anchor)
    expect(Math.abs(ax - bx), 'zoom keeps the cursor world x').toBeLessThan(0.05)
    expect(Math.abs(ay - by), 'zoom keeps the cursor world y').toBeLessThan(0.05)

    // A blank drag moves the camera by the screen delta over the zoom.
    const frame = await producerFrame(deep, { root: lane.projects.deep, home: lane.home })
    const [fx, fy] = blankPoint(frame)
    await deep.page.mouse.move(box.x + fx, box.y + fy)
    await deep.page.mouse.down()
    await deep.page.mouse.move(box.x + fx - 100, box.y + fy - 50, { steps: 8 })
    await deep.page.mouse.up()
    await expect.poll(async () => (await panelCamera(deep)).x).not.toBe(zoomed.x)
    const panned = await panelCamera(deep)
    expect(panned.zoom).toBe(zoomed.zoom)
    for (const [axis, pixels] of [['x', 100], ['y', 50]]) {
      const [min, max] = panDeltaBounds(pixels, zoomed.zoom)
      expect(panned[axis] - zoomed[axis], `drag ${axis}: rounded camera lower bound`).toBeGreaterThanOrEqual(min)
      expect(panned[axis] - zoomed[axis], `drag ${axis}: rounded camera upper bound`).toBeLessThanOrEqual(max)
    }
    await expect(deep.summary).toHaveAttribute('data-depth', 'root')

    // A plain wheel notch down pans down, never zooms.
    await deep.page.mouse.move(box.x + box.width / 2, box.y + box.height / 2)
    await deep.page.mouse.wheel(0, 100)
    await expect.poll(async () => (await panelCamera(deep)).y).toBeGreaterThan(panned.y)
    expect((await panelCamera(deep)).zoom).toBe(panned.zoom)

    // Ctrl+0 fits again: the camera the fresh load fitted.
    await deep.canvas.focus()
    await deep.page.keyboard.press('Control+0')
    await expect.poll(() => panelCamera(deep)).toEqual(fitted)

    // 移除箭头 is the 「更多」 menu's toggle now (#46): one click clears the
    // arrows, the next brings them back, and the view is rebuilt in place.
    const view = await deep.summary.innerText()
    const selected = await deep.summary.getAttribute('data-selected')
    const more = deep.panel.getByRole('button', { name: 'More actions', exact: true })
    const menu = deep.panel.locator('.dsh-uml-viewer-more-menu')
    const arrows = () => menu.getByRole('menuitemcheckbox', { name: 'Remove arrows', exact: true })
    const toggleArrows = async () => {
      await more.click()
      await arrows().click()
    }
    await toggleArrows()
    await more.click()
    await expect(arrows()).toHaveAttribute('aria-checked', 'true')
    await deep.page.keyboard.press('Escape')
    const arrowsRemoved = await producerFrame(deep, {
      root: lane.projects.deep, home: lane.home, extra: '{:declutter :triangles}',
    })
    expect(arrowsRemoved.edges.length).toBeGreaterThan(0)
    expect(arrowsRemoved.edges.every(edge => edge.kind === 'indicator')).toBe(true)
    await toggleArrows()
    await more.click()
    await expect(arrows()).toHaveAttribute('aria-checked', 'false')
    await deep.page.keyboard.press('Escape')
    await expect(deep.summary).toHaveText(view)
    await expect(deep.summary).toHaveAttribute('data-selected', selected ?? '')
    await expect(deep.summary).toHaveAttribute('data-depth', 'root')
    expect(deep.problems).toEqual([])
  } finally {
    await lane.close()
  }
})
