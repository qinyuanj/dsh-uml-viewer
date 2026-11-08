import { spawnSync } from 'node:child_process'
import { createRequire } from 'node:module'
import path from 'node:path'
import { BROWSERS } from './dsh-web.mjs'

// The CLI does not read playwright.config.mjs for `install`, so set the scratch
// prefix explicitly here as well as in the config used by the test runner.
const require = createRequire(import.meta.url)
const cli = require.resolve('@playwright/test/cli')
const result = spawnSync(process.execPath, [cli, 'install', 'chromium'], {
  stdio: 'inherit',
  env: { ...process.env, PLAYWRIGHT_BROWSERS_PATH: BROWSERS },
})
if (result.error) throw result.error
process.exitCode = result.status ?? 1
