import { test } from 'node:test'
import assert from 'node:assert/strict'
import { UPGRADE_PROMPTS, managementResult, upgradeReply } from './upgrade-reply.mjs'

const MAIN = 'D:\\a\\dsh-uml-viewer\\dsh-uml-viewer/.scratch/upgrade-packages/dsh-uml-viewer-1.0.1.tgz'
const tools = [{ name: 'plugin_manager' }, { name: 'uml_display' }]
const user = text => ({ role: 'user', content: [{ type: 'text', text }] })
const use = (id, input) => ({ role: 'assistant', content: [{ type: 'tool_use', id, name: 'plugin_manager', input }] })
// The pinned tool returns JSON.stringify(result) wrapped in a text block, as CI11 recorded.
const result = (id, value) => ({ role: 'user', content: [{ type: 'tool_result', tool_use_id: id,
  content: [{ type: 'text', text: JSON.stringify(value) }], is_error: false }] })
const system = { role: 'system', content: [{ type: 'text', text: 'You are an AI agent powered by DeepSeek Harness.' }] }

const removed = { stage: 'remove', target: 'dsh-uml-viewer', changed: true, application: 'applied', warnings: [] }
const installed = { stage: 'enable', target: 'dsh-uml-viewer', bundle: 'dsh-uml-viewer', version: '1.0.1',
  changed: true, application: 'applied', warnings: [], registries: [null] }

test('reads the manager result out of the recorded tool_result text block', () => {
  assert.deepEqual(managementResult(result('t', removed).content[0]), removed)
  assert.equal(managementResult({ type: 'tool_result', content: [{ type: 'text', text: 'not json' }] }), null)
  assert.equal(managementResult(undefined), null)
})

test('removal prompt lists, removes, then reports the real removal application', () => {
  const observed = {}
  const reply = upgradeReply(observed, { main: MAIN })
  const start = [user('Open the shop sample project'), user(UPGRADE_PROMPTS.remove)]
  assert.equal(reply({ messages: start }), null, 'tool-less requests (titles) stay with the default mock')
  assert.deepEqual(reply({ tools, messages: start }),
    { tool: { name: 'plugin_manager', input: { action: 'list_bundles', limit: 100 } } })
  const listed = [...start, use('l1', { action: 'list_bundles', limit: 100 }), result('l1', { entries: [], total: 0 })]
  assert.deepEqual(reply({ tools, messages: listed }),
    { tool: { name: 'plugin_manager', input: { action: 'remove_bundle', target: 'dsh-uml-viewer' } } })
  const done = [...listed, use('r1', { action: 'remove_bundle', target: 'dsh-uml-viewer' }), result('r1', removed), system]
  assert.deepEqual(reply({ tools, messages: done }), { text: 'Isolated removal applied' })
  assert.deepEqual(observed.removal, removed)
  assert.equal(observed.install, undefined)
})

test('a removal that waits for a restart is reported as not applied', () => {
  const observed = {}
  const reply = upgradeReply(observed, { main: MAIN })
  const pending = { ...removed, application: 'restart-required', restartReason: 'package-moved' }
  const messages = [user(UPGRADE_PROMPTS.remove), use('l1', { action: 'list_bundles', limit: 100 }), result('l1', {}),
    use('r1', { action: 'remove_bundle', target: 'dsh-uml-viewer' }), result('r1', pending)]
  assert.deepEqual(reply({ tools, messages }), { text: 'Isolated removal did not apply' })
  assert.equal(observed.removal.application, 'restart-required')
})

test('install prompt calls public install_bundle with the absolute main tarball and reports its own result', () => {
  const observed = {}
  const reply = upgradeReply(observed, { main: MAIN })
  // The earlier removal turn stays in the transcript; only the latest prompt's tail counts.
  const earlier = [user(UPGRADE_PROMPTS.remove), use('l1', { action: 'list_bundles', limit: 100 }), result('l1', {}),
    use('r1', { action: 'remove_bundle', target: 'dsh-uml-viewer' }), result('r1', removed),
    { role: 'assistant', content: [{ type: 'text', text: 'Isolated removal applied' }] }]
  const start = [...earlier, user(UPGRADE_PROMPTS.install)]
  assert.deepEqual(reply({ tools, messages: start }),
    { tool: { name: 'plugin_manager', input: { action: 'install_bundle', target: MAIN } } })
  const done = [...start, use('i1', { action: 'install_bundle', target: MAIN }), result('i1', installed), system]
  assert.deepEqual(reply({ tools, messages: done }), { text: 'Isolated install applied' })
  assert.deepEqual(observed.install, installed)
})

test('an install that is not applied, or answers another tool call, is not reported as applied', () => {
  for (const outcome of [
    { ...installed, application: 'failed', error: { code: 'ambiguous-install' } },
    { ...installed, application: 'restart-required', restartReason: 'package-moved' },
  ]) {
    const observed = {}
    const reply = upgradeReply(observed, { main: MAIN })
    const messages = [user(UPGRADE_PROMPTS.install), use('i1', { action: 'install_bundle', target: MAIN }), result('i1', outcome)]
    assert.deepEqual(reply({ tools, messages }), { text: 'Isolated install did not apply' })
    assert.equal(observed.install.application, outcome.application)
  }
  const observed = {}
  const reply = upgradeReply(observed, { main: MAIN })
  const messages = [user(UPGRADE_PROMPTS.install), use('i1', { action: 'install_bundle', target: MAIN }),
    result('other', installed)]
  assert.deepEqual(reply({ tools, messages }), { text: 'Isolated install did not apply' })
  assert.equal(observed.install, null)
})

test('requests outside the upgrade prompts, or without the public tool, are not scripted as upgrades', () => {
  const reply = upgradeReply({}, { main: MAIN })
  assert.equal(reply({ tools, messages: [user('Render the sample diagram')] }), null)
  assert.deepEqual(reply({ tools: [{ name: 'uml_display' }], messages: [user(UPGRADE_PROMPTS.install)] }),
    { text: 'Missing public plugin_manager tool' })
  assert.throws(() => upgradeReply({}, { main: 'relative/main.tgz' }), /absolute/)
})
