// Explicit fixture capture, never run by pnpm test and never builds Rust.
// Usage: node capture-overlays.mjs <existing-Windows-absolute-binary> <project>
// The project must have diagram.edn and a :catalog class with member target.
import { execFileSync } from 'node:child_process'
import fs from 'node:fs'
import path from 'node:path'

const [binary, project] = process.argv.slice(2)
if (!binary || !path.isAbsolute(binary) || !project || !path.isAbsolute(project)) {
  throw new Error('Pass an existing absolute binary and project; this script never builds')
}
const actions = [
  { action: 'open' }, { action: 'open-card', id: ':catalog' },
  { action: 'card', width: 624, viewHeight: 700 }, { action: 'open-source', id: ':catalog', member: 'target' },
  { action: 'quit' },
]
const stdout = execFileSync(binary, ['resident', '--project', project], {
  input: actions.map((action, index) => JSON.stringify({ version: 1, seq: index + 1, ...action })).join('\n') + '\n',
  encoding: 'utf8', windowsHide: true,
})
const directory = path.join(import.meta.dirname, 'protocol-samples')
fs.mkdirSync(directory, { recursive: true })
const events = stdout.trim().split(/\r?\n/).map(line => JSON.parse(line))
for (const event of ['card', 'source']) {
  const sample = events.find(value => value.event === event)
  if (!sample) throw new Error(`resident did not produce ${event}: ${stdout}`)
  fs.writeFileSync(path.join(directory, `${event}.json`), JSON.stringify(sample) + '\n')
}
console.log('Captured real card/source event lines without building Rust')
