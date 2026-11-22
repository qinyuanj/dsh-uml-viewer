import { readFileSync, writeFileSync } from 'node:fs'
const argv = process.argv.slice(2)
if (argv[0] === 'resident') {
  await import('./resident.mjs')
} else {
  const config = JSON.parse(readFileSync('fake-binary.json', 'utf8'))
  writeFileSync('invocation.json', JSON.stringify({ cwd: process.cwd(), args: argv }))
  if (config.delay) await new Promise(resolve => setTimeout(resolve, config.delay))
  if (config.exitCode === 0) writeFileSync('diagram.edn', '{:classes []}\n')
  process.stdout.write(config.stdout ?? 'Wrote diagram.edn\n')
  process.stderr.write(config.stderr ?? '')
  process.exitCode = config.exitCode
}
