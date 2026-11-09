import { test } from 'node:test'
import assert from 'node:assert/strict'
import { editViewPolicy, VIEW_POLICY } from './view-policy.mjs'
import { makeViewCompanion } from './view-companion.mjs'
import fs from 'node:fs'
import path from 'node:path'

function modelRequest(text, tail = []) {
  return { tools: [{}], messages: [{ role: 'user', content: [{ type: 'text', text }] }, ...tail] }
}

test('companion edits its prepared scratch policy once before IR and stops after failed IR', () => {
  const parent = path.resolve(import.meta.dirname, '../../.scratch')
  fs.mkdirSync(parent, { recursive: true })
  const deep = fs.mkdtempSync(path.join(parent, 't29-unit-'))
  try {
    const companion = makeViewCompanion()
    companion.prepare({ deep })
    const message = omit('{:context :real}')
    const reply = companion.reply(modelRequest(message))
    assert.deepEqual(reply, { tool: { name: 'uml_ir', input: { request } } })
    const expected = VIEW_POLICY.replace('[  ] ; source-only', '[ :api ] ; source-only')
    assert.equal(fs.readFileSync(path.join(deep, 'policy.edn'), 'utf8'), expected)
    assert.deepEqual(companion.reply(modelRequest(message)), reply)
    assert.equal(companion.edits.length, 1)
    assert.equal(fs.readFileSync(path.join(deep, 'policy.edn'), 'utf8'), expected)
    const failed = [
      { role: 'assistant', content: [{ type: 'tool_use', id: 'ir-1', name: 'uml_ir', input: { request } }] },
      { role: 'user', content: [{ type: 'tool_result', tool_use_id: 'ir-1', content: [{ type: 'text', text: JSON.stringify({ validResult: false }) }] }] },
    ]
    assert.equal(companion.reply(modelRequest(message, failed)).text, 'Stopped omit: IR failed; no display.')
    assert.equal(companion.reply(modelRequest(message, failed.slice(0, 1))).text, 'Stopped omit: IR failed; no display.')
    const succeeded = structuredClone(failed)
    succeeded[1].content[0].content[0].text = '{"validResult":true}'
    assert.deepEqual(companion.reply(modelRequest(message, succeeded)), { tool: { name: 'uml_display', input: { path: 'diagram.edn' } } })
    const display = [
      ...succeeded,
      { role: 'assistant', content: [{ type: 'tool_use', id: 'display-1', name: 'uml_display' }] },
      { role: 'user', content: [{ type: 'tool_result', tool_use_id: 'display-1', content: '{"validResult":false}' }] },
    ]
    assert.equal(companion.reply(modelRequest(message, display)).text, 'Stopped omit: display failed.')
    display.at(-1).content[0].content = '{"validResult":true}'
    assert.equal(companion.reply(modelRequest(message, display)).text, 'Finished omit: IR, display.')
    assert.match(companion.reply(modelRequest('UML viewer panel request:\n{:op :surprise}')).text, /^Refused/)
    assert.equal(companion.edits.length, 1)
    assert.equal(fs.readFileSync(path.join(deep, 'policy.edn'), 'utf8'), expected)
  } finally { fs.rmSync(deep, { recursive: true, force: true }) }
})

test('companion refuses a mismatched omit context and keeps the chosen context through proposal creation', () => {
  const parent = path.resolve(import.meta.dirname, '../../.scratch')
  fs.mkdirSync(parent, { recursive: true })
  const deep = fs.mkdtempSync(path.join(parent, 't29-context-'))
  try {
    const companion = makeViewCompanion()
    companion.prepare({ deep })
    const proposalOmit = omit('{:context :proposal :name "Review plan" :proposal-id :p-review}')
    assert.match(companion.reply(modelRequest(proposalOmit)).text, /^Refused/)
    assert.equal(fs.readFileSync(path.join(deep, 'policy.edn'), 'utf8'), VIEW_POLICY)
    // CI7 run-GWPnzU request 006: the resident quotes name before proposal-id.
    const choice = 'UML viewer panel request:\n{:op :context :context {:context :proposal :name "Review plan" :proposal-id :p-review}}'
    assert.equal(companion.reply(modelRequest(choice)).text, 'Discussing the chosen diagram.')
    assert.equal(fs.readFileSync(path.join(deep, 'policy.edn'), 'utf8'), VIEW_POLICY)
    assert.equal(companion.reply(modelRequest(proposalOmit)).tool.name, 'uml_ir')
    const name = '2026-11-09 17:50:21'
    const create = `UML viewer panel request:\n{:op :new-proposal :name "${name}" :request "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"}`
    assert.equal(companion.reply(modelRequest(create)).tool.name, 'uml_ir')
    assert.equal(companion.edits[1].context, 'p-review')
    assert.equal(companion.edits[1].proposalId, ':p-e2e-1')
    assert.equal(companion.reply(modelRequest(create)).tool.name, 'uml_ir')
    assert.equal(companion.edits.length, 2)
    assert.match(companion.reply(modelRequest(create.replace('aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa', request))).text, /^Refused/)
  } finally { fs.rmSync(deep, { recursive: true, force: true }) }
})

test('proposal omit leaves source omit and neighboring proposal layout untouched', () => {
  const result = editViewPolicy(VIEW_POLICY, omit('{:context :proposal :name "Review plan" :proposal-id :p-review}'))
  assert.equal(result.text, VIEW_POLICY.replace(':omit [ ]} ; proposal-only', ':omit [:api]} ; proposal-only'))
  assert.equal(result.context, 'p-review')
})

test('new named and timestamp proposals splice one empty group with a fresh id', () => {
  for (const name of ['Architecture review', '2026-11-09 17:50:21']) {
    const message = `UML viewer panel request:\n{:op :new-proposal :name "${name}" :request "${request}"}`
    const result = editViewPolicy(VIEW_POLICY, message)
    assert.equal(result.text, VIEW_POLICY.replace('   ; New proposals', `   {:id :p-e2e-1 :name "${name}" :layers []}\n   ; New proposals`))
    assert.equal(result.proposalId, ':p-e2e-1')
    assert.equal(result.context, 'real')
  }
})

test('unknown messages, paths and invalid timestamp names refuse without a policy result', () => {
  for (const message of ['please hide api', omit('{:context :proposal :proposal-id :unknown :name "Other"}'),
    ...['../policy.edn', 'bad/name', '2026-02-30 12:00:00', '2026-11-09 25:00:00', ''].map(name =>
      `UML viewer panel request:\n{:op :new-proposal :name "${name}" :request "${request}"}`)]) {
    assert.throws(() => editViewPolicy(VIEW_POLICY, message))
  }
})

const request = '0123456789abcdef0123456789abcdef'
const omit = context => `UML viewer panel request:\n{:op :omit :target {:id :api :kind :class} :context ${context} :request "${request}"}`

test('source omit changes only its literal vector, preserving comments, order and layout bytes', () => {
  const result = editViewPolicy(VIEW_POLICY, omit('{:context :real}'))
  const expected = VIEW_POLICY.replace(' :omit [  ] ; source-only', ' :omit [ :api ] ; source-only')
  assert.equal(result.text, expected)
  const start = Buffer.byteLength(VIEW_POLICY.slice(0, VIEW_POLICY.indexOf('[  ]')))
  assert.deepEqual(Buffer.from(result.text).subarray(0, start), Buffer.from(VIEW_POLICY).subarray(0, start))
  assert.deepEqual(Buffer.from(result.text).subarray(start + 8), Buffer.from(VIEW_POLICY).subarray(start + 4))
})
