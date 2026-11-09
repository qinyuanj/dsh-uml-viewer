import assert from 'node:assert/strict'
import { mkdtemp, mkdir, writeFile, readFile, rm, chmod } from 'node:fs/promises'
import { createHash } from 'node:crypto'
import { spawnSync } from 'node:child_process'
import path from 'node:path'
import { test } from 'node:test'
import { platforms, mainPlatform, tagVersion, releaseTag, stageArchives, verifyInventory } from './release.mjs'
import { releaseVersion } from './version.mjs'

const root = path.resolve(import.meta.dirname, '..')
const sha = bytes => createHash('sha256').update(bytes).digest('hex')

async function scratch() {
  return mkdtemp(path.join(root, '.scratch-release-test-'))
}

// A controlled assembler output, read by the real stage command. These are
// fixture bytes, not built native archives; they exercise the release guards.
async function assembled(directory, { version, platform, corrupt = false }) {
  const packageDir = path.join(directory, `assembled-${platform}`)
  await mkdir(packageDir, { recursive: true })
  const mainName = `dsh-uml-viewer-${version}.tgz`
  const platformName = `dsh-uml-viewer-${platform}-${version}.tgz`
  const main = Buffer.from(`controlled main archive ${version}\n`)
  const binary = Buffer.from(`controlled ${platform} archive ${version}\n`)
  await writeFile(path.join(packageDir, mainName), main)
  await writeFile(path.join(packageDir, platformName), binary)
  await writeFile(path.join(packageDir, 'package-report.json'), JSON.stringify({
    version, platform,
    optionalDependencies: Object.fromEntries(platforms.map(entry => [`@dsh-uml-viewer/${entry.platform}`, version])),
    main: { path: path.join(packageDir, mainName), sha256: sha(main), files: ['README.md', 'package.json'] },
    binary: {
      path: path.join(packageDir, platformName),
      sha256: corrupt ? sha(Buffer.from('other')) : sha(binary),
      files: [`bin/uml-viewer${platform === 'win32-x64' ? '.exe' : ''}`, 'package.json'],
    },
  }, null, 2) + '\n')
  return packageDir
}

// Staged upload trees for all four runners, optionally mutated to reproduce a
// release-guard violation.
async function inventoryFixture(directory, options = {}) {
  const version = options.version ?? await releaseVersion()
  const artifactsDir = path.join(directory, 'artifacts')
  const mainName = `dsh-uml-viewer-${version}.tgz`
  const main = Buffer.from(`controlled main archive ${version}\n`)
  const mainHash = sha(main)
  const optionalDependencies = Object.fromEntries(platforms.map(entry => [`@dsh-uml-viewer/${entry.platform}`, version]))
  if (options.optionalShift) optionalDependencies['@dsh-uml-viewer/linux-x64'] = options.optionalShift
  for (const entry of platforms) {
    if (entry.platform === options.omit) continue
    const upload = path.join(artifactsDir, `package-${entry.platform}`)
    await mkdir(upload, { recursive: true })
    const platformName = options.stalePlatform === entry.platform
      ? `dsh-uml-viewer-${entry.platform}-0.9.0.tgz`
      : `dsh-uml-viewer-${entry.platform}-${version}.tgz`
    const binary = Buffer.from(`controlled ${entry.platform} archive ${version}\n`)
    await writeFile(path.join(upload, platformName), binary)
    const stagesMain = entry.platform === (options.mainRunner ?? mainPlatform) || entry.platform === options.secondMain
    const record = {
      platform: entry.platform, runner: entry.runner, version,
      optionalDependencies,
      artifacts: {
        main: { file: mainName, sha256: mainHash },
        platform: {
          file: platformName,
          sha256: entry.platform === options.tampered ? sha(Buffer.from('tampered bytes')) : sha(binary),
        },
      },
      inventory: {
        main: options.divergentInventory === entry.platform
          ? ['README.md', 'icon.svg', 'package.json']
          : ['README.md', 'package.json'],
        platform: options.extraPlatformFile === entry.platform
          ? ['bin/uml-viewer', 'package.json', 'extra.txt']
          : [`bin/uml-viewer${entry.os === 'win32' ? '.exe' : ''}`, 'package.json'],
      },
      staged: stagesMain ? ['main', 'platform'] : ['platform'],
    }
    if (stagesMain) await writeFile(path.join(upload, mainName), main)
    await writeFile(path.join(upload, 'assemble-report.json'), JSON.stringify(record, null, 2) + '\n')
  }
  if (options.extraArchive) await writeFile(path.join(artifactsDir, options.extraArchive), 'stray\n')
  return artifactsDir
}

