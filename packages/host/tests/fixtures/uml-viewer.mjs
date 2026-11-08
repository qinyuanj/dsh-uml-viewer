// A real child process; its observable argv/cwd and output are the CLI seam.
import { readFileSync, writeFileSync } from 'node:fs'
const args = process.argv.slice(2)
const config = JSON.parse(readFileSync('fake-binary.json', 'utf8'))
writeFileSync('invocation.json', JSON.stringify({ cwd: process.cwd(), args }))
if (config.delay) await new Promise(resolve => setTimeout(resolve, config.delay))
if (config.exitCode === 0) writeFileSync('diagram.edn', '{:classes []}\n')
process.stdout.write(config.stdout ?? 'Wrote diagram.edn\n')
process.stderr.write(config.stderr ?? '')
process.exitCode = config.exitCode
