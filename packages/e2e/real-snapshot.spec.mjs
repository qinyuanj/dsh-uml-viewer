import { test, expect } from '@playwright/test'
import { launchViewerLane } from './lane.mjs'
import { panelCamera, toScreen } from './canvas.mjs'
import { ROOT } from './dsh-web.mjs'
import {
  REAL_PROJECT,
  flattenSnapshot,
  locatorFrame,
  prepareRealProjects,
  prepareRealProjectsWithoutSnapshot,
  readDocument,
  readSnapshot,
} from './real-snapshot.mjs'
import path from 'node:path'

/**
 * #153: the panel painted from the repository's own committed `.metrics`
 * snapshot, not from a hand-written fixture.
 *
 * Everything asserted below is derived at run time from that snapshot and from
 * the IR the panel itself generated, so a refresh or a rename moves the
 * expectation with the data. The only hard-coded names are the crate `ir` and
 * the module `ir.graph`, the two long-stable namespaces this file anchors on;
 * they are called out where they are used.
 */

const name = item => item.id.slice(1)
const focusOf = path => path.split('.').map(part => `:${part}`).join(' ')
const mean = values => values.reduce((a, b) => a + b, 0) / values.length

/** The class a snapshot namespace belongs to: the longest dotted class id it starts with. */
function ownerClass(doc, ns) {
  let best = null
  for (const candidate of doc.classes.keys()) {
    if ((ns === candidate || ns.startsWith(`${candidate}.`)) && (!best || candidate.length > best.length)) best = candidate
  }
  return best
}

const bar = panel => panel.panel.getByRole('toolbar', { name: 'Selection actions', exact: true })

/**
 * Select a class by clicking its title strip, and prove the click landed on it:
 * the summary publishes `data-selected`, so a point that fell on an edge or a
 * member row fails here instead of reading another element's card.
 */
async function select(lane, panel, frame, className) {
  const box = frame.world.find(item => item.kind === 'class' && name(item) === className)
  expect(box, `the diagram draws a ${className} box`).toBeTruthy()
  const { x, y, w, h } = box.rect
  // The title strip, then the body: a click on a member row selects the member,
  // and a click on an edge selects the edge, so the panel's own answer decides.
  const points = [
    [x + 8, y + 8],
    [x + w / 2, y + 8],
    [x + w / 2, y + h / 2],
    [x + w - 12, y + 8],
  ]
  for (const point of points) {
    await lane.point(panel, toScreen(frame, ...point))
    const landed = await expect(panel.summary)
      .toHaveAttribute('data-selected', `:${className}`, { timeout: 3_000 })
      .then(() => true, () => false)
    if (landed) return
  }
  throw new Error(`could not select ${className}; the panel selected ${await panel.summary.getAttribute('data-selected')} instead`)
}

/**
 * Wait for the camera to stop moving. Drilling animates the zoom, and a frame
 * taken mid-animation puts every later click a few pixels off the box it means.
 */
async function settle(panel) {
  await panel.page.waitForTimeout(600)
  let previous = await panelCamera(panel)
  for (let attempt = 0; attempt < 40; attempt++) {
    await panel.page.waitForTimeout(100)
    const current = await panelCamera(panel)
    if (current.x === previous.x && current.y === previous.y && current.zoom === previous.zoom) return current
    previous = current
  }
  throw new Error('the panel camera never settled')
}

async function drillInto(lane, panel, frame, className, from = null) {
  const current = await frame(from)
  const box = current.world.find(item => item.kind === 'class' && name(item) === className)
  expect(box, `the diagram draws a ${className} container`).toBeTruthy()
  await lane.point(panel, toScreen(current, box.rect.x + box.rect.w / 2, box.rect.y + box.rect.h / 2), 2)
  await expect(panel.summary).toHaveAttribute('data-depth', 'drilled', { timeout: 30_000 })
  await settle(panel)
  return frame(focusOf(className))
}

/** Read the class card as rows of text and cells, keeping the column tips. */
async function readCard(dialog) {
  const rows = await dialog.locator('.dsh-uml-card-row').evaluateAll(list => list.map(row => ({
    member: row.getAttribute('data-member') ?? row.querySelector('[data-member]')?.getAttribute('data-member') ?? null,
    texts: [...row.querySelectorAll('.dsh-uml-card-text')].map(node => node.textContent.trim()),
    cells: [...row.querySelectorAll('.dsh-uml-card-cell')].map(node => node.textContent.trim()),
    tips: [...row.querySelectorAll('.dsh-uml-card-cell')].map(node => node.getAttribute('title')),
  })))
  return { text: await dialog.innerText(), rows }
}

