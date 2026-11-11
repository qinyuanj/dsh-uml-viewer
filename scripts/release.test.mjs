import assert from 'node:assert/strict'
import { mkdtemp, mkdir, writeFile, readFile, rm, chmod } from 'node:fs/promises'
import { createHash } from 'node:crypto'
import { spawnSync } from 'node:child_process'
import { gunzipSync, gzipSync } from 'node:zlib'
import path from 'node:path'
import { test } from 'node:test'
import { archiveLimits, readArchive, writeArchive } from './archive.mjs'
import { platforms, mainPlatform, tagVersion, releaseTag, stageArchives, verifyInventory } from './release.mjs'
import { releaseVersion } from './version.mjs'

const root = path.resolve(import.meta.dirname, '..')
const sha = bytes => createHash('sha256').update(bytes).digest('hex')

async function scratch() {
  return mkdtemp(path.join(root, '.scratch-release-test-'))
}

// The fixtures are real small gzip/ustar archives built with the production
// writer, so every guard sees the bytes a release would ship. Their payload is
// fixture text, not a built binary, font or plugin bundle: they prove the
// guards, and they are no claim of native package acceptance.
const fixtureMainMembers = {
  'README.md': 'fixture readme for a controlled release archive\n',
  'icon.svg': '<svg xmlns="http://www.w3.org/2000/svg"/>\n',
  'cordis.patch.yml': '- id: fixture\n',
  'dsh-plugin.json': '{"name":"dsh-uml-viewer"}\n',
  'docs/plugin-development.md': 'fixture document\n',
  'lib/host.js': 'export const fixture = "host"\n',
  'lib/client.js': 'export const fixture = "client"\n',
  'locale/en.json': '{}\n',
  'locale/zh.json': '{}\n',
  'fonts/SarasaMonoSC-Regular.ttf': 'fixture font bytes, not the packaged Sarasa face\n',
  'fonts/LICENSE-Sarasa.txt': 'fixture license text, not the packaged license\n',
}

const source = JSON.parse(await readFile(path.join(root, 'package.json'), 'utf8'))
const dependencies = version => Object.fromEntries(platforms.map(entry => [`@dsh-uml-viewer/${entry.platform}`, version]))
const binaryName = os => `bin/uml-viewer${os === 'win32' ? '.exe' : ''}`

// The manifests an assembler run embeds, derived from the source package.json
// exactly as scripts/package.mjs derives them.
function mainManifest(version, overrides = {}) {
  const { scripts, devDependencies, private: developmentOnly, packageManager, ...runtime } = source
  return { ...runtime, version, optionalDependencies: dependencies(version), ...overrides }
}

function platformManifest(version, entry, overrides = {}) {
  return {
    name: `@dsh-uml-viewer/${entry.platform}`, version,
    description: `Native UML viewer for ${entry.platform}`,
    license: source.license, repository: source.repository,
    os: [entry.os], cpu: [entry.cpu], files: ['bin'],
    exports: { './package.json': './package.json' }, ...overrides,
  }
}

function applyMembers(members, changes = {}) {
  for (const [name, value] of Object.entries(changes)) {
    if (value === null) delete members[name]
    else members[name] = value
  }
  return members
}

function mainMembers(version, { manifest, members } = {}) {
  return applyMembers({
    ...fixtureMainMembers,
    'package.json': JSON.stringify(mainManifest(version, manifest), null, 2) + '\n',
  }, members)
}

function platformMembers(version, entry, { manifest, members } = {}) {
  return applyMembers({
    'package.json': JSON.stringify(platformManifest(version, entry, manifest), null, 2) + '\n',
    [binaryName(entry.os)]: `fixture binary payload for ${entry.platform}\n`,
  }, members)
}

const archiveOf = members => writeArchive(new Map(Object.entries(members)
  .map(([name, value]) => [name, Buffer.isBuffer(value) ? value : Buffer.from(value)])))
const inventory = members => Object.keys(members).sort()

