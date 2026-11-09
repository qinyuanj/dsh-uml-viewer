import { test, expect } from '@playwright/test'
import fs from 'node:fs/promises'
import path from 'node:path'
import { launchViewerLane } from './lane.mjs'
import { producerFrame, toScreen } from './canvas.mjs'

/**
 * #29: not yet run against a real binary or browser. Run only with an
 * EXISTING binary:
 * UML_VIEWER_BIN=<absolute existing exe>
 * UML_VIEWER_E2E_SCRATCH=<scratch>/e2e-uml-viewer
 * PLAYWRIGHT_BROWSERS_PATH=<scratch>/e2e-uml-viewer/browsers
 * (set the last two for BOTH browser install and test invocations).
 *
 * The scripted LLM stands in for the companion: for :omit and :new-proposal
 * it runs uml_ir with the request id, then uml_display; for :context it only
 * answers. The policy edit itself is the companion's work (companion rules);
 * this spec checks the panel half: visible messages in click order, the draft
 * untouched, IR after a failed quality call, display only after IR success.
 */
function viewReply(request) {
  if (!request.tools?.length) return null
  const messages = request.messages ?? []
  let index = -1
  let text
  for (let i = 0; i < messages.length; i += 1) {
    for (const part of messages[i].content ?? []) {
      if (part.type === 'text' && part.text?.startsWith('UML viewer panel request:')) {
        index = i
        text = part.text
      }
    }
  }
  if (index < 0) return null
  const op = /:op :([a-z-]+)/.exec(text)?.[1]
  const id = /:request "([0-9a-f]{32})"/.exec(text)?.[1]
  const tail = messages.slice(index)
  const done = name => tail.some(message => (message.content ?? []).some(part => part.type === 'tool_use' && part.name === name))
  if (op === 'context') return { text: 'Discussing the chosen diagram.' }
  if (op === 'refresh-crap') {
    if (!done('uml_crap')) return { tool: { name: 'uml_crap', input: {} } }
    return { text: 'Stopped refresh-crap: quality returned validResult false; no IR or display.' }
  }
  if (!done('uml_ir')) return { tool: { name: 'uml_ir', input: { request: id } } }
  if (JSON.stringify(tail).includes('\\"validResult\\":false')) return { text: `Stopped ${op}: IR failed; no display.` }
  if (!done('uml_display')) return { tool: { name: 'uml_display', input: { path: 'diagram.edn' } } }
  return { text: `Finished ${op}: IR, display.` }
}

async function refusingCrap(projects) {
  for (const root of Object.values(projects)) {
    await fs.writeFile(path.join(root, 'quality-fixture.mjs'), `process.stderr.write('controlled quality refusal\\n'); process.exitCode = 1\n`)
    await fs.writeFile(path.join(root, 'uml-quality.json'), JSON.stringify({ version: 1, operations: {
      crap: { argv: [process.execPath, path.join(root, 'quality-fixture.mjs')], forwardArgs: 'none', validResultExitCodes: [0], scope: 'whole-tree' },
    } }))
  }
}

async function menu(panel, frame, id, label) {
  const item = frame.world.find(item => item.id === id && (item.kind === 'class' || item.kind === 'package'))
  expect(item, `frame contains ${id}`).toBeTruthy()
  const [x, y] = toScreen(frame, item.rect.x + item.rect.w / 2, item.rect.y + 10)
  await panel.canvas.click({ position: { x, y }, button: 'right' })
  await panel.panel.getByRole('menu', { name: 'Element requests' }).getByRole('menuitem', { name: label, exact: true }).click()
}

test('Hide after a failed refresh still regenerates the IR, then displays; the draft stays', async ({ browser }) => {
  const lane = await launchViewerLane(browser, { prepareProjects: refusingCrap, reply: viewReply })
  try {
    const a = await lane.openPanel('deep')
    const frame = await producerFrame(a, { root: lane.projects.deep, home: lane.home })
    const composer = a.page.locator('[data-composer-input][contenteditable="true"]:visible').last()
    await composer.fill('Unfinished draft 草稿')
    await menu(a, frame, ':api', 'Refresh CRAP')
    await expect(a.page.getByText('Stopped refresh-crap', { exact: false }).last()).toBeVisible()
    await menu(a, frame, ':api', 'Hide')
    await expect(a.page.getByText(/^UML viewer panel request:\n\{:op :omit :target \{:id :api :kind :class\} :context \{:context :real\} :request "[0-9a-f]{32}"\}$/).last()).toBeVisible()
    await expect(a.page.getByText('Finished omit: IR, display.', { exact: true }).last()).toBeVisible()
    await expect(composer).toHaveText('Unfinished draft 草稿')
    await expect(a.summary).toHaveAttribute('data-kind', 'frame')
    expect(a.problems).toEqual([])
    await lane.screenshot(a, 't29-hide')
  } finally { await lane.close() }
})

test('choosing a proposal tells the companion; 新建提案 sends a timestamp name; all in click order', async ({ browser }) => {
  const lane = await launchViewerLane(browser, { reply: viewReply })
  try {
    const a = await lane.openPanel('deep')
    await producerFrame(a, { root: lane.projects.deep, home: lane.home })
    const inspector = a.panel.getByRole('complementary', { name: '检查器' })
    await inspector.getByRole('button', { name: '新建提案', exact: true }).click()
    await expect(a.page.getByText(/^UML viewer panel request:\n\{:op :new-proposal :name "\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2}" :request "[0-9a-f]{32}"\}$/).last()).toBeVisible()
    await expect(a.page.getByText('Finished new-proposal: IR, display.', { exact: true }).last()).toBeVisible()
    // The source diagram row is the first entry; choosing it is a :context request.
    await inspector.locator('.dsh-uml-proposals button').first().click()
    await expect(a.page.getByText('UML viewer panel request:\n{:op :context :context {:context :real}}', { exact: true }).last()).toBeVisible()
    await expect(a.summary).toHaveAttribute('data-context', 'real')
    expect(a.problems).toEqual([])
  } finally { await lane.close() }
})