const workflow = () => readFile(path.join(root, '.github/workflows/package-release.yml'), 'utf8')

function jobBlock(text, name) {
  const lines = text.split(/\r?\n/)
  const start = lines.findIndex(line => new RegExp(`^  ${name}:\\s*$`).test(line))
  assert.notEqual(start, -1, `workflow job ${name} is missing`)
  let end = lines.length
  for (let index = start + 1; index < lines.length; index += 1) {
    if (/^ {2}\S/.test(lines[index])) { end = index; break }
  }
  return lines.slice(start, end).join('\n')
}

// The shell body of one workflow step, so the real publish loop can be executed
// against a stubbed `npm` instead of being asserted as text.
function stepScript(text, stepName) {
  const lines = text.split(/\r?\n/)
  const start = lines.findIndex(line => line.trim() === `- name: ${stepName}`)
  assert.notEqual(start, -1, `workflow step ${stepName} is missing`)
  const runIndex = lines.findIndex((line, index) => index > start && /^\s+run: \|\s*$/.test(line))
  assert.notEqual(runIndex, -1, `workflow step ${stepName} has no run script`)
  const indent = lines[runIndex].match(/^\s*/)[0].length
  const body = []
  for (let index = runIndex + 1; index < lines.length; index += 1) {
    const line = lines[index]
    if (line.trim() !== '' && line.match(/^\s*/)[0].length <= indent) break
    body.push(line)
  }
  return body.join('\n')
}

// The workflow shell needs a bash that shares this host's paths and environment:
// a Windows `bash` on PATH may be WSL's, which cannot see `$RUNNER_TEMP` here.
// Those hosts skip loudly instead of reporting a false regression.
function hostShell() {
  const probe = spawnSync('bash', ['-c', 'printf %s "$UML_VIEWER_SHELL_PROBE"'], {
    encoding: 'utf8', env: { ...process.env, UML_VIEWER_SHELL_PROBE: 'ready' },
  })
  if (probe.error || probe.status !== 0 || probe.stdout !== 'ready') return null
  return 'bash'
}

// Runs a workflow shell step with a stub `npm` that records its arguments, so a
// publish never reaches a registry.
async function runStep(directory, script) {
  const bin = path.join(directory, 'stub-bin')
  await mkdir(bin, { recursive: true })
  const log = path.join(directory, 'npm-calls.txt')
  const stub = [
    '#!/bin/sh',
    '{ printf \'%s\' "$1"; shift; for arg in "$@"; do printf \'\\t%s\' "$arg"; done; printf \'\\n\'; } >> "$NPM_LOG"',
    '',
  ].join('\n')
  await writeFile(path.join(bin, 'npm'), stub)
  await chmod(path.join(bin, 'npm'), 0o755)
  const result = spawnSync('bash', ['-euo', 'pipefail', '-c', script], {
    cwd: root, encoding: 'utf8',
    env: {
      ...process.env, RUNNER_TEMP: directory, NPM_LOG: log,
      PATH: `${bin}${path.delimiter}${process.env.PATH}`,
    },
  })
  let calls = []
  try {
    calls = (await readFile(log, 'utf8')).trim().split(/\r?\n/).filter(Boolean)
      .map(line => { const [command, ...args] = line.split('\t'); return { command, args } })
  } catch { /* the step produced no npm call */ }
  return { result, calls }
}

test('each delivered platform is assembled on a runner of the same OS and CPU', async () => {
  assert.deepEqual(platforms.map(entry => entry.platform), ['win32-x64', 'linux-x64', 'darwin-arm64', 'darwin-x64'])
  assert.equal(mainPlatform, 'linux-x64')
  for (const entry of platforms) {
    assert.equal(entry.platform, `${entry.os}-${entry.cpu}`)
    assert.ok(entry.runner.startsWith({ win32: 'windows-', linux: 'ubuntu-', darwin: 'macos-' }[entry.os]), entry.runner)
    // macos-latest is arm64, so the Intel build needs an explicit -intel label.
    if (entry.os === 'darwin') assert.equal(entry.runner.endsWith('-intel'), entry.cpu === 'x64')
    assert.equal(entry.main, entry.platform === mainPlatform)
  }
})