// A hostile tar: the production writer cannot emit duplicate, unsafe or linked
// members, so the defects the verifier must refuse are built here. Names and
// header checksums are real tar, so only the intended defect is unusual.
function hostileTar(entries) {
  const blocks = []
  for (const { name, bytes = Buffer.alloc(0), type = '0', mode = 0o644 } of entries) {
    const header = Buffer.alloc(512)
    header.write(name, 0, 100, 'ascii')
    header.write(mode.toString(8).padStart(7, '0') + '\0', 100, 8, 'ascii')
    header.write('0000000\0', 108, 8, 'ascii')
    header.write('0000000\0', 116, 8, 'ascii')
    header.write(bytes.length.toString(8).padStart(11, '0') + '\0', 124, 12, 'ascii')
    header.write('00000000000\0', 136, 12, 'ascii')
    header.fill(32, 148, 156)
    header.write(type, 156, 1, 'ascii')
    header.write('ustar\0', 257, 6, 'ascii')
    header.write('00', 263, 2, 'ascii')
    const sum = header.reduce((total, byte) => total + byte, 0)
    header.write(sum.toString(8).padStart(6, '0') + '\0 ', 148, 8, 'ascii')
    blocks.push(header, bytes, Buffer.alloc((512 - bytes.length % 512) % 512))
  }
  blocks.push(Buffer.alloc(1024))
  return gzipSync(Buffer.concat(blocks), { level: 9 })
}

// Rewrites a real archive's tar bytes while leaving the gzip stream valid.
function retar(bytes, change) {
  const tar = gunzipSync(bytes)
  change(tar)
  return gzipSync(tar, { level: 9 })
}

// A real main archive of this workspace's release version, to corrupt on purpose.
const validMain = version => archiveOf(mainMembers(version))

// Flips one byte, counting from the end for a negative index.
function flip(bytes, index) {
  const copy = Buffer.from(bytes)
  copy[index < 0 ? copy.length + index : index] ^= 0xff
  return copy
}

// A controlled assembler output read by the real `stage` command: real small
// archives whose inventory and hashes are exactly the report's claim.
async function assembled(directory, { version, platform, corrupt = false, main, binary }) {
  const entry = platforms.find(item => item.platform === platform)
  const packageDir = path.join(directory, `assembled-${platform}`)
  await mkdir(packageDir, { recursive: true })
  const mainName = `dsh-uml-viewer-${version}.tgz`
  const platformName = `dsh-uml-viewer-${platform}-${version}.tgz`
  const mainContent = main ?? mainMembers(version)
  const binaryContent = binary ?? platformMembers(version, entry)
  const mainBytes = archiveOf(mainContent)
  const binaryBytes = archiveOf(binaryContent)
  await writeFile(path.join(packageDir, mainName), mainBytes)
  await writeFile(path.join(packageDir, platformName), binaryBytes)
  await writeFile(path.join(packageDir, 'package-report.json'), JSON.stringify({
    version, platform,
    optionalDependencies: dependencies(version),
    main: { path: path.join(packageDir, mainName), sha256: sha(mainBytes), files: inventory(mainContent) },
    binary: {
      path: path.join(packageDir, platformName),
      sha256: corrupt ? sha(Buffer.from('other')) : sha(binaryBytes),
      files: inventory(binaryContent),
    },
  }, null, 2) + '\n')
  return { packageDir, main: inventory(mainContent), platform: inventory(binaryContent) }
}

// Staged upload trees for all four runners, optionally mutated to reproduce a
// release-guard violation. Every archive is a real gzip/ustar archive.
async function inventoryFixture(directory, options = {}) {
  const version = options.version ?? await releaseVersion()
  const artifactsDir = path.join(directory, 'artifacts')
  const mainName = `dsh-uml-viewer-${version}.tgz`
  const mainContent = mainMembers(version, options.main)
  const mainBytes = options.mainBytes ?? archiveOf(mainContent)
  const mainHash = sha(mainBytes)
  const mainList = options.mainReport ?? inventory(mainContent)
  const optionalDependencies = dependencies(version)
  if (options.optionalShift) optionalDependencies['@dsh-uml-viewer/linux-x64'] = options.optionalShift
  for (const entry of platforms) {
    if (entry.platform === options.omit) continue
    const upload = path.join(artifactsDir, `package-${entry.platform}`)
    await mkdir(upload, { recursive: true })
    const platformName = options.stalePlatform === entry.platform
      ? `dsh-uml-viewer-${entry.platform}-0.9.0.tgz`
      : `dsh-uml-viewer-${entry.platform}-${version}.tgz`
    const content = platformMembers(version, entry, options.platform?.[entry.platform])
    const binary = options.platformBytes?.[entry.platform] ?? archiveOf(content)
    await writeFile(path.join(upload, platformName), binary)
    const platformList = options.platformReport?.[entry.platform] ?? inventory(content)
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
          : mainList,
        platform: options.extraPlatformFile === entry.platform
          ? [...platformList, 'extra.txt']
          : platformList,
      },
      staged: stagesMain ? ['main', 'platform'] : ['platform'],
    }
    if (stagesMain) await writeFile(path.join(upload, mainName), mainBytes)
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

