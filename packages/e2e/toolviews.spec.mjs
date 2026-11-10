import fsSync from 'node:fs'
import fs from 'node:fs/promises'
import path from 'node:path'
import { test, expect } from '@playwright/test'
import { producerFrame, toScreen } from './canvas.mjs'
import { launchViewerLane } from './lane.mjs'
import { closeTab } from './lifecycle.mjs'

/**
 * #54: the five `uml_*` tools read as their outcome in the conversation, and
 * `uml_display` carries 「在面板中查看」. Real binary, real resident, real
 * browser; the model is the scripted mock endpoint, which is not evidence of a
 * real model obeying the companion rules.
 *
 * The quality operations are a Node fixture declared in the scratch project, so
 * a run costs no Cargo work; IR, resident and display are the real binary's.
 */
async function qualityFixture(projects) {
  for (const root of Object.values(projects)) {
    await fs.writeFile(path.join(root, 'quality-fixture.mjs'), `
import fs from 'node:fs'
const operation = process.argv[2]
fs.appendFileSync('quality-calls.jsonl', JSON.stringify({operation, args: process.argv.slice(3)})+'\\n')
process.stderr.write('fixture quality snapshot accepted\\n')
`)
    await fs.writeFile(path.join(root, 'uml-quality.json'), declared(process.execPath, root))
  }
}

/** Both operations declared: a valid quality run. */
function declared(node, root) {
  return JSON.stringify({
    version: 1,
    operations: {
      crap: { argv: [node, path.join(root, 'quality-fixture.mjs'), 'crap'], forwardArgs: 'none', validResultExitCodes: [0], scope: 'whole-tree' },
      mutate: { argv: [node, path.join(root, 'quality-fixture.mjs'), 'mutate'], forwardArgs: 'append', validResultExitCodes: [0] },
    },
  })
}

/** A user message's own text; tool results travel as user-role messages with none. */
function userText(message) {
  return (message.content ?? []).filter(part => part.type === 'text').map(part => part.text ?? '').join('\n')
}

/** The latest user text: a chat prompt or a panel request. */
function lastUserText(messages) {
  for (let i = messages.length - 1; i >= 0; i -= 1) {
    if (messages[i].role !== 'user') continue
    const text = userText(messages[i])
    if (text !== '') return text
  }
  return ''
}

/** The lane's own bootstrap prompts, unchanged from the default mock endpoint. */
function bootstrap(messages) {
  const serialized = JSON.stringify(messages)
  if (!serialized.includes('"name":"uml_ir"')) return { tool: { name: 'uml_ir', input: {} } }
  if (!serialized.includes('Render the sample diagram')) return { text: 'Generated the sample project IR.' }
  if (!serialized.includes('"name":"uml_display"')) return { tool: { name: 'uml_display', input: { path: 'diagram.edn' } } }
  return { text: 'The isolated mock model displayed the generated IR. No real model was called.' }
}

/** Tool calls issued after the latest user message, and the results they got. */
function since(messages, text) {
  const index = messages.findIndex(message => message.role === 'user' && userText(message) === text)
  const tail = messages.slice(index)
  const calls = tail.flatMap(message => (message.content ?? []).filter(part => part.type === 'tool_use'))
  const done = name => calls.some(part => part.name === name)
  const results = (name) => {
    const ids = calls.filter(part => part.name === name).map(part => part.id)
    return tail.flatMap(message => (message.content ?? [])
      .filter(part => part.type === 'tool_result' && ids.includes(part.tool_use_id))
      .flatMap(part => typeof part.content === 'string' ? [part.content] : (part.content ?? []).map(block => block.text ?? '')))
  }
  return { done, results }
}

/**
 * One valid mutation naming its target, then a quality run the configuration
 * refuses: the refusal is a readable reason, and it is what the row must show.
 * The reply rewrites the scratch project's configuration synchronously, before
 * the mock endpoint answers, exactly like a companion editing the policy.
 */