test('the workflow matrix is exactly the four release platforms on native runners', async () => {
  const text = await workflow()
  const entries = [...text.matchAll(/-\s+platform:\s*(\S+)\s*\r?\n\s+runner:\s*(\S+)\s*\r?\n\s+binary:\s*(\S+)/g)]
    .map(([, platform, runner, binary]) => ({ platform, runner, binary }))
  assert.deepEqual(entries, platforms.map(({ platform, runner, binary }) => ({ platform, runner, binary })))
  // The main archive is staged by one runner only, named in one place.
  assert.equal((text.match(new RegExp(`matrix\\.platform == '${mainPlatform}'`, 'g')) ?? []).length, 1)
  assert.equal((text.match(/--main/g) ?? []).length, 1)
})

test('the tagged publish job is the only holder of an OIDC token and a release environment', async () => {
  const text = await workflow()
  const publish = jobBlock(text, 'publish')
  assert.equal((text.match(/id-token:/g) ?? []).length, 1)
  assert.match(publish, /id-token:\s*write/)
  assert.doesNotMatch(jobBlock(text, 'assemble'), /id-token|permissions:/)
  assert.doesNotMatch(jobBlock(text, 'inventory'), /id-token/)
  const permissions = /permissions:\r?\n((?: {6}\S[^\n]*\r?\n)+)/.exec(publish)?.[1]
  assert.ok(permissions, 'the publish job declares its permissions explicitly')
  assert.deepEqual(permissions.trim().split(/\r?\n/).map(line => line.trim()), ['contents: read', 'id-token: write'])
  assert.doesNotMatch(jobBlock(text, 'assemble'), /:\s*write/)
  assert.doesNotMatch(jobBlock(text, 'inventory'), /:\s*write/)
  assert.match(publish, /environment:\s*release/)
  assert.match(publish, /startsWith\(github\.ref, 'refs\/tags\/v'\)/)
  assert.match(publish, /github\.repository == 'qinyuanj\/dsh-uml-viewer'/)
  assert.doesNotMatch(text, /secrets\.|NODE_AUTH_TOKEN|_authToken|npm_token/)
})

test('pull requests stop at packaging and an npm publish dry run', async () => {
  const text = await workflow()
  const inventory = jobBlock(text, 'inventory')
  const publish = jobBlock(text, 'publish')
  assert.match(inventory, /if: github\.event_name == 'pull_request'/)
  assert.match(inventory, /npm publish "\$archive" --dry-run/)
  assert.doesNotMatch(publish, /--dry-run/)
  assert.match(publish, /npm publish "\$archive" --provenance/)
  // The private source package is never the publish target; only staged
  // archives are, and every publish names one.
  const publishes = text.split(/\r?\n/).filter(line => /^\s*npm publish\b/.test(line))
  assert.equal(publishes.length, 2)
  for (const line of publishes) assert.match(line, /npm publish "\$archive"/)
  assert.match(text, /node scripts\/version\.mjs --check/)
})

test('the publish job guards the tag, verifies the inventory, and publishes platforms first', async () => {
  const text = await workflow()
  const publish = jobBlock(text, 'publish')
  const order = ['check-tag "$GITHUB_REF_NAME"', 'release.mjs verify', 'release.mjs plan', 'npm publish "$archive"']
  const positions = order.map(marker => publish.indexOf(marker))
  for (const [index, position] of positions.entries()) {
    assert.notEqual(position, -1, `publish job is missing ${order[index]}`)
    if (index > 0) assert.ok(position > positions[index - 1], `${order[index]} must follow ${order[index - 1]}`)
  }
  // A shorter plan must fail instead of silently publishing fewer archives.
  assert.match(publish, /wc -l < "\$RUNNER_TEMP\/publish-order\.txt"/)
  assert.match(publish, /test "\$planned" -eq 5/)
  assert.match(publish, /while IFS= read -r archive; do/)
})

test('the publish step really publishes the five staged archives in plan order', async t => {
  if (hostShell() === null) {
    t.skip("no bash shares this host's paths and environment, so the workflow shell cannot be executed here")
    return
  }
  const directory = await scratch()
  try {
    const version = await releaseVersion()
    await inventoryFixture(directory)
    const script = stepScript(await workflow(), 'Publish the platform packages, then the main package')
    const { result, calls } = await runStep(directory, script)
    assert.equal(result.status, 0, result.stderr)
    assert.equal(calls.length, 5)
    for (const call of calls) {
      assert.equal(call.command, 'publish')
      assert.deepEqual(call.args.slice(1), ['--provenance', '--access', 'public', '--ignore-scripts'])
    }
    const archives = path.join(directory, 'artifacts')
    assert.deepEqual(calls.map(call => call.args[0]), [
      ...platforms.map(entry => path.join(archives, `package-${entry.platform}`, `dsh-uml-viewer-${entry.platform}-${version}.tgz`)),
      path.join(archives, `package-${mainPlatform}`, `dsh-uml-viewer-${version}.tgz`),
    ])
  } finally { await rm(directory, { recursive: true, force: true }) }
})