/** Open one class's card through the action bar, so the click is verified twice. */
async function openCard(lane, panel, frame, doc, className) {
  await select(lane, panel, frame, className)
  await bar(panel).getByRole('button', { name: 'Open class card', exact: true }).click()
  const dialog = panel.panel.getByRole('dialog', { name: `Class card ${doc.classes.get(className).name}`, exact: true })
  await expect(dialog).toBeVisible({ timeout: 30_000 })
  const card = await readCard(dialog)
  await dialog.getByRole('button', { name: 'Close class card', exact: true }).click()
  await expect(dialog).toBeHidden({ timeout: 10_000 })
  return card
}

const classRow = (card, title) => card.rows.find(row => row.texts.includes(title) && row.cells.length > 0)

const memberRow = (card, member) =>
  card.rows.find(row => row.member === member) ?? card.rows.find(row => row.texts.includes(member))

test('paints quality, mutation and duplicate data from the committed snapshot', async ({ browser }) => {
  test.setTimeout(300_000)
  const lane = await launchViewerLane(browser, { prepareProjects: prepareRealProjects })
  try {
    const root = lane.projects[REAL_PROJECT]
    const panel = await lane.openPanel(REAL_PROJECT)
    const snapshot = await readSnapshot(root)
    const doc = await readDocument(root)

    // The snapshot under test is the repository's own, and it is not small.
    expect(snapshot.entries.size).toBeGreaterThan(50)
    expect(snapshot.mutate.size).toBeGreaterThan(3)
    expect(snapshot.dry.candidates).toBeGreaterThan(10)

    // Locate boxes with the same snapshot in the shape the scene CLI can read.
    const metrics = await flattenSnapshot(root, path.join(lane.home, 'locator-metrics'))
    const frame = (focus = null) => locatorFrame(panel, { root, metricsRoot: metrics, focus })
    const rootFrame = await frame()
    // The locator snapshot carries the committed dry.edn as-is, so its candidate
    // count is the snapshot's own.
    expect(rootFrame.dry?.candidates ?? 0).toBe(snapshot.dry.candidates)
    expect(rootFrame.world.some(item => item.kind === 'class' && item.id === ':ir')).toBe(true)

    // Anchor 1, named and long-stable: the `ir` crate.
    const irFrame = await drillInto(lane, panel, frame, 'ir')
    await lane.screenshot(panel, 'real-snapshot')

    const leaves = irFrame.world.filter(item => item.kind === 'class' && doc.isLeaf(name(item)))
    const measured = leaves.filter(item => snapshot.measured(name(item)))
    // A floor of zero would make the count below vacuous.
    expect(measured.length).toBeGreaterThanOrEqual(5)

    const cards = new Map()
    for (const leaf of leaves) {
      // Re-read the frame per card: selecting a class reveals the action bar and
      // resizes the canvas, so a frame read once drifts for the boxes below it.
      const fresh = await frame(focusOf('ir'))
      cards.set(name(leaf), await openCard(lane, panel, fresh, doc, name(leaf)))
    }

    const withQuality = [...cards].filter(([, card]) => card.text.includes('Crap μ')).map(([className]) => className)
    // Rename-tolerant invariant: at least as many classes paint CRAP-derived
    // quality data as the snapshot measures in this view. A refresh that adds
    // or drops modules moves both sides together.
    expect(withQuality.length).toBeGreaterThanOrEqual(measured.length)
    // No class the snapshot measures renders in the no-data state.
    expect(measured.map(name).filter(className => !withQuality.includes(className))).toEqual([])

    // Each covered class carries the two metrics independently: a CRAP-derived
    // pool (mean CRAP and coverage) and mutation counts, side by side.
    for (const className of measured.map(name)) {
      const row = classRow(cards.get(className), doc.classes.get(className).name)
      expect(row, `the ${className} card pools its quality into a summary row`).toBeTruthy()
      expect(row.cells).toHaveLength(5)
      expect(row.cells[0]).toMatch(/^-?[\d.]+μ$/)
      expect(row.cells[1]).toMatch(/^\d+%$/)
      for (const cell of row.cells.slice(2)) expect(cell).toMatch(/^\d+$/)
    }

    // Anchor 2, named and long-stable: `ir.graph` (crates/ir/src/graph.rs).
    // Its card must show the snapshot's own numbers, from both metrics.
    expect(withQuality).toContain('ir.graph')
    const graph = cards.get('ir.graph')
    const entries = snapshot.entries.get('ir.graph')
    expect(entries?.length).toBeGreaterThan(0)
    const pooled = classRow(graph, doc.classes.get('ir.graph').name)
    expect(pooled, 'the card pools the class into a summary row').toBeTruthy()
    expect(pooled.cells[0]).toBe(`${mean(entries.map(entry => entry.crap)).toFixed(1)}μ`)
    expect(pooled.cells[1]).toBe(`${Math.round(mean(entries.map(entry => entry.coverage)))}%`)
    const member = entries.find(entry => snapshot.outcome('ir.graph', entry.name).killed > 0)
    expect(member, 'the snapshot kills at least one mutant in ir.graph').toBeTruthy()
    const row = memberRow(graph, member.name)
    expect(row, `the card lists the measured member ${member.name}`).toBeTruthy()
    const outcomes = snapshot.outcome('ir.graph', member.name)
    expect(row.cells.slice(0, 5)).toEqual([
      member.crap.toFixed(1),
      String(member.complexity),
      `${Math.round(member.coverage)}%`,
      String(outcomes.killed),
      String(outcomes.survived),
    ])
    expect(row.cells[5]).toMatch(/^\d+$/)

    // The same project and the same view: a class the snapshot does not measure
    // keeps the empty state, so the assertions above are not always true.
    const unmeasured = leaves.map(name).filter(className => !snapshot.measured(className))
    expect(unmeasured.length).toBeGreaterThan(0)
    for (const className of unmeasured) expect(cards.get(className).text).not.toContain('Crap μ')

    // Duplicate structure: a candidate from dry.edn, reached from the card of
    // the class its namespace names. Chosen from the snapshot, not hard-coded.
    const duplicate = snapshot.dry.namespaces
      .map(ns => ({ ns, owner: ownerClass(doc, ns) }))
      .find(candidate => candidate.owner?.startsWith('ir.') && doc.isLeaf(candidate.owner))
    expect(duplicate, 'dry.edn names a duplicate inside the ir crate').toBeTruthy()
    const parent = duplicate.owner.split('.').slice(0, -1).join('.')
    const parentFrame = parent === 'ir'
      ? await frame(focusOf('ir'))
      : await drillInto(lane, panel, frame, parent, focusOf('ir'))
    const duplicateCard = await openCard(lane, panel, await frame(focusOf(parent)), doc, duplicate.owner)
    expect(duplicateCard.text).toMatch(/Duplicates: \d+ functions/)
    expect(Number(/Duplicates: (\d+) functions/.exec(duplicateCard.text)[1])).toBeGreaterThanOrEqual(2)
    expect(duplicateCard.text).toMatch(/weakest [\d.]+/)
  } finally {
    await lane.close()
  }
})

