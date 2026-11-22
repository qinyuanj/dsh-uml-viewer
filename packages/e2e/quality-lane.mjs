import fs from 'node:fs/promises'
import path from 'node:path'

export function qualityReply(request) {
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

export async function qualityFixture(projects) {
  for (const root of Object.values(projects)) {
    await fs.writeFile(path.join(root, 'quality-fixture.mjs'), `
import fs from 'node:fs'
const operation = process.argv[2]
const args = process.argv.slice(3)
fs.appendFileSync('quality-calls.jsonl', JSON.stringify({operation,args})+'\\n')
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

export async function calls(root) {
  try { return (await fs.readFile(path.join(root, 'quality-calls.jsonl'), 'utf8')).trim().split('\n').map(line => JSON.parse(line)) }
  catch (error) { if (error.code === 'ENOENT') return []; throw error }
}