test('the pull-request step only dry-runs the same plan order', async t => {
  if (hostShell() === null) {
    t.skip("no bash shares this host's paths and environment, so the workflow shell cannot be executed here")
    return
  }
  const directory = await scratch()
  try {
    await inventoryFixture(directory)
    const script = stepScript(await workflow(), 'npm publish dry run')
    const { result, calls } = await runStep(directory, script)
    assert.equal(result.status, 0, result.stderr)
    assert.equal(calls.length, 5)
    for (const call of calls) {
      assert.equal(call.command, 'publish')
      assert.deepEqual(call.args.slice(1), ['--dry-run', '--access', 'public', '--ignore-scripts', '--provenance=false'])
    }
    assert.match(calls.at(-1).args[0], new RegExp(`dsh-uml-viewer-\\d+\\.\\d+\\.\\d+\\.tgz$`))
  } finally { await rm(directory, { recursive: true, force: true }) }
})

test('a short publish plan stops the workflow before any publish', async t => {
  if (hostShell() === null) {
    t.skip("no bash shares this host's paths and environment, so the workflow shell cannot be executed here")
    return
  }
  const directory = await scratch()
  try {
    await inventoryFixture(directory)
    // A plan with four lines must fail instead of publishing four archives.
    const version = await releaseVersion()
    const artifacts = path.join(directory, 'artifacts')
    const plan = path.join(directory, 'publish-order.txt')
    await writeFile(plan, [
      path.join(artifacts, 'package-win32-x64', `dsh-uml-viewer-win32-x64-${version}.tgz`),
      path.join(artifacts, 'package-linux-x64', `dsh-uml-viewer-linux-x64-${version}.tgz`),
      path.join(artifacts, 'package-darwin-arm64', `dsh-uml-viewer-darwin-arm64-${version}.tgz`),
      path.join(artifacts, 'package-darwin-x64', `dsh-uml-viewer-darwin-x64-${version}.tgz`),
    ].join('\n') + '\n')
    const script = stepScript(await workflow(), 'Publish the platform packages, then the main package')
      .replace(/node scripts\/release\.mjs plan[^\n]*\n/, '')
    const { result, calls } = await runStep(directory, script)
    assert.notEqual(result.status, 0)
    assert.deepEqual(calls, [])
  } finally { await rm(directory, { recursive: true, force: true }) }
})

test('a release tag must be v<workspace version>', async () => {
  const version = await releaseVersion()
  assert.equal(tagVersion('v1.0.0'), '1.0.0')
  assert.equal(tagVersion('v2.3.4-rc.1'), '2.3.4-rc.1')
  for (const bad of ['1.0.0', 'v1.0', 'v01.0.0', 'release-1.0.0', '', ' v1.0.0']) {
    assert.throws(() => tagVersion(bad), /v<workspace version>/, String(bad))
  }
  assert.equal(releaseTag(`v${version}`, version), version)
  assert.throws(() => releaseTag('v0.9.0', version), new RegExp(`0\\.9\\.0.*${version.replaceAll('.', '\\.')}`))

  const accepted = spawnSync(process.execPath, ['scripts/release.mjs', 'check-tag', `v${version}`], { cwd: root, encoding: 'utf8' })
  assert.equal(accepted.status, 0, accepted.stdout + accepted.stderr)
  const refused = spawnSync(process.execPath, ['scripts/release.mjs', 'check-tag', `v${version}-stale`], { cwd: root, encoding: 'utf8' })
  assert.notEqual(refused.status, 0)
  assert.match(refused.stderr, /does not match Cargo workspace version/)
})

