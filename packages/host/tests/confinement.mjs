// The test confinement provider returns a different argv. Reaching this
// wrapper proves the plugin spawned that argv, rather than the raw binary.
import { spawnSync } from 'node:child_process'
import { writeFileSync } from 'node:fs'
writeFileSync('confined.json', JSON.stringify(process.argv.slice(2)))
const result = spawnSync(process.argv[2], process.argv.slice(3), { stdio: 'inherit' })
process.exitCode = result.status ?? 1
