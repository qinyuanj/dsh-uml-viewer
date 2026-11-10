import { test, expect } from '@playwright/test'
import fs from 'node:fs/promises'
import path from 'node:path'
import { launchViewerLane } from './lane.mjs'
import { producerFrame, toScreen } from './canvas.mjs'
import { exposeTarget } from './gestures.mjs'
import { makeViewCompanion } from './view-companion.mjs'
import { VIEW_POLICY } from './view-policy.mjs'

/** #29 deterministic companion implementation of the protocol, NOT real LLM
 * compliance evidence. It edits only the lane-owned Deep scratch policy with
 * literal splices, once before returning uml_ir(request). Real IR/resident/
 * display are exercised by CI; no synthetic frame or production writer is used.
 * Local validation is Node-only; browser runtime requires CI's existing binary.
 */
async function refusingCrap(projects) {
  const root = projects.deep
  await fs.writeFile(path.join(root, 'quality-fixture.mjs'), `process.stderr.write('controlled quality refusal\\n'); process.exitCode = 1\n`)
  await fs.writeFile(path.join(root, 'uml-quality.json'), JSON.stringify({ version: 1, operations: {
    crap: { argv: [process.execPath, path.join(root, 'quality-fixture.mjs')], forwardArgs: 'none', validResultExitCodes: [0], scope: 'whole-tree' },
  } }))
}

async function launch(browser, quality = false) {
  const companion = makeViewCompanion()
  const lane = await launchViewerLane(browser, {
    prepareProjects: async projects => {
      companion.prepare(projects)
      if (quality) await refusingCrap(projects)
    },
    reply: companion.reply,
  })
  return { lane, companion }
}

function item(frame, id) {
  return frame.world.find(item => item.id === id && (item.kind === 'class' || item.kind === 'package'))
}

function targetPoint(frame, id) {
  const target = item(frame, id)
  expect(target, `frame contains ${id}`).toBeTruthy()
  return toScreen(frame, target.rect.x + target.rect.w / 2, target.rect.y + 10)
}

/** Fresh producer frame with `id` clickable beside the inspector (pans only if covered). */
async function exposed(panel, options, id) {
  return (await exposeTarget(panel, async () => {
    const frame = await producerFrame(panel, options)
    return { frame, point: targetPoint(frame, id) }
  })).frame
}

async function menu(panel, frame, id, label) {
  const [x, y] = targetPoint(frame, id)
  await panel.canvas.click({ position: { x, y }, button: 'right' })
  await panel.panel.getByRole('menu', { name: 'Element requests' }).getByRole('menuitem', { name: label, exact: true }).click()
}

function preserved(edit) {
  const offset = Buffer.byteLength(edit.original.slice(0, edit.start))
  expect(Buffer.from(edit.text).subarray(0, offset)).toEqual(Buffer.from(edit.original).subarray(0, offset))
  expect(Buffer.from(edit.text).subarray(offset + Buffer.byteLength(edit.after)))
    .toEqual(Buffer.from(edit.original).subarray(offset + Buffer.byteLength(edit.before)))
}

const composer = a => a.page.locator('[data-composer-input][contenteditable="true"]:visible').last()
/** The bottom status bar; its request line is the request status (#51). */
const statusBar = a => a.panel.locator('.dsh-uml-viewer-statusbar')
const finished = (a, op) => expect(a.page.getByText(`Finished ${op}: IR, display.`, { exact: true }).last()).toBeVisible()

test('Hide after a failed refresh still regenerates the IR, then displays; the draft stays', async ({ browser }) => {
  const { lane, companion } = await launch(browser, true)
  try {
    const a = await lane.openPanel('deep')
    const frame = await exposed(a, { root: lane.projects.deep, home: lane.home }, ':api')
    await composer(a).fill('Unfinished draft 草稿')
    const diagram = await fs.readFile(path.join(lane.projects.deep, 'diagram.edn'))
    await menu(a, frame, ':api', 'Refresh CRAP')
    await expect(a.page.getByText('Stopped refresh-crap', { exact: false }).last()).toBeVisible()
    expect(await fs.readFile(path.join(lane.projects.deep, 'diagram.edn'))).toEqual(diagram)
    expect(companion.edits).toHaveLength(0)
    await menu(a, frame, ':api', 'Hide')
    await expect(a.page.getByText(/^UML viewer panel request:\n\{:op :omit :target \{:id :api :kind :class\} :context \{:context :real\} :request "[0-9a-f]{32}"\}$/).last()).toBeVisible()
    await finished(a, 'omit')
    // #52: the companion's uml_ir and the display it followed completed the
    // hide, and the bar says so.
    await expect(statusBar(a)).toContainText('Hide :api: Done')
    await expect(statusBar(a)).toHaveAttribute('data-request', 'done')
    expect(companion.edits).toHaveLength(1)
    preserved(companion.edits[0])
    expect(await fs.readFile(path.join(lane.projects.deep, 'policy.edn'), 'utf8'))
      .toBe(VIEW_POLICY.replace('[  ] ; source-only', '[ :api ] ; source-only'))
    const after = await producerFrame(a, { root: lane.projects.deep, home: lane.home })
    expect(item(after, ':api'), 'regenerated source frame excludes the requested class').toBeUndefined()
    await expect(a.summary).toHaveAttribute('data-context', 'real')
    await expect(composer(a)).toHaveText('Unfinished draft 草稿')
    await expect(a.summary).toHaveAttribute('data-kind', 'frame')
    expect(a.problems).toEqual([])
    await lane.screenshot(a, 't29-hide')
  } finally { await lane.close() }
})

