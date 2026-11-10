// A real child process speaking the resident protocol's line framing. The test
// controls it through resident.json in the session cwd; it never reads a project.
import { appendFileSync, readFileSync, writeFileSync } from 'node:fs'

const config = JSON.parse(readFileSync('resident.json', 'utf8'))
writeFileSync('resident-invocation.json', JSON.stringify({ cwd: process.cwd(), argv: process.argv.slice(2) }))
const frame = JSON.parse(readFileSync(config.framePath, 'utf8'))
const sourceContext = { kind: 'real', edn: '{:context :real}' }
let discussionContext = config.context ?? sourceContext
const waitingFrame = {
  ...frame,
  waiting: true,
  world: [],
  edges: [],
  // Chrome left the frame with #46: the waiting canvas is empty, its status is
  // the state event's code.
  chrome: [],
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
    'select', 'search', 'locate', 'context', 'declutter', 'save', 'quit',
    'inspector', 'card', 'open-card', 'close-card', 'scroll-card', 'open-source', 'quality-target'],
})

function exit(reason) {
  writeFileSync('resident-exit.json', JSON.stringify({ reason }))
}

function handle(action) {
  appendFileSync('resident-actions.log', `${JSON.stringify(action)}\n`)
  switch (action.action) {
    case 'open':
      discussionContext = action.state?.context ?? sourceContext
      return answer(action.seq, { event: 'frame', frame: config.openWaiting === false ? frame : waitingFrame })
    case 'reload':
      return config.displayError
        ? answer(action.seq, { event: 'error', message: config.displayError })
        : answer(action.seq, { event: 'frame', frame: config.reloadWaiting === true ? waitingFrame : frame })
    case 'display':
      return config.displayError
        ? answer(action.seq, { event: 'error', message: config.displayError })
        : answer(action.seq, { event: 'frame', frame })
    case 'context': {
      const proposalId = action.proposalId
      if (proposalId && (config.unknownProposals ?? []).includes(proposalId)) {
        return answer(action.seq, { event: 'error', message: `unknown proposal ${proposalId}` })
      }
      discussionContext = proposalId
        ? { kind: 'proposal', proposalId, name: proposalId, edn: `{:context :proposal :proposal-id ${proposalId}}` }
        : sourceContext
      answer(action.seq, { event: 'context', context: discussionContext })
      return answer(action.seq, { event: 'frame', frame })
    }
    case 'save':
      return answer(action.seq, {
        event: 'state',
        // #45: the tree path and the localizable canvas status.
        breadcrumbs: [{ id: null, name: 'Shop' }, { id: ':core', name: 'core' }],
        status: { code: 'waiting-display' },
        state: {
          schema: 'dsh-uml-viewer/session-state', version: 1, diagram: config.diagram ?? null,
          context: discussionContext, focus: [':core'],
          proposalId: discussionContext.kind === 'proposal' ? discussionContext.proposalId : null,
          openLayer: null, declutter: ':triangles', camera: { x: 4, y: 5, zoom: 0.75 }, selected: null, detail: null,
        },
      })
    case 'search':
      return answer(action.seq, { event: 'search', query: action.query, results: [{ id: ':core', name: 'core', ns: 'core', label: 'core' }] })
    // #27 overlays: the test supplies each model through resident.json.
    case 'inspector':
      return answer(action.seq, { event: 'inspector', inspector: config.inspector })
    case 'card':
      return config.cardError
        ? answer(action.seq, { event: 'error', message: config.cardError })
        : answer(action.seq, { event: 'card', card: config.card })
    case 'open-source':
      return config.sourceError
        ? answer(action.seq, { event: 'error', message: config.sourceError })
        : answer(action.seq, { event: 'source', source: config.source })
    // #28: the test supplies one entry per element id. A raw target is
    // answered as it stands (so a malformed one reaches the host validator);
    // `error` is a refusal, as the real resident's is.
    case 'quality-target': {
      const entry = (config.qualityTargets ?? {})[action.id]
      if (!entry) {
        return answer(action.seq, { event: 'error',
          message: `no class or namespace component ${action.id} in the shown diagram` })
      }
      if (entry.error) return answer(action.seq, { event: 'error', message: entry.error })
      return answer(action.seq, { event: 'quality-target',
        target: { id: entry.id ?? action.id, kind: entry.kind, files: entry.files } })
    }
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
