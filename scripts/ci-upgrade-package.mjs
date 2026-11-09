import fs from 'node:fs/promises'
import path from 'node:path'
import { spawnSync } from 'node:child_process'
import { root, releaseVersion } from './version.mjs'

// This producer deliberately cannot compile locally. Run after ordinary panel
// and package acceptance: the shared target's executable becomes 1.0.1, while
// the already assembled 1.0.0 tarballs remain immutable acceptance inputs.
if (process.env.CI !== 'true' || process.env.GITHUB_ACTIONS !== 'true') {
  throw new Error('The second native version may only be built in GitHub Actions')
}
if (await releaseVersion() !== '1.0.0') throw new Error('Upgrade fixture expects the committed 1.0.0 authority')
const scratch = path.join(root, '.scratch')
await fs.mkdir(scratch, { recursive: true })
const copy = await fs.mkdtemp(path.join(scratch, 'upgrade-source-'))
const output = path.join(scratch, 'upgrade-packages')
function run(file, args, cwd = copy, extraEnv = {}) {
  const result = spawnSync(file, args, { cwd, stdio: 'inherit', env: { ...process.env, ...extraEnv } })
  if (result.error) throw result.error
  if (result.status !== 0 || result.signal) throw new Error(`${file} failed: ${result.signal ?? result.status}`)
}
try {
  const tracked = spawnSync('git', ['ls-files', '-z'], { cwd: root, encoding: 'utf8' })
  if (tracked.error || tracked.status !== 0) throw new Error('Could not enumerate the controlled source copy')
  for (const name of tracked.stdout.split('\0').filter(Boolean)) {
    const to = path.join(copy, name)
    await fs.mkdir(path.dirname(to), { recursive: true })
    await fs.copyFile(path.join(root, name), to)
  }
  const cargo = path.join(copy, 'Cargo.toml')
  const original = await fs.readFile(cargo, 'utf8')
  const changed = original.replace(/(\[workspace\.package\][\s\S]*?^version\s*=\s*)"1\.0\.0"/m, '$1"1.0.1"')
  if (changed === original) throw new Error('Missing workspace version authority')
  await fs.writeFile(cargo, changed)
  run(process.execPath, ['scripts/version.mjs', '--write'])
  // Only esbuild and its installed dependency closure are shared. Each plugin
  // package resolves contracts from this copy, never the original workspace.
  await fs.symlink(path.join(root, 'node_modules'), path.join(copy, 'node_modules'), 'junction')
  for (const pkg of ['host', 'client']) {
    const scope = path.join(copy, 'packages', pkg, 'node_modules', '@dsh-uml-viewer')
    await fs.mkdir(scope, { recursive: true })
    await fs.symlink(path.join(copy, 'packages', 'contracts'), path.join(scope, 'contracts'), 'junction')
  }
  run(process.execPath, ['scripts/build-plugin.mjs'])
  // No --locked here: Cargo must update the disposable copy's lockfile for the
  // new workspace version. The original Cargo.lock is never written.
  const target = path.join(root, 'target')
  // `.cargo/config.toml` already turns debug information and incremental
  // compilation off; CI's CARGO_BUILD_JOBS=4 caps the parallelism.
  run('cargo', ['build', '-p', 'uml-viewer', '-j', '4'], copy, { CARGO_TARGET_DIR: target })
  const binary = path.join(target, 'debug', process.platform === 'win32' ? 'uml-viewer.exe' : 'uml-viewer')
  // The actual producer executes --version and refuses a relabelled old binary.
  run(process.execPath, ['scripts/package.mjs', '--binary', binary, '--out', output])
  const report = JSON.parse(await fs.readFile(path.join(output, 'package-report.json'), 'utf8'))
  if (report.version !== '1.0.1') throw new Error('Second producer returned the wrong version')
  await fs.appendFile(process.env.GITHUB_OUTPUT, `version=${report.version}\n`)
} finally {
  // copy was created by mkdtemp under this exact owned scratch prefix.
  if (path.dirname(copy) !== scratch || !path.basename(copy).startsWith('upgrade-source-')) throw new Error('Unexpected copy cleanup target')
  await fs.rm(copy, { recursive: true, force: true })
}