test('choosing a proposal tells the companion; 新建提案 sends a timestamp name; all in click order', async ({ browser }) => {
  const { lane, companion } = await launch(browser)
  try {
    const a = await lane.openPanel('deep')
    const inspector = a.panel.getByRole('complementary', { name: '检查器' })
    await inspector.getByRole('button', { name: 'Review plan', exact: true }).click()
    await expect(a.summary).toHaveAttribute('data-context', 'proposal')
    await expect(a.summary).toContainText('Discussion: Review plan.')
    await expect(a.page.getByText('Discussing the chosen diagram.', { exact: true }).last()).toBeVisible()
    const root = lane.projects.deep
    const grouped = await exposed(a, { root, home: lane.home, extra: '{:proposal-id :p-review}' }, ':proposal.surface')
    const surface = item(grouped, ':proposal.surface')
    expect(surface).toBeTruthy()
    await lane.point(a, toScreen(grouped, surface.rect.x + surface.rect.w / 2, surface.rect.y + 10), 2)
    await expect(a.summary).toHaveAttribute('data-depth', 'drilled')
    const layer = await exposed(a, { root, home: lane.home, extra: '{:proposal-id :p-review :open-layer :proposal.surface}' }, ':api')
    await composer(a).fill('Proposal draft 草稿')
    await menu(a, layer, ':api', 'Hide')
    await finished(a, 'omit')
    expect(companion.edits[0].context).toBe('p-review')
    preserved(companion.edits[0])
    expect(await fs.readFile(path.join(root, 'policy.edn'), 'utf8'))
      .toBe(VIEW_POLICY.replace('[ ]} ; proposal-only', '[:api]} ; proposal-only'))
    const hidden = await producerFrame(a, { root, home: lane.home, extra: '{:proposal-id :p-review :open-layer :proposal.surface}' })
    expect(item(hidden, ':api')).toBeUndefined()
    const source = await producerFrame(a, { root, home: lane.home })
    expect(item(source, ':api'), 'proposal omit leaves the source class visible').toBeTruthy()
    await inspector.getByRole('button', { name: '新建提案', exact: true }).click()
    const request = a.page.getByText(/^UML viewer panel request:\n\{:op :new-proposal :name "\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2}" :request "[0-9a-f]{32}"\}$/).last()
    await expect(request).toBeVisible()
    const name = /:name "([^"]+)"/.exec(await request.innerText())[1]
    await finished(a, 'new-proposal')
    expect(companion.edits).toHaveLength(2)
    const created = companion.edits[1]
    preserved(created)
    expect(created.context).toBe('p-review')
    expect(created.proposalId).toBe(':p-e2e-1')
    expect(await fs.readFile(path.join(root, 'policy.edn'), 'utf8')).toContain(`{:id :p-e2e-1 :name "${name}" :layers []}`)
    await expect(inspector.getByRole('button', { name, exact: true })).toBeVisible()
    // Creation preserves discussion context; choosing the actual new row changes it.
    await expect(a.summary).toContainText('Discussion: Review plan.')
    await inspector.getByRole('button', { name, exact: true }).click()
    await expect(a.summary).toContainText(`Discussion: ${name}.`)
    await expect.poll(() => companion.contexts.at(-1)).toBe('p-e2e-1')
    const unchanged = await fs.readFile(path.join(root, 'policy.edn'))
    await inspector.locator('.dsh-uml-proposals button').first().click()
    await expect(a.page.getByText('UML viewer panel request:\n{:op :context :context {:context :real}}', { exact: true }).last()).toBeVisible()
    // #51/#53: a context switch is done as soon as it is admitted, and it is
    // the newest entry, so the bar's line is the one that shows it. A done
    // entry fades ten seconds later, so this is where a finished state is read.
    await expect(statusBar(a)).toContainText('Switch discussion context: Done')
    await expect(a.summary).toHaveAttribute('data-context', 'real')
    await expect.poll(() => companion.contexts.at(-1)).toBe('real')
    expect(await fs.readFile(path.join(root, 'policy.edn'))).toEqual(unchanged)
    await expect(composer(a)).toHaveText('Proposal draft 草稿')
    expect(a.problems).toEqual([])
  } finally { await lane.close() }
})

test('failed IR after a scoped edit leaves the displayed diagram and draft unchanged', async ({ browser }) => {
  const { lane, companion } = await launch(browser)
  try {
    const a = await lane.openPanel('deep')
    const root = lane.projects.deep
    const frame = await exposed(a, { root, home: lane.home }, ':api')
    const diagram = await fs.readFile(path.join(root, 'diagram.edn'))
    const summary = await a.summary.innerText()
    await composer(a).fill('Keep failed request draft')
    // Deliberately malformed scratch policy makes the real IR call fail.
    await fs.writeFile(path.join(root, 'policy.edn'), VIEW_POLICY.replace(':hierarchical true', ':hierarchical [invalid'))
    const start = lane.mock.requests.length
    await menu(a, frame, ':api', 'Hide')
    await expect(a.page.getByText('Stopped omit: IR failed; no display.', { exact: true }).last()).toBeVisible()
    expect(companion.edits).toHaveLength(1)
    preserved(companion.edits[0])
    const replies = await Promise.all(lane.mock.requests.slice(start).map((_, i) => fs.readFile(
      path.join(lane.home, 'mock-requests', `${String(start + i + 1).padStart(3, '0')}-reply.json`), 'utf8').then(JSON.parse)))
    expect(replies.some(reply => reply.tool?.name === 'uml_ir')).toBe(true)
    expect(replies.some(reply => reply.tool?.name === 'uml_display')).toBe(false)
    expect(await fs.readFile(path.join(root, 'diagram.edn'))).toEqual(diagram)
    await expect(a.summary).toHaveText(summary)
    await expect(composer(a)).toHaveText('Keep failed request draft')
    expect(a.problems).toEqual([])
  } finally { await lane.close() }
})