test('stage uploads the current platform archive and the main archive only on the main runner', async () => {
  const directory = await scratch()
  try {
    const version = await releaseVersion()
    const packageDir = await assembled(directory, { version, platform: 'linux-x64' })
    const platformOnly = await stageArchives({ packageDir, platform: 'linux-x64', uploadDir: path.join(directory, 'linux') })
    assert.deepEqual(platformOnly.staged, ['platform'])
    assert.deepEqual(platformOnly.inventory, { main: ['README.md', 'package.json'], platform: ['bin/uml-viewer', 'package.json'] })
    assert.equal(platformOnly.artifacts.main.sha256, sha(Buffer.from(`controlled main archive ${version}\n`)))
    const upload = path.join(directory, 'linux')
    assert.match((await readFile(path.join(upload, 'assemble-report.json'), 'utf8')), /linux-x64/)

    const withMain = await stageArchives({ packageDir, platform: 'linux-x64', uploadDir: path.join(directory, 'mains'), main: true })
    assert.deepEqual(withMain.staged, ['main', 'platform'])
    for (const file of [`dsh-uml-viewer-${version}.tgz`, `dsh-uml-viewer-linux-x64-${version}.tgz`]) {
      await readFile(path.join(directory, 'mains', file))
    }
  } finally { await rm(directory, { recursive: true, force: true }) }
})

test('stage refuses a foreign main runner, an unknown platform and a stale assembler report', async () => {
  const directory = await scratch()
  try {
    const version = await releaseVersion()
    const arm = await assembled(directory, { version, platform: 'darwin-arm64' })
    await assert.rejects(
      stageArchives({ packageDir: arm, platform: 'darwin-arm64', uploadDir: path.join(directory, 'arm'), main: true }),
      /main archive must be staged on linux-x64/)
    await assert.rejects(
      stageArchives({ packageDir: arm, platform: 'plan9-x64', uploadDir: path.join(directory, 'plan9') }),
      /Unknown platform plan9-x64/)
    await assert.rejects(
      stageArchives({ packageDir: arm, platform: 'linux-x64', uploadDir: path.join(directory, 'mismatch') }),
      /Assembled report is for darwin-arm64, not linux-x64/)
    const corrupt = await assembled(directory, { version, platform: 'win32-x64', corrupt: true })
    await assert.rejects(
      stageArchives({ packageDir: corrupt, platform: 'win32-x64', uploadDir: path.join(directory, 'corrupt') }),
      /does not match the assembler report hash/)
  } finally { await rm(directory, { recursive: true, force: true }) }
})

test('the staged inventory of four platforms and one main archive verifies and plans platforms first', async () => {
  const directory = await scratch()
  try {
    const artifactsDir = await inventoryFixture(directory)
    const inventory = await verifyInventory({ artifactsDir })
    assert.deepEqual(inventory.artifacts.map(artifact => artifact.platform), [...platforms.map(entry => entry.platform), null])
    assert.deepEqual(inventory.publishOrder, inventory.artifacts)
    assert.equal(inventory.reproducibleMain.runners, 4)
    assert.deepEqual(inventory.reproducibleMain.files, ['README.md', 'package.json'])
    // Every runner's main hash is evidence; only the staged one is published.
    assert.deepEqual(inventory.mainHashes.map(item => item.platform), platforms.map(entry => entry.platform))
    for (const item of inventory.mainHashes) assert.match(item.sha256, /^[0-9a-f]{64}$/)
    for (const artifact of inventory.artifacts) {
      assert.equal(artifact.sha256, sha(await readFile(artifact.path)))
      assert.equal(artifact.file, path.basename(artifact.path))
    }
    assert.equal(inventory.artifacts.at(-1).file, `dsh-uml-viewer-${inventory.version}.tgz`)

    const cli = spawnSync(process.execPath, ['scripts/release.mjs', 'plan', '--artifacts', artifactsDir], { cwd: root, encoding: 'utf8' })
    assert.equal(cli.status, 0, cli.stderr)
    const planned = cli.stdout.trim().split(/\r?\n/)
    assert.deepEqual(planned, inventory.publishOrder.map(artifact => artifact.path))
  } finally { await rm(directory, { recursive: true, force: true }) }
})

