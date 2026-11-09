import fs from 'node:fs'
import path from 'node:path'
import assert from 'node:assert/strict'
import { editViewPolicy, VIEW_POLICY } from './view-policy.mjs'

/** Deterministic test companion, not evidence of real LLM instruction compliance.
 * The lane supplies one owned scratch project via prepare; messages cannot select paths.
 * Synchronous edits complete before the mock endpoint can return uml_ir.
 */
export function makeViewCompanion() {
  let policy
  let context = 'real'
  const applied = new Map()
  const edits = []
  const contexts = []
  return {
    edits, contexts,
    prepare(projects) {
      if (policy) throw new Error('fixture already prepared')
      policy = path.join(projects.deep, 'policy.edn')
      fs.writeFileSync(policy, VIEW_POLICY)
    },
    reply(request) {
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
      const tail = messages.slice(index)
      const done = name => tail.some(message => (message.content ?? []).some(part => part.type === 'tool_use' && part.name === name))
      const op = /:op :([a-z-]+)/.exec(text)?.[1]
      if (op === 'context') {
        const real = text === 'UML viewer panel request:\n{:op :context :context {:context :real}}'
        const proposal = /^UML viewer panel request:\n\{:op :context :context \{:context :proposal :proposal-id (:p-review|:p-e2e-\d+) :name "([^"\n]+)"\}\}$/.exec(text)
        if (!real && !proposal) return { text: 'Refused unrecognized fixture context.' }
        if (proposal && !fs.readFileSync(policy, 'utf8').includes(`:id ${proposal[1]} :name "${proposal[2]}"`)) return { text: 'Refused unknown fixture proposal.' }
        context = real ? 'real' : proposal[1].slice(1)
        contexts.push(context)
        return { text: 'Discussing the chosen diagram.' }
      }
      if (op === 'refresh-crap') {
        if (!done('uml_crap')) return { tool: { name: 'uml_crap', input: {} } }
        return { text: 'Stopped refresh-crap: quality returned validResult false; no IR or display.' }
      }
      if (!['omit', 'new-proposal'].includes(op)) return { text: 'Refused unrecognized fixture request.' }
      const id = /:request "([0-9a-f]{32})"/.exec(text)?.[1]
      if (!done('uml_ir')) {
        try {
          if (!policy) throw new Error('fixture not prepared')
          if (applied.has(id)) {
            if (applied.get(id) !== text) throw new Error('request id reused for another edit')
          } else {
            const original = fs.readFileSync(policy, 'utf8')
            const edit = editViewPolicy(original, text, context)
            if (edit.context !== context) throw new Error('omit differs from discussion context')
            // Check actual UTF-8 ranges, including comments/CJK, rather than serializing EDN.
            const prefixBytes = Buffer.byteLength(original.slice(0, edit.start))
            assert.deepEqual(Buffer.from(edit.text).subarray(0, prefixBytes), Buffer.from(original).subarray(0, prefixBytes))
            assert.deepEqual(Buffer.from(edit.text).subarray(prefixBytes + Buffer.byteLength(edit.after)),
              Buffer.from(original).subarray(prefixBytes + Buffer.byteLength(edit.before)))
            fs.writeFileSync(policy, edit.text)
            edits.push({ ...edit, original, message: text })
            applied.set(edit.request, text)
          }
        } catch (error) { return { text: `Refused fixture edit: ${error.message}` } }
        return { tool: { name: 'uml_ir', input: { request: id } } }
      }
      // Read tool result text structurally; escaped transcript substring matching misses failures.
      const succeeded = name => {
        const ids = tail.flatMap(message => message.content ?? [])
          .filter(part => part.type === 'tool_use' && part.name === name).map(part => part.id)
        const results = tail.flatMap(message => message.content ?? [])
          .filter(part => part.type === 'tool_result' && ids.includes(part.tool_use_id))
          .flatMap(part => typeof part.content === 'string' ? [part.content] : (part.content ?? []).map(block => block.text ?? ''))
        return results.some(result => /"validResult"\s*:\s*true/.test(result))
          && !results.some(result => /"validResult"\s*:\s*false/.test(result))
      }
      if (!succeeded('uml_ir')) return { text: `Stopped ${op}: IR failed; no display.` }
      if (!done('uml_display')) return { tool: { name: 'uml_display', input: { path: 'diagram.edn' } } }
      if (!succeeded('uml_display')) return { text: `Stopped ${op}: display failed.` }
      return { text: `Finished ${op}: IR, display.` }
    },
  }
}