function qualityCompanion() {
  let root
  const prompt = 'Check the quality tool results'
  return {
    async prepare(projects) {
      await qualityFixture(projects)
      root = projects.deep
    },
    reply(request) {
      if (!request.tools?.length) return { text: 'Isolated UML session' }
      const messages = request.messages ?? []
      if (lastUserText(messages) !== prompt) return bootstrap(messages)
      const turn = since(messages, prompt)
      if (!turn.done('uml_mutate')) return { tool: { name: 'uml_mutate', input: { targets: ['src/core/order.ts'] } } }
      if (!turn.done('uml_crap')) {
        // A configuration that no longer declares every operation: the dispatcher
        // refuses before running anything, with this reason.
        fsSync.writeFileSync(path.join(root, 'uml-quality.json'), JSON.stringify({ version: 1, operations: {
          crap: { argv: [process.execPath, path.join(root, 'quality-fixture.mjs'), 'crap'], forwardArgs: 'none', validResultExitCodes: [0], scope: 'whole-tree' },
        } }))
        return { tool: { name: 'uml_crap', input: { targets: ['src/core/price.ts'] } } }
      }
      return { text: 'Checked the quality tool results.' }
    },
  }
}

/**
 * The lane bootstrap, then a companion that answers one panel request with
 * quality → IR → display. The panel request names `:core`, so the display it
 * answers carries that element.
 */
function displayReply(request) {
  if (!request.tools?.length) return { text: 'Isolated UML session' }
  const messages = request.messages ?? []
  const text = lastUserText(messages)
  if (!text.startsWith('UML viewer panel request:')) return bootstrap(messages)
  const op = /:op :(refresh-[a-z-]+)/.exec(text)?.[1]
  const turn = since(messages, text)
  if (!turn.done('uml_crap')) return { tool: { name: 'uml_crap', input: {} } }
  if (!turn.results('uml_crap').some(result => /"validResult"\s*:\s*true/.test(result))) {
    return { text: `Stopped ${op}: quality returned validResult false; no IR or display.` }
  }
  if (!turn.done('uml_ir')) return { tool: { name: 'uml_ir', input: {} } }
  if (!turn.done('uml_display')) return { tool: { name: 'uml_display', input: { path: 'diagram.edn' } } }
  return { text: `Finished ${op} for :core: quality, IR, display.` }
}

/**
 * A finished turn keeps its steps collapsed, as the host's own tool cards do;
 * the tool rows are inside. Expanding the latest turn is how a user reads them.
 */
async function expandSteps(page) {
  const steps = page.getByRole('button', { name: /^(Completed in |已完成)/ }).last()
  await expect(steps).toBeVisible()
  await steps.click()
  await expect(steps).toHaveAttribute('aria-expanded', 'true')
  // The turn keeps its tool calls in one more group; the rows are inside it.
  const tools = page.getByRole('button', { name: /^(Called tools|工具调用)/ }).last()
  await expect(tools).toBeVisible()
  await tools.click()
  await expect(tools).toHaveAttribute('aria-expanded', 'true')
}

/** The element menu's quality entry, from the element's own box on the canvas. */
async function clickRefresh(panel, frame, id, label) {
  const item = frame.world.find(item => item.id === id && (item.kind === 'class' || item.kind === 'package'))
  expect(item, `frame contains ${id}`).toBeTruthy()
  const [x, y] = toScreen(frame, item.rect.x + item.rect.w / 2, item.rect.y + 10)
  await panel.canvas.click({ position: { x, y }, button: 'right' })
  await panel.panel.getByRole('menu', { name: 'Element requests' }).getByRole('menuitem', { name: label, exact: true }).click()
}

/** Settings → General → Language, the way a user switches the DSH language. */
async function switchLanguage(page, label) {
  await page.getByRole('button', { name: /^(Account menu|账号菜单)$/ }).first().click()
  await page.locator('[role^="menuitem"]').filter({ hasText: /^(Settings|设置)/ }).first().click()
  await page.getByText(/^(General|通用设置)$/).first().click()
  const row = page.getByText(/^(Language|语言)$/).first().locator('xpath=ancestor::*[.//button[@aria-haspopup="menu"]][1]')
  await row.locator('button[aria-haspopup="menu"]').click()
  await page.locator('[role^="menuitem"]').filter({ hasText: new RegExp(`^${label}$`) }).first().click()
  await page.keyboard.press('Escape')
}

