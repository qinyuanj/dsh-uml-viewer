// A real child process speaking the resident protocol's line framing. The test
// controls it through resident.json in the session cwd; it never reads a project.
import { appendFileSync, readFileSync, writeFileSync } from 'node:fs'

const config = JSON.parse(readFileSync('resident.json', 'utf8'))
writeFileSync('resident-invocation.json', JSON.stringify({ cwd: process.cwd(), argv: process.argv.slice(2) }))
const frame = JSON.parse(readFileSync(config.framePath, 'utf8'))
const waitingFrame = {
  ...frame,
  waiting: true,
  world: [],
  edges: [],
  chrome: [{ kind: 'hint', text: 'No diagram yet', ink: { role: 'muted' } }],
}

function write(payload) {
  process.stdout.write(`${JSON.stringify(payload)}\n`)
}

function answer(seq, payload) {
  write({ version: 1, seq, ...payload })
}

write({
  version: 1, seq: null, event: 'ready', schema: 'dsh-uml-viewer/resident',
  frameSchema: 'dsh-uml-viewer/scene-frame', frameVersion: 1,
  stateSchema: 'dsh-uml-viewer/session-state', stateVersion: 1,
  actions: ['open', 'viewport', 'camera', 'display', 'reload', 'drill-in', 'back', 'namespace-tree',
    'select', 'search', 'locate', 'context', 'declutter', 'save', 'quit'],
})

function exit(reason) {
  writeFileSync('resident-exit.json', JSON.stringify({ reason }))
}

function handle(action) {
  appendFileSync('resident-actions.log', `${JSON.stringify(action)}\n`)
  switch (action.action) {
    case 'open':
      return answer(action.seq, { event: 'frame', frame: config.openWaiting === false ? frame : waitingFrame })
    case 'reload':
      return config.displayError
        ? answer(action.seq, { event: 'error', message: config.displayError })
        : answer(action.seq, { event: 'frame', frame: config.reloadWaiting === true ? waitingFrame : frame })
    case 'display':
      return config.displayError
        ? answer(action.seq, { event: 'error', message: config.displayError })
        : answer(action.seq, { event: 'frame', frame })
    case 'save':
      return answer(action.seq, {
        event: 'state',
        state: {
          schema: 'dsh-uml-viewer/session-state', version: 1, diagram: config.diagram ?? null,
          context: { kind: 'real', edn: '{:context :real}' }, focus: [':core'], proposalId: null,
          openLayer: null, declutter: ':triangles', camera: { x: 4, y: 5, zoom: 0.75 }, selected: null, detail: null,
        },
      })
    case 'search':
      return answer(action.seq, { event: 'search', query: action.query, results: [{ id: ':core', name: 'core', ns: 'core', label: 'core' }] })
    case 'quit':
      exit('quit')
      answer(action.seq, { event: 'ack' })
      process.exit(0)
      return
    default:
      return answer(action.seq, { event: 'ack' })
  }
}

let buffer = ''
process.stdin.setEncoding('utf8')
process.stdin.on('data', chunk => {
  buffer += chunk
  for (;;) {
    const end = buffer.indexOf('\n')
    if (end < 0) break
    const line = buffer.slice(0, end)
    buffer = buffer.slice(end + 1)
    if (line.trim()) handle(JSON.parse(line))
  }
})
process.stdin.on('end', () => {
  exit('stdin')
  process.exitCode = 0
})