test('shows the no-data state when the committed snapshot is removed', async ({ browser }) => {
  test.setTimeout(300_000)
  const lane = await launchViewerLane(browser, { prepareProjects: prepareRealProjectsWithoutSnapshot })
  try {
    const root = lane.projects[REAL_PROJECT]
    const panel = await lane.openPanel(REAL_PROJECT)
    const doc = await readDocument(root)
    // What the committed snapshot measures, read from the repository itself:
    // this case has to fail exactly the assertions the positive case makes.
    const committed = await readSnapshot(ROOT)

    // The project's own .metrics is empty here, so the scene CLI and the panel
    // agree on "no data" and the plain project root locates boxes directly.
    const frame = (focus = null) => locatorFrame(panel, { root, metricsRoot: root, focus })
    const rootFrame = await frame()
    expect(rootFrame.dry?.candidates ?? 0).toBe(0)

    const irFrame = await drillInto(lane, panel, frame, 'ir')
    expect(committed.measured('ir.graph')).toBe(true)
    const card = await openCard(lane, panel, irFrame, doc, 'ir.graph')
    expect(card.text).not.toContain('Crap μ')
    expect(card.rows.flatMap(row => row.cells).filter(cell => /^\d/.test(cell))).toEqual([])
    await lane.screenshot(panel, 'real-snapshot-without-snapshot')
  } finally {
    await lane.close()
  }
})