test('quality tool results show validity, reason and targets, in the DSH language (#54)', async ({ browser }) => {
  const companion = qualityCompanion()
  const lane = await launchViewerLane(browser, { prepareProjects: companion.prepare, reply: companion.reply })
  try {
    const deep = await lane.openPanel('deep')
    await lane.sendPrompt(deep, 'Check the quality tool results')
    await expect(deep.page.getByText('Checked the quality tool results.', { exact: true }).last()).toBeVisible()
    await expandSteps(deep.page)
    await expect(deep.page.getByText('src/core/order.ts', { exact: true }).last()).toBeVisible()
    await expect(deep.page.getByText('src/core/price.ts', { exact: true }).last()).toBeVisible()
    await expect(deep.page.getByText('operations must declare crap and mutate', { exact: true }).last()).toBeVisible()
    await expect(deep.page.getByText('Invalid', { exact: true }).last()).toBeVisible()
    await expect(deep.page.getByText('Valid', { exact: true }).last()).toBeVisible()
    await expect(deep.page.getByText('Exit code', { exact: true }).last()).toBeVisible()

    // The rows read in the host's language without a reload.
    await switchLanguage(deep.page, '中文')
    await expect(deep.page.getByText('有效', { exact: true }).last()).toBeVisible()
    await expect(deep.page.getByText('无效', { exact: true }).last()).toBeVisible()
    await expect(deep.page.getByText('原因', { exact: true }).last()).toBeVisible()
    await expect(deep.page.getByText('退出码', { exact: true }).last()).toBeVisible()
    await expect(deep.page.getByText('operations must declare crap and mutate', { exact: true }).last()).toBeVisible()
    expect(deep.problems).toEqual([])
  } finally {
    await lane.close()
  }
})

test('uml_display shows the diagram and 「在面板中查看」 opens the panel on the element (#54)', async ({ browser }) => {
  const lane = await launchViewerLane(browser, { prepareProjects: qualityFixture, reply: displayReply })
  try {
    const deep = await lane.openPanel('deep')
    const panel = () => deep.page.locator('.dsh-uml-viewer-panel').last()
    const summary = () => panel().locator('.dsh-uml-viewer-summary')

    // A display that answered no panel request opens the tab alone.
    await expandSteps(deep.page)
    await closeTab(deep)
    await expect(deep.page.locator('.dsh-uml-viewer-panel')).toBeHidden()
    await deep.page.getByRole('button', { name: 'View in panel', exact: true }).last().click()
    await expect(panel()).toBeVisible()
    await expect(summary()).toHaveAttribute('data-kind', 'frame')
    await expect(summary()).toHaveAttribute('data-selected', '')

    // A refresh asked for on one class: the display that answers it names that
    // class, so the conversation can open the panel on it. `:core` is a
    // namespace; its classes are one level in.
    const root = await producerFrame(deep, { root: lane.projects.deep, home: lane.home })
    const core = root.world.find(item => item.id === ':core' && (item.kind === 'class' || item.kind === 'package'))
    expect(core, 'the deep root shows the core component').toBeTruthy()
    await lane.point(deep, toScreen(root, core.rect.x + core.rect.w / 2, core.rect.y + core.rect.h / 2), 2)
    await expect(deep.summary).toHaveAttribute('data-depth', 'drilled', { timeout: 30_000 })
    const frame = await producerFrame(deep, { root: lane.projects.deep, home: lane.home, focus: ':core' })
    expect(frame.world.some(item => item.kind === 'class' && item.id === ':core.order')).toBe(true)
    await clickRefresh(deep, frame, ':core.order', 'Refresh CRAP')
    await expect(deep.page.getByText('Finished refresh-crap for :core: quality, IR, display.', { exact: true }).last())
      .toBeVisible()
    await expandSteps(deep.page)
    await expect(deep.page.getByText('Diagram: diagram.edn', { exact: true }).last()).toBeVisible()

    await closeTab(deep)
    await expect(deep.page.locator('.dsh-uml-viewer-panel')).toBeHidden()
    await deep.page.getByRole('button', { name: 'View in panel', exact: true }).last().click()
    await expect(summary()).toHaveAttribute('data-kind', 'frame')
    await expect(summary()).toHaveAttribute('data-selected', ':core.order')
    expect(deep.problems).toEqual([])
    await lane.screenshot(deep, 't54-view-in-panel')
  } finally {
    await lane.close()
  }
})
