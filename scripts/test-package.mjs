import { mkdir, mkdtemp, rm } from 'node:fs/promises'
import path from 'node:path'
import { spawnSync } from 'node:child_process'

const root = path.resolve(import.meta.dirname, '..')
const suites = ['scripts/package.test.mjs', 'scripts/package-version.test.mjs', 'scripts/release.test.mjs']
const explicit = process.env.UML_VIEWER_PACKAGE_TEST_SCRATCH
let owned
if (!explicit) {
  await mkdir(path.join(root, '.scratch'), { recursive: true })
  owned = await mkdtemp(path.join(root, '.scratch', 'package-tests-'))
}
const scratch = explicit ?? owned
try {
  const result = spawnSync(process.execPath, ['--test', ...suites], {
    cwd: root, stdio: 'inherit',
    env: { ...process.env, UML_VIEWER_PACKAGE_TEST_SCRATCH: scratch },
  })
  if (result.error) throw result.error
  process.exitCode = result.status ?? 1
} finally {
  if (owned) await rm(owned, { recursive: true, force: true })
}