// Bounds for an extracted workflow step: a blocking or flooding body must fail
// the suite with a clear message instead of hanging it or exhausting memory.
const stepLimits = Object.freeze({ timeout: 30_000, maxBuffer: 1024 * 1024, killSignal: 'SIGKILL' })

// A failure message that names the bound that was hit, so a timeout or a buffer
// overflow is never reported as a plain assertion mismatch.
function stepFailure(result, limits = stepLimits) {
  if (result.error) return `the workflow step did not complete (${result.error.code ?? 'error'}): ${result.error.message}`
  if (result.signal) return `the workflow step was killed by ${result.signal} after ${limits.timeout} ms`
  return result.stderr || result.stdout || `the workflow step exited with ${result.status}`
}

// Runs a workflow shell step with a stub `npm` that records its arguments, so a
// publish never reaches a registry.
async function runStep(directory, script, limits = stepLimits, { failCall = 0 } = {}) {
  const bin = path.join(directory, 'stub-bin')
  await mkdir(bin, { recursive: true })
  const log = path.join(directory, 'npm-calls.txt')
  const stub = [
    '#!/bin/sh',
    'call=0',
    'if [ -f "$NPM_LOG" ]; then call=$(wc -l < "$NPM_LOG"); fi',
    'call=$((call + 1))',
    '{ printf \'%s\' "$1"; shift; for arg in "$@"; do printf \'\\t%s\' "$arg"; done; printf \'\\n\'; } >> "$NPM_LOG"',
    'if [ "$call" -eq "$NPM_FAIL_CALL" ]; then printf \'npm stub: injected failure on call %s\\n\' "$call" >&2; exit 1; fi',
    '',
  ].join('\n')
  await writeFile(path.join(bin, 'npm'), stub)
  await chmod(path.join(bin, 'npm'), 0o755)
  const result = spawnSync('bash', ['-euo', 'pipefail', '-c', script], {
    cwd: root, encoding: 'utf8',
    timeout: limits.timeout, maxBuffer: limits.maxBuffer, killSignal: limits.killSignal,
    env: {
      ...process.env, RUNNER_TEMP: directory, NPM_LOG: log, NPM_FAIL_CALL: String(failCall),
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

test('pull requests stop at offline package dry runs', async () => {
  const text = await workflow()
  const inventory = jobBlock(text, 'inventory')
  const publish = jobBlock(text, 'publish')
  assert.match(inventory, /if: github\.event_name == 'pull_request'/)
  assert.match(inventory, /npm pack "\$archive" --dry-run --ignore-scripts/)
  assert.doesNotMatch(inventory, /npm publish/)
  assert.match(publish, /npm publish "\$archive" --provenance/)
  // The private source package is never the pack or publish target; only
  // staged archives are, and every npm invocation names one.
  const packs = inventory.split(/\r?\n/).filter(line => /^\s*npm pack\b/.test(line))
  assert.equal(packs.length, 1)
  for (const line of packs) assert.match(line, /npm pack "\$archive"/)
  assert.match(text, /node scripts\/version\.mjs --check/)
})

test('the publish job guards the tag, verifies, dry-runs, then publishes platforms first', async () => {
  const text = await workflow()
  const publish = jobBlock(text, 'publish')
  const order = [
    'check-tag "$GITHUB_REF_NAME"',
    'release.mjs verify',
    'npm publish "$archive" --dry-run --access public --ignore-scripts --provenance=false',
    'npm publish "$archive" --provenance --access public --ignore-scripts',
  ]
  const positions = order.map(marker => publish.indexOf(marker))
  for (const [index, position] of positions.entries()) {
    assert.notEqual(position, -1, `publish job is missing ${order[index]}`)
    if (index > 0) assert.ok(position > positions[index - 1], `${order[index]} must follow ${order[index - 1]}`)
  }
  // A shorter plan must fail instead of silently publishing fewer archives.
  assert.match(publish, /wc -l < "\$RUNNER_TEMP\/publish-order\.txt"/)
  assert.match(publish, /test "\$planned" -eq 5/)
  assert.match(publish, /while IFS= read -r archive; do/)
  assert.doesNotMatch(publish, /--force|retry/i)
})

async function tagPublishScript() {
  const text = await workflow()
  return [
    stepScript(text, 'Dry-run the platform packages, then the main package'),
    stepScript(text, 'Publish the platform packages, then the main package'),
  ].join('\n')
}

test('the tag job dry-runs all five archives before really publishing the same five', async t => {
  if (hostShell() === null) {
    t.skip("no bash shares this host's paths and environment, so the workflow shell cannot be executed here")
    return
  }
  const directory = await scratch()
  try {
    const version = await releaseVersion()
    await inventoryFixture(directory)
    const { result, calls } = await runStep(directory, await tagPublishScript())
    assert.equal(result.status, 0, stepFailure(result, stepLimits))
    assert.equal(calls.length, 10)
    assert.ok(calls.every(call => call.command === 'publish'))
    for (const call of calls.slice(0, 5)) {
      assert.deepEqual(call.args.slice(1), ['--dry-run', '--access', 'public', '--ignore-scripts', '--provenance=false'])
    }
    for (const call of calls.slice(5)) {
      assert.deepEqual(call.args.slice(1), ['--provenance', '--access', 'public', '--ignore-scripts'])
    }
    const archives = path.join(directory, 'artifacts')
    const planned = [
      ...platforms.map(entry => path.join(archives, `package-${entry.platform}`, `dsh-uml-viewer-${entry.platform}-${version}.tgz`)),
      path.join(archives, `package-${mainPlatform}`, `dsh-uml-viewer-${version}.tgz`),
    ]
    assert.deepEqual(calls.map(call => call.args[0]), [...planned, ...planned])
  } finally { await rm(directory, { recursive: true, force: true }) }
})

test('a tag dry-run conflict prevents every real publish', async t => {
  if (hostShell() === null) {
    t.skip("no bash shares this host's paths and environment, so the workflow shell cannot be executed here")
    return
  }
  const directory = await scratch()
  try {
    await inventoryFixture(directory)
    const { result, calls } = await runStep(directory, await tagPublishScript(), stepLimits, { failCall: 3 })
    assert.notEqual(result.status, 0, 'a dry-run conflict must fail the tag job')
    assert.match(result.stderr, /injected failure on call 3/)
    assert.equal(calls.length, 3)
    assert.ok(calls.every(call => call.command === 'publish' && call.args.includes('--dry-run')))
    assert.equal(calls.filter(call => call.args.includes('--provenance')).length, 0)
  } finally { await rm(directory, { recursive: true, force: true }) }
})

test('a real publish conflict stops the remaining publishes', async t => {
  if (hostShell() === null) {
    t.skip("no bash shares this host's paths and environment, so the workflow shell cannot be executed here")
    return
  }
  const directory = await scratch()
  try {
    const version = await releaseVersion()
    await inventoryFixture(directory)
    const { result, calls } = await runStep(directory, await tagPublishScript(), stepLimits, { failCall: 8 })
    assert.notEqual(result.status, 0, 'a real publish conflict must fail the tag job')
    assert.match(result.stderr, /injected failure on call 8/)
    assert.equal(calls.length, 8)
    assert.ok(calls.slice(0, 5).every(call => call.args.includes('--dry-run')))
    const real = calls.slice(5)
    assert.equal(real.length, 3)
    for (const call of real) {
      assert.deepEqual(call.args.slice(1), ['--provenance', '--access', 'public', '--ignore-scripts'])
    }
    assert.match(real.at(-1).args[0], new RegExp(`dsh-uml-viewer-darwin-arm64-${version}\\.tgz$`))
  } finally { await rm(directory, { recursive: true, force: true }) }
})

test('the pull-request step packs all five staged archives in plan order without publishing', async t => {
  if (hostShell() === null) {
    t.skip("no bash shares this host's paths and environment, so the workflow shell cannot be executed here")
    return
  }
  const directory = await scratch()
  try {
    const version = await releaseVersion()
    await inventoryFixture(directory)
    const script = stepScript(await workflow(), 'npm pack dry run')
    const { result, calls } = await runStep(directory, script)
    assert.equal(result.status, 0, stepFailure(result, stepLimits))
    assert.equal(calls.length, 5)
    for (const call of calls) {
      assert.equal(call.command, 'pack')
      assert.deepEqual(call.args.slice(1), ['--dry-run', '--ignore-scripts'])
    }
    const archives = path.join(directory, 'artifacts')
    assert.deepEqual(calls.map(call => call.args[0]), [
      ...platforms.map(entry => path.join(archives, `package-${entry.platform}`, `dsh-uml-viewer-${entry.platform}-${version}.tgz`)),
      path.join(archives, `package-${mainPlatform}`, `dsh-uml-viewer-${version}.tgz`),
    ])
  } finally { await rm(directory, { recursive: true, force: true }) }
})

test('a pull-request pack failure stops before the remaining archives', async t => {
  if (hostShell() === null) {
    t.skip("no bash shares this host's paths and environment, so the workflow shell cannot be executed here")
    return
  }
  const directory = await scratch()
  try {
    const version = await releaseVersion()
    await inventoryFixture(directory)
    const script = stepScript(await workflow(), 'npm pack dry run')
    const { result, calls } = await runStep(directory, script, stepLimits, { failCall: 3 })
    assert.notEqual(result.status, 0, 'a pack failure must fail the pull-request step')
    assert.match(result.stderr, /injected failure on call 3/)
    assert.equal(calls.length, 3)
    assert.ok(calls.every(call => call.command === 'pack'))
    assert.match(calls.at(-1).args[0], new RegExp(`dsh-uml-viewer-darwin-arm64-${version}\\.tgz$`))
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
    assert.notEqual(result.status, 0, 'a four-line plan must fail the step')
    assert.deepEqual(calls, [])
  } finally { await rm(directory, { recursive: true, force: true }) }
})

test('the extracted workflow step runner is bounded in time and output', async t => {
  if (hostShell() === null) {
    t.skip("no bash shares this host's paths and environment, so the workflow shell cannot be executed here")
    return
  }
  // The defaults the real step execution inherits are the bound, not a hint.
  assert.deepEqual({ ...stepLimits }, { timeout: 30_000, maxBuffer: 1024 * 1024, killSignal: 'SIGKILL' })
  const directory = await scratch()
  try {
    const short = { timeout: 750, maxBuffer: 64 * 1024, killSignal: 'SIGKILL' }
    const hung = await runStep(directory, 'sleep 30', short)
    assert.equal(hung.result.error?.code, 'ETIMEDOUT')
    assert.notEqual(hung.result.status, 0)
    assert.match(stepFailure(hung.result, short), /did not complete \(ETIMEDOUT\)/)
    const withinLimit = await runStep(directory, 'printf %20000s x', short)
    assert.equal(withinLimit.result.error, undefined)
    assert.equal(withinLimit.result.status, 0)
    assert.equal(withinLimit.result.stdout.length, 20_000)
    // A finite write can exit 0 before the parent observes ENOBUFS when it fits
    // in the OS pipe. Keep writing so the output bound must stop a live child.
    const flooded = await runStep(directory, 'while :; do printf %20000s x; done', { ...short, timeout: 10_000 })
    assert.equal(flooded.result.error?.code, 'ENOBUFS')
    assert.notEqual(flooded.result.status, 0)
    // spawnSync may retain the read chunk that crosses maxBuffer; it must still
    // stop far below the default 1 MiB bound instead of draining the producer.
    assert.ok(flooded.result.stdout.length >= short.maxBuffer)
    assert.ok(flooded.result.stdout.length < stepLimits.maxBuffer)
    assert.match(stepFailure(flooded.result, short), /did not complete \(ENOBUFS\)/)
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
    const { packageDir, main, platform } = await assembled(directory, { version, platform: 'linux-x64' })
    const platformOnly = await stageArchives({ packageDir, platform: 'linux-x64', uploadDir: path.join(directory, 'linux') })
    assert.deepEqual(platformOnly.staged, ['platform'])
    assert.deepEqual(platformOnly.inventory, { main, platform })
    assert.equal(platformOnly.artifacts.main.sha256, sha(await readFile(path.join(packageDir, `dsh-uml-viewer-${version}.tgz`))))
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
    const { packageDir: arm } = await assembled(directory, { version, platform: 'darwin-arm64' })
    await assert.rejects(
      stageArchives({ packageDir: arm, platform: 'darwin-arm64', uploadDir: path.join(directory, 'arm'), main: true }),
      /main archive must be staged on linux-x64/)
    await assert.rejects(
      stageArchives({ packageDir: arm, platform: 'plan9-x64', uploadDir: path.join(directory, 'plan9') }),
      /Unknown platform plan9-x64/)
    await assert.rejects(
      stageArchives({ packageDir: arm, platform: 'linux-x64', uploadDir: path.join(directory, 'mismatch') }),
      /Assembled report is for darwin-arm64, not linux-x64/)
    const { packageDir: corrupt } = await assembled(directory, { version, platform: 'win32-x64', corrupt: true })
    await assert.rejects(
      stageArchives({ packageDir: corrupt, platform: 'win32-x64', uploadDir: path.join(directory, 'corrupt') }),
      /does not match the assembler report hash/)
  } finally { await rm(directory, { recursive: true, force: true }) }
})

test('a plausible large payload stays inside the default archive limits', async () => {
  // A built native binary is tens of MiB and the packaged Sarasa face is
  // 25.6 MiB, so the caps must accept a realistic archive, not only reject a
  // hostile one. This is the production verify path with the default limits.
  const directory = await scratch()
  try {
    const artifactsDir = await inventoryFixture(directory, {
      main: { members: { 'lib/payload.bin': Buffer.alloc(8 * 1024 * 1024, 7) } },
    })
    const release = await verifyInventory({ artifactsDir })
    assert.equal(release.artifacts.length, 5)
    assert.ok(release.artifacts.at(-1).files.includes('lib/payload.bin'))
  } finally { await rm(directory, { recursive: true, force: true }) }
})

test('the staged inventory of four platforms and one main archive verifies and plans platforms first', async () => {
  const directory = await scratch()
  try {
    const version = await releaseVersion()
    const expectedMain = inventory(mainMembers(version))
    const artifactsDir = await inventoryFixture(directory)
    const release = await verifyInventory({ artifactsDir })
    assert.deepEqual(release.artifacts.map(artifact => artifact.platform), [...platforms.map(entry => entry.platform), null])
    assert.deepEqual(release.publishOrder, release.artifacts)
    assert.equal(release.reproducibleMain.runners, 4)
    assert.deepEqual(release.reproducibleMain.files, expectedMain)
    // Every runner's main hash is evidence; only the staged one is published.
    assert.deepEqual(release.mainHashes.map(item => item.platform), platforms.map(entry => entry.platform))
    for (const item of release.mainHashes) assert.match(item.sha256, /^[0-9a-f]{64}$/)
    for (const artifact of release.artifacts) {
      assert.equal(artifact.sha256, sha(await readFile(artifact.path)))
      assert.equal(artifact.file, path.basename(artifact.path))
      // The recorded list is read out of the real archive, which is what makes
      // the inventory evidence rather than a copy of the assembler report.
      assert.deepEqual(artifact.files, [...await readArchive(artifact.path)].map(([name]) => name.slice('package/'.length)).sort())
    }
    assert.deepEqual(release.artifacts.at(-1).files, expectedMain)
    assert.equal(release.artifacts.at(-1).file, `dsh-uml-viewer-${release.version}.tgz`)

    const cli = spawnSync(process.execPath, ['scripts/release.mjs', 'plan', '--artifacts', artifactsDir], { cwd: root, encoding: 'utf8' })
    assert.equal(cli.status, 0, cli.stderr)
    const planned = cli.stdout.trim().split(/\r?\n/)
    assert.deepEqual(planned, release.publishOrder.map(artifact => artifact.path))
  } finally { await rm(directory, { recursive: true, force: true }) }
})

// Options may be a function of the release version, for a fixture that has to
// corrupt or over-size a real archive of this workspace's version.
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
  // An archive that is not really an archive is refused instead of trusted.
  ['a plaintext main file named .tgz', { mainBytes: Buffer.from('controlled main archive\n') }, /not gzip data/],
  ['a plaintext platform file named .tgz', { platformBytes: { 'linux-x64': Buffer.from('controlled archive\n') } }, /not gzip data/],
  ['a corrupt gzip header', version => ({ mainBytes: flip(validMain(version), 0) }), /not gzip data/],
  ['a corrupt gzip stream', version => ({ mainBytes: flip(validMain(version), 40) }), /gzip data is corrupt/],
  ['a corrupt gzip checksum', version => ({ mainBytes: flip(validMain(version), -6) }), /gzip data is corrupt/],
  ['a corrupt tar header checksum', version => ({ mainBytes: retar(validMain(version), tar => { tar[105] ^= 0x01 }) }), /header checksum .* does not match/],
  ['a duplicate archive entry', { mainBytes: hostileTar([
    { name: 'package/package.json', bytes: Buffer.from('{}') },
    { name: 'package/package.json', bytes: Buffer.from('{}') },
  ]) }, /appears more than once/],
  ['an archive with no package/package.json', { mainBytes: archiveOf(fixtureMainMembers) }, /has no package\/package\.json/],
  ['an archive whose package/package.json is not JSON', {
    mainBytes: archiveOf({ ...fixtureMainMembers, 'package.json': 'not json\n' }),
  }, /package\/package\.json is not valid JSON/],
  ['a path that traverses out of package/', { mainBytes: hostileTar([
    { name: 'package/package.json', bytes: Buffer.from('{}') },
    { name: 'package/../evil.js', bytes: Buffer.from('x') },
  ]) }, /unsafe path segment/],
  ['an entry outside package/', { mainBytes: hostileTar([
    { name: 'other/package.json', bytes: Buffer.from('{}') },
  ]) }, /is outside package\//],
  ['an absolute path entry', { mainBytes: hostileTar([{ name: '/etc/passwd', bytes: Buffer.from('x') }]) }, /absolute path/],
  ['a symbolic link entry', { mainBytes: hostileTar([
    { name: 'package/package.json', bytes: Buffer.from('{}') },
    { name: 'package/bin/uml-viewer', type: '2' },
  ]) }, /is a symbolic link, not a regular file/],
  ['a directory entry', { mainBytes: hostileTar([{ name: 'package/lib', type: '5' }]) }, /is a directory, not a regular file/],
  ['an entry over the size limit', () => ({
    main: { members: { 'lib/huge.bin': Buffer.alloc(64 * 1024) } },
    limits: { ...archiveLimits, entryBytes: 8 * 1024 },
  }), /over the 8192 byte entry limit/],
  ['an archive over the decompressed limit', () => ({
    main: { members: { 'lib/bomb.bin': Buffer.alloc(2 * 1024 * 1024) } },
    limits: { ...archiveLimits, totalBytes: 1024 * 1024 },
  }), /decompressed archive limit/],
  ['an archive over the compressed limit', { limits: { ...archiveLimits, compressedBytes: 64 } }, /compressed archive limit/],
  // The archive's own manifest must agree with the workspace and the platform
  // table, not only with the report that describes it.
  ['a main package name that lies', { main: { manifest: { name: 'not-the-viewer' } } }, /main package name .* is not/],
  ['a main package version that lies', { main: { manifest: { version: '9.9.9' } } }, /main package version .* != workspace version/],
  ['main optional dependencies that lie', {
    main: { manifest: { optionalDependencies: { '@dsh-uml-viewer/linux-x64': '0.9.0' } } },
  }, /main package optional dependencies must pin exactly/],
  ['a private main manifest', { main: { manifest: { private: true } } }, /main package\.json is private/],
  ['a main manifest that still ships scripts', { main: { manifest: { scripts: { install: 'x' } } } }, /must not ship scripts/],
  ['a main package cpu that lies', { main: { manifest: { cpu: ['arm64'] } } }, /main package cpu .* != source/],
  ['a packaged font that is missing', { main: { members: { 'fonts/SarasaMonoSC-Regular.ttf': null } } }, /main archive has no fonts\/SarasaMonoSC-Regular\.ttf/],
  ['a packaged license that is empty', { main: { members: { 'fonts/LICENSE-Sarasa.txt': '' } } }, /main archive fonts\/LICENSE-Sarasa\.txt is empty/],
  ['a file the source package does not publish', { main: { members: { 'src/host.ts': 'export {}\n' } } }, /files list does not publish/],
  ['an invented main inventory', version => ({
    mainReport: [...inventory(mainMembers(version)), 'lib/invented.js'],
  }), /main archive is missing reported lib\/invented\.js/],
  ['an unreported main file', version => ({
    main: { members: { 'lib/extra.js': 'export {}\n' } },
    mainReport: inventory(mainMembers(version)),
  }), /main archive contains unreported lib\/extra\.js/],
  ['a per-file hash in the report that disagrees', version => ({
    mainReport: inventory(mainMembers(version)).map(name => name === 'README.md' ? { name, sha256: sha(Buffer.from('other')) } : name),
  }), /main archive entry README\.md does not match its reported sha256/],
  ['a platform package name that lies', {
    platform: { 'linux-x64': { manifest: { name: '@dsh-uml-viewer/darwin-arm64' } } },
  }, /platform package name .* is not @dsh-uml-viewer\/linux-x64/],
  ['a platform package version that lies', {
    platform: { 'darwin-x64': { manifest: { version: '9.9.9' } } },
  }, /platform package version .* != workspace version/],
  ['a platform package os that lies', {
    platform: { 'darwin-arm64': { manifest: { os: ['linux'] } } },
  }, /platform package os .* is not \["darwin"\] for darwin-arm64/],
  ['a platform package cpu that lies', {
    platform: { 'win32-x64': { manifest: { cpu: ['arm64'] } } },
  }, /platform package cpu .* is not \["x64"\] for win32-x64/],
  ['a platform archive with no binary entry', {
    platform: { 'linux-x64': { members: { 'bin/uml-viewer': null } } },
    platformReport: { 'linux-x64': ['bin/uml-viewer', 'package.json'] },
  }, /platform archive has no bin\/uml-viewer/],
  ['a platform archive with an empty binary entry', {
    platform: { 'linux-x64': { members: { 'bin/uml-viewer': '' } } },
  }, /platform archive bin\/uml-viewer is empty/],
]) {
  test(`the release inventory refuses ${name}`, async () => {
    const directory = await scratch()
    try {
      const version = await releaseVersion()
      const resolved = typeof options === 'function' ? options(version) : options
      const artifactsDir = await inventoryFixture(directory, resolved)
      await assert.rejects(verifyInventory({ artifactsDir, limits: resolved.limits }), message)
    } finally { await rm(directory, { recursive: true, force: true }) }
  })
}

test('stage and verify run end to end through the command line, as CI calls them', async () => {
  const directory = await scratch()
  try {
    const version = await releaseVersion()
    const artifactsDir = path.join(directory, 'artifacts')
    for (const entry of platforms) {
      const { packageDir } = await assembled(directory, { version, platform: entry.platform })
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

test('the verify command fails closed on an archive that only looks like one', async () => {
  const directory = await scratch()
  try {
    const artifactsDir = await inventoryFixture(directory, { mainBytes: Buffer.from('plaintext pretending to be a .tgz\n') })
    const cli = spawnSync(process.execPath, ['scripts/release.mjs', 'verify', '--artifacts', artifactsDir], { cwd: root, encoding: 'utf8' })
    assert.notEqual(cli.status, 0, 'a plaintext file named .tgz must not verify')
    assert.match(cli.stderr, /not gzip data/)
    assert.doesNotMatch(cli.stdout, /publishOrder/)
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
