import { test, expect } from '@playwright/test'
import fs from 'node:fs/promises'
import path from 'node:path'
import { launchViewerLane } from './lane.mjs'
import { producerFrame, toScreen } from './canvas.mjs'

/**
 * #28: authored under RESOURCE-POLICY.md. Run only with an EXISTING binary:
 * UML_VIEWER_BIN=<absolute existing exe>
 * UML_VIEWER_E2E_SCRATCH=D:/dev/github/spec203-notes/scratch-t28
 * PLAYWRIGHT_BROWSERS_PATH=D:/dev/github/spec203-notes/scratch-t28/browsers
 * (set the last two for BOTH browser install and test invocations).
 *
 * The fixture quality operation is Node, explicitly configured in the scratch
 * project; it never builds or runs Cargo. IR and display use the real binary.
 * The scripted LLM follows the returned validResult; it is not a real model.
 */
function qualityReply(request) {
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
  const op = /:op :(refresh-[a-z-]+)/.exec(text)?.[1]
  const target = /:id ([^ ]+)/.exec(text)?.[1]
  const files = [...text.matchAll(/"((?:\\.|[^"\\])*)"/g)].map(match => JSON.parse(match[0]))
  const tool = op === 'refresh-crap' ? 'uml_crap' : 'uml_mutate'
  const tail = messages.slice(index)
  const serialized = JSON.stringify(tail)
  const done = name => tail.some(message => (message.content ?? []).some(part => part.type === 'tool_use' && part.name === name))
  if (!done(tool)) return { tool: { name: tool, input: tool === 'uml_crap' ? {} : { targets: files, ...(op === 'refresh-mutate-all' ? { mutateAll: true } : {}) } } }
  if (serialized.includes('\\"validResult\\":false')) return { text: `Stopped ${op}: quality returned validResult false; no IR or display.` }
  if (!done('uml_ir')) return { tool: { name: 'uml_ir', input: {} } }
  if (!done('uml_display')) return { tool: { name: 'uml_display', input: { path: 'diagram.edn' } } }
  return { text: `Finished ${op} for ${target}: quality, IR, display.` }
}

async function qualityFixture(projects) {
  for (const root of Object.values(projects)) {
    await fs.writeFile(path.join(root, 'quality-fixture.mjs'), `
import fs from 'node:fs'
const operation = process.argv[2]
const args = process.argv.slice(3)
fs.appendFileSync('quality-calls.jsonl', JSON.stringify({operation,args})+'\\n')
// The CRAP failure case is chosen by the test; all other calls succeed.
const exit = operation === 'crap' && fs.existsSync('refuse-crap') ? 1 : 0
process.stderr.write(exit ? 'controlled quality refusal\\n' : 'fixture quality snapshot accepted\\n')
process.exitCode = exit
`)
    await fs.writeFile(path.join(root, 'uml-quality.json'), JSON.stringify({
      version: 1,
      operations: Object.fromEntries(['crap', 'mutate'].map(op => [op, {
        argv: [process.execPath, path.join(root, 'quality-fixture.mjs'), op],
        forwardArgs: op === 'crap' ? 'none' : 'append', validResultExitCodes: [0], ...(op === 'crap' ? { scope: 'whole-tree' } : {}),
      }])),
    }))
  }
}

async function clickRefresh(panel, frame, id, label) {
  const item = frame.world.find(item => item.id === id && (item.kind === 'class' || item.kind === 'package'))
  expect(item, `frame contains ${id}`).toBeTruthy()
  const [x, y] = toScreen(frame, item.rect.x + item.rect.w / 2, item.rect.y + 10)
  await panel.canvas.click({ position: { x, y }, button: 'right' })
  await panel.panel.getByRole('menu', { name: 'Quality refresh' }).getByRole('menuitem', { name: label, exact: true }).click()
}

async function calls(root) {
  try { return (await fs.readFile(path.join(root, 'quality-calls.jsonl'), 'utf8')).trim().split('\n').map(line => JSON.parse(line)) }
  catch (error) { if (error.code === 'ENOENT') return []; throw error }
}

test('three visible bound-session requests leave the draft intact and run quality → IR → display', async ({ browser }) => {
  const lane = await launchViewerLane(browser, { prepareProjects: qualityFixture, reply: qualityReply })
  try {
    const a = await lane.openPanel('deep')
    const b = await lane.openPanel('atlas')
    const frame = await producerFrame(a, { root: lane.projects.deep, home: lane.home })
    const composer = a.page.locator('[data-composer-input][contenteditable="true"]:visible').last()
    await composer.fill('Unfinished draft 草稿')
    const expected = [
      ['Refresh CRAP', 'refresh-crap'],
      ['Differential mutation', 'refresh-mutate'],
      ['Full mutation (--mutate-all)', 'refresh-mutate-all'],
    ]
    for (const [label, op] of expected) {
      await clickRefresh(a, frame, ':core', label)
      await expect(composer).toHaveText('Unfinished draft 草稿')
      await expect(a.page.getByText(`Finished ${op} for :core: quality, IR, display.`, { exact: true }).last()).toBeVisible()
      const message = a.page.getByText(`UML viewer panel request:\n{:op :${op} :target {:id :core :kind :component} :files ["src/core/order.ts" "src/core/price.ts"]}`, { exact: true })
      await expect(message.last()).toBeVisible()
    }
    expect(await calls(lane.projects.deep)).toEqual([
      { operation: 'crap', args: [] },
      { operation: 'mutate', args: ['src/core/order.ts', 'src/core/price.ts'] },
      { operation: 'mutate', args: ['--mutate-all', 'src/core/order.ts', 'src/core/price.ts'] },
    ])
    expect(await calls(lane.projects.atlas)).toEqual([])
    await expect(b.page.getByText('UML viewer panel request:', { exact: false })).toHaveCount(0)
    await expect(composer).toHaveText('Unfinished draft 草稿')
    expect(a.problems).toEqual([])
    expect(b.problems).toEqual([])
    await lane.screenshot(a, 't28-requests-draft')
  } finally { await lane.close() }
})

test('validResult false stops the request before IR/display and reports why', async ({ browser }) => {
  const lane = await launchViewerLane(browser, { prepareProjects: qualityFixture, reply: qualityReply })
  try {
    const a = await lane.openPanel('deep')
    const frame = await producerFrame(a, { root: lane.projects.deep, home: lane.home })
    await fs.writeFile(path.join(lane.projects.deep, 'refuse-crap'), '')
    const before = lane.mock.requests.length
    await clickRefresh(a, frame, ':api', 'Refresh CRAP')
    await expect(a.page.getByText('Stopped refresh-crap: quality returned validResult false; no IR or display.', { exact: true }).last()).toBeVisible()
    const requests = lane.mock.requests.slice(before).map(r => JSON.parse(r.body))
    // Only the failed quality call follows the visible request: no IR, no display.
    const last = requests.at(-1).messages
    const at = last.findIndex(m => m.content?.some(p => p.text?.startsWith('UML viewer panel request:')))
    expect(at).toBeGreaterThanOrEqual(0)
    expect(last.slice(at).flatMap(m => m.content ?? []).filter(p => p.type === 'tool_use').map(p => p.name)).toEqual(['uml_crap'])
    // One binary invocation, and it refused before any snapshot was published.
    expect(await calls(lane.projects.deep)).toEqual([{ operation: 'crap', args: [] }])
  } finally { await lane.close() }
})
