import { test, expect } from '@playwright/test'
import fs from 'node:fs/promises'
import path from 'node:path'
import { launchViewerLane } from './lane.mjs'
import { producerFrame, toScreen } from './canvas.mjs'
import { calls, qualityFixture, qualityReply } from './quality-lane.mjs'

/**
 * #28: both cases passed against a real binary in CI5 (42591228938, f4c12e3).
 * Run only with an EXISTING binary:
 * UML_VIEWER_BIN=<absolute existing exe>
 * UML_VIEWER_E2E_SCRATCH=<scratch>/e2e-uml-viewer
 * PLAYWRIGHT_BROWSERS_PATH=<scratch>/e2e-uml-viewer/browsers
 * (set the last two for BOTH browser install and test invocations).
 *
 * The quality fixture and the scripted LLM are `quality-lane.mjs`.
 */
async function clickRefresh(panel, frame, id, label) {
  const item = frame.world.find(item => item.id === id && (item.kind === 'class' || item.kind === 'package'))
  expect(item, `frame contains ${id}`).toBeTruthy()
  const [x, y] = toScreen(frame, item.rect.x + item.rect.w / 2, item.rect.y + 10)
  await panel.canvas.click({ position: { x, y }, button: 'right' })
  await panel.panel.getByRole('menu', { name: 'Element requests' }).getByRole('menuitem', { name: label, exact: true }).click()
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
      // The display refreshed this panel's own stream, not another surface.
      await expect(a.summary).toHaveAttribute('data-kind', 'frame')
      await expect(a.panel).toHaveAttribute('data-session-id', a.sessionId)
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
    // #52: the companion's quality call failed the request, with a reason and
    // the exit code the version-1 record reported.
    const bar = a.panel.locator('.dsh-uml-viewer-statusbar')
    await expect(bar).toContainText('Refresh CRAP :api: Failed — the quality run reported validResult false (exit code 1)')
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