for (const [name, options, message] of [
  ['a missing platform archive', { omit: 'darwin-x64' }, /assemble reports, found 3/],
  ['two runners staging the main archive', { secondMain: 'win32-x64' }, /Exactly one runner must stage the main archive, found 2/],
  ['a main archive staged on the wrong runner', { mainRunner: 'darwin-arm64' }, /main archive must be staged on linux-x64/],
  ['a main archive with different contents on one runner', { divergentInventory: 'darwin-x64' }, /Main archive inventory differs between runners: darwin-x64/],
  ['a platform archive that bundles an extra file', { extraPlatformFile: 'win32-x64' }, /platform archive inventory/],
  ['a platform archive whose bytes do not match the report', { tampered: 'win32-x64' }, /recorded sha256 does not match the staged bytes/],
  ['a platform archive named for another version', { stalePlatform: 'linux-x64' }, /platform archive dsh-uml-viewer-linux-x64-0\.9\.0\.tgz is not/],
  ['a stray fifth archive', { extraArchive: 'dsh-uml-viewer-src-1.0.0.tgz' }, /Expected exactly 5 release archives, found 6/],
  ['optional dependencies that drift from the workspace version', { optionalShift: '0.9.0' }, /optional dependencies must pin exactly/],
  ['a report from another release version', { version: '9.9.9' }, /platform package version 9\.9\.9 != workspace version/],
]) {
  test(`the release inventory refuses ${name}`, async () => {
    const directory = await scratch()
    try {
      const artifactsDir = await inventoryFixture(directory, options)
      await assert.rejects(verifyInventory({ artifactsDir }), message)
    } finally { await rm(directory, { recursive: true, force: true }) }
  })
}

test('stage and verify run end to end through the command line, as CI calls them', async () => {
  const directory = await scratch()
  try {
    const version = await releaseVersion()
    const artifactsDir = path.join(directory, 'artifacts')
    for (const entry of platforms) {
      const packageDir = await assembled(directory, { version, platform: entry.platform })
      const uploadDir = path.join(artifactsDir, `package-${entry.platform}`)
      const args = ['scripts/release.mjs', 'stage', '--package-dir', packageDir, '--platform', entry.platform, '--upload-dir', uploadDir]
      const result = spawnSync(process.execPath, entry.main ? [...args, '--main'] : args, { cwd: root, encoding: 'utf8' })
      assert.equal(result.status, 0, result.stderr)
    }
    const out = path.join(directory, 'release-inventory.json')
    const verified = spawnSync(process.execPath, ['scripts/release.mjs', 'verify', '--artifacts', artifactsDir, '--out', out], { cwd: root, encoding: 'utf8' })
    assert.equal(verified.status, 0, verified.stderr)
    const inventory = JSON.parse(await readFile(out, 'utf8'))
    assert.equal(inventory.version, version)
    assert.equal(inventory.artifacts.length, 5)
    assert.match(inventory.reproducibleMain.sha256, /^[0-9a-f]{64}$/)

    const orderFile = path.join(directory, 'publish-order.txt')
    const planned = spawnSync(process.execPath, ['scripts/release.mjs', 'plan', '--artifacts', artifactsDir, '--out', orderFile], { cwd: root, encoding: 'utf8' })
    assert.equal(planned.status, 0, planned.stderr)
    const lines = (await readFile(orderFile, 'utf8')).trim().split(/\r?\n/)
    assert.equal(lines.length, 5)
    assert.match(lines[0], /dsh-uml-viewer-win32-x64-/)
    assert.match(lines.at(-1), /[\\/]dsh-uml-viewer-\d+\.\d+\.\d+\.tgz$/)
  } finally { await rm(directory, { recursive: true, force: true }) }
})

test('test:package runs the stale-payload guard and the release guards', async () => {
  const manifest = JSON.parse(await readFile(path.join(root, 'package.json'), 'utf8'))
  assert.equal(manifest.scripts['test:package'], 'node scripts/test-package.mjs')
  // The source checkout stays private; only assembled archives are published.
  assert.equal(manifest.private, true)
  const runner = await readFile(path.join(root, 'scripts/test-package.mjs'), 'utf8')
  for (const suite of ['scripts/package.test.mjs', 'scripts/package-version.test.mjs', 'scripts/release.test.mjs']) {
    assert.ok(runner.includes(suite), `${suite} must run in test:package`)
  }
  assert.match(runner, /UML_VIEWER_PACKAGE_TEST_SCRATCH/)
})

test('the existing CI keeps the package contract step and its jobs', async () => {
  const text = await readFile(path.join(root, '.github/workflows/ci.yml'), 'utf8')
  for (const job of ['typescript', 'panel-e2e', 'windows-crap', 'windows-mutate', 'macos-quality', 'linux']) {
    assert.match(text, new RegExp(`^  ${job}:`, 'm'), `ci.yml job ${job}`)
  }
  assert.match(text, /pnpm test:package/)
  assert.match(text, /pnpm version:check/)
})
