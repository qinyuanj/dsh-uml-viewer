// A real quality child process for the Cordis seam. It records observable
// starts/ends and argv, then prints the one-line version-1 record the real
// binary would print. Configuration lives in quality-binary.json.
import { appendFileSync, existsSync, readFileSync } from 'node:fs'

const argv = process.argv.slice(2)
if (argv[0] !== 'quality') {
  await import('./uml-viewer.mjs')
} else {
  await quality()
}

async function quality() {
const config = existsSync('quality-binary.json')
  ? JSON.parse(readFileSync('quality-binary.json', 'utf8'))
  : {}
const operation = argv[1] ?? 'crap'
const log = event => appendFileSync('quality-events.jsonl',
  `${JSON.stringify({ event, argv, cwd: process.cwd(), pid: process.pid })}\n`)
const tool = operation === 'crap' ? 'crapper' : operation === 'mutate' ? 'mutator' : 'dryer'
const exitCode = config.exitCode ?? 0
const record = {
  version: 1,
  operation,
  projectRoot: config.projectRoot ?? process.cwd(),
  exitCode,
  validResult: config.validResult ?? exitCode === 0,
  crapScope: operation === 'crap' ? (config.crapScope ?? 'whole-tree') : null,
  configured: config.configured ?? false,
  tool: config.tool ?? tool,
  reason: config.reason ?? null,
}

log('start')
if (config.delay) await new Promise(resolve => setTimeout(resolve, config.delay))
if (config.signal) {
  log('end')
  process.kill(process.pid, config.signal)
  await new Promise(() => {})
}
process.stdout.write(config.stdout ?? `${JSON.stringify(record)}\n`)
process.stderr.write(config.stderr ?? '')
process.exitCode = exitCode
log('end')
}
