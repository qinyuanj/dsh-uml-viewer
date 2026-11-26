import assert from 'node:assert/strict'
import { mkdir, mkdtemp, rm, writeFile } from 'node:fs/promises'
import { execFileSync, spawn } from 'node:child_process'
import { createServer } from 'node:http'
import path from 'node:path'
import { test } from 'node:test'
import {
  checkReleaseDrift,
  collectFacts,
  compareReleaseFacts,
  formatDrift,
  formatInProgress,
  newestReleaseTag,
  packageNames,
  parseStatusHeader,
  readRegistryPackage,
  readReleasingStatus,
  registryUrlFor,
  releaseState,
  releasingDocPath,
} from './release-drift.mjs'

const root = path.resolve(import.meta.dirname, '..')
const main = packageNames[0]
const platform = packageNames[1]

function packument(latest, versions) {
  return {
    'dist-tags': { latest },
    versions: Object.fromEntries(versions.map(version => [version, { version }])),
  }
}

// A registry double: it answers with controlled packuments and never touches
// the network.
function registryDouble(documents) {
  return async url => {
    const name = decodeURIComponent(url.slice(url.lastIndexOf('/') + 1))
    const document = documents[name]
    if (!document) return { status: 404, ok: false, json: async () => ({ error: 'not_found' }) }
    return { status: 200, ok: true, json: async () => document }
  }
}

// The Status header restates the published version, so every fixture carries
// one unless the case is about the header itself.
function facts({ repoVersion, tag, latest, versions, docs }) {
  return {
    repoVersion,
    tag: { tag, version: tag.replace(/^v/, '') },
    packages: packageNames.map(name => ({ name, latest, versions, missing: false })),
    docs: docs ?? { checked: '2026-11-26', tag: `v${latest}`, version: latest },
  }
}

function fakeClock() {
  let value = 0
  return { now: () => value, sleep: async ms => { value += ms } }
}

async function scratchRepo(version) {
  const directory = await mkdtemp(path.join(root, '.scratch-drift-test-'))
  await writeFile(path.join(directory, 'package.json'), `${JSON.stringify({ name: 'dsh-uml-viewer', version }, null, 2)}\n`)
  await mkdir(path.join(directory, 'docs'), { recursive: true })
  await writeFile(path.join(directory, 'docs/releasing.md'), `# Release packaging and publishing\n\nStatus (2026-11-26): \`v${version}\` is the latest verified **published** release.\n`)
  const identity = ['-c', 'user.email=fixture@example.com', '-c', 'user.name=fixture']
  const git = (...args) => execFileSync('git', [...identity, ...args], { cwd: directory, stdio: 'ignore' })
  git('init', '-q')
  git('add', '.')
  git('commit', '-q', '-m', 'fixture')
  git('tag', '-a', `v${version}`, '-m', `v${version}`)
  return directory
}

// A fixture Status header for the command tests, so the failure path never
// depends on the network or on the real document.
async function scratchDoc(version, { checked = '2026-11-26' } = {}) {
  const directory = await mkdtemp(path.join(root, '.scratch-drift-doc-'))
  const file = path.join(directory, 'releasing.md')
  await writeFile(file, `# Release packaging and publishing\n\nStatus (${checked}): \`v${version}\` is the latest verified **published** release.\n`)
  return { directory, file }
}

// The command runs as a child process, so the parent event loop must stay
// free to answer the registry requests the child makes.
function runCommand(args) {
  return new Promise(resolve => {
    const child = spawn(process.execPath, args, { cwd: root })
    let stdout = ''
    let stderr = ''
    child.stdout.on('data', chunk => { stdout += chunk })
    child.stderr.on('data', chunk => { stderr += chunk })
    child.on('close', status => resolve({ status, stdout, stderr }))
  })
}

function registryServer(documents) {
  const server = createServer((request, response) => {
    const name = decodeURIComponent(request.url.slice(request.url.lastIndexOf('/') + 1))
    const document = documents[name]
    if (!document) {
      response.statusCode = 404
      response.end('{"error":"not_found"}')
      return
    }
    response.setHeader('content-type', 'application/json')
    response.setHeader('connection', 'close')
    response.end(JSON.stringify(document))
  })
  return new Promise(resolve => {
    server.listen(0, '127.0.0.1', () => resolve({ server, url: `http://127.0.0.1:${server.address().port}` }))
  })
}

test('the three states follow the repository version against the newest tag', () => {
  assert.equal(releaseState({ repoVersion: '1.2.3', tag: { tag: 'v1.2.2', version: '1.2.2' } }), 'in-progress')
  assert.equal(releaseState({ repoVersion: '1.2.2', tag: { tag: 'v1.2.2', version: '1.2.2' } }), 'tagged')
  assert.equal(releaseState({ repoVersion: '1.2.1', tag: { tag: 'v1.2.2', version: '1.2.2' } }), 'behind')
})

test('(a) a repository version newer than the newest tag is a release in progress, not drift', () => {
  const observed = facts({ repoVersion: '1.2.3', tag: 'v1.2.2', latest: '1.2.2', versions: ['1.2.0', '1.2.2'] })
  const comparison = compareReleaseFacts(observed)
  assert.equal(comparison.state, 'in-progress')
  assert.equal(comparison.ok, true)
  assert.deepEqual(comparison.problems, [])
  const message = formatInProgress({ facts: observed, warnings: comparison.warnings })
  assert.match(message, /release in progress: repository package\.json version 1\.2\.3 is newer than the newest release tag v1\.2\.2 \(1\.2\.2\)/)
})

test('(b) a repository version older than the newest tag is drift and fails', () => {
  const observed = facts({ repoVersion: '1.2.1', tag: 'v1.2.2', latest: '1.2.2', versions: ['1.2.0', '1.2.2'] })
  const comparison = compareReleaseFacts(observed)
  assert.equal(comparison.state, 'behind')
  assert.equal(comparison.ok, false)
  assert.ok(comparison.problems.some(problem => /repository package\.json version 1\.2\.1 is older than the newest release tag v1\.2\.2 \(1\.2\.2\): the tree does not carry the tagged version/.test(problem)))
})

test('(c) the tagged state fails when the tag version is missing from the registry', () => {
  const observed = facts({ repoVersion: '1.2.3', tag: 'v1.2.3', latest: '1.2.2', versions: ['1.2.0', '1.2.2'] })
  const comparison = compareReleaseFacts(observed)
  assert.equal(comparison.state, 'tagged')
  assert.equal(comparison.ok, false)
  assert.ok(comparison.problems.some(problem => /npm has no version 1\.2\.3 for dsh-uml-viewer: the tag's version was never published/.test(problem)))
  assert.ok(comparison.problems.some(problem => /npm has no version 1\.2\.3 for @dsh-uml-viewer\/win32-x64: the tag's version was never published/.test(problem)))
})

test('the tagged state fails when the registry is behind the tag', () => {
  const observed = facts({ repoVersion: '1.2.2', tag: 'v1.2.2', latest: '1.2.1', versions: ['1.2.0', '1.2.1'] })
  const comparison = compareReleaseFacts(observed)
  assert.equal(comparison.ok, false)
  assert.ok(comparison.problems.some(problem => /npm latest for dsh-uml-viewer is 1\.2\.1, not the tag version 1\.2\.2 \(the registry is behind the tag\)/.test(problem)))
  assert.ok(comparison.problems.some(problem => /npm has no version 1\.2\.2 for dsh-uml-viewer: the tag's version was never published/.test(problem)))
})

test('the four facts agree in the tagged state', () => {
  const observed = facts({ repoVersion: '1.2.2', tag: 'v1.2.2', latest: '1.2.2', versions: ['1.2.0', '1.2.2'] })
  const comparison = compareReleaseFacts(observed)
  assert.equal(comparison.state, 'tagged')
  assert.deepEqual(comparison.problems, [])
  assert.equal(comparison.ok, true)
})

test('the drift report prints every value and every difference', () => {
  const observed = facts({ repoVersion: '1.2.3', tag: 'v1.2.3', latest: '1.2.2', versions: ['1.2.2'] })
  const report = formatDrift({ attempts: 2, elapsedMs: 15_000, facts: observed, problems: compareReleaseFacts(observed).problems })
  assert.match(report, /release drift detected after 2 attempt/)
  assert.match(report, /npm latest dsh-uml-viewer\s+= 1\.2\.2/)
  assert.match(report, /npm latest @dsh-uml-viewer\/win32-x64\s+= 1\.2\.2/)
  assert.match(report, /repository package\.json version\s+= 1\.2\.3/)
  assert.match(report, /newest release tag\s+= v1\.2\.3 \(1\.2\.3\)/)
  assert.match(report, /docs\/releasing\.md Status header\s+= v1\.2\.2 \(checked 2026-11-26\)/)
  assert.match(report, /differences:/)
  assert.match(report, /the tag's version was never published/)
})

test('the newest release tag is the highest v* tag that carries a version', () => {
  assert.deepEqual(newestReleaseTag(['v1.2.2', 'v1.2.1', 'v1.1.0']), { tag: 'v1.2.2', version: '1.2.2' })
  assert.deepEqual(newestReleaseTag(['v1.2.2-rc.1', 'v1.2.1']), { tag: 'v1.2.2-rc.1', version: '1.2.2-rc.1' })
  assert.throws(() => newestReleaseTag([]), /No v\* release tag found/)
})

test('the registry double answers both lockstep packages', async () => {
  const fetchImpl = registryDouble({
    [main]: packument('1.2.2', ['1.2.0', '1.2.2']),
    [platform]: packument('1.2.2', ['1.2.0', '1.2.2']),
  })
  const entry = await readRegistryPackage(main, { fetchImpl, registry: 'https://registry.test' })
  assert.equal(entry.latest, '1.2.2')
  assert.deepEqual(entry.versions, ['1.2.0', '1.2.2'])
  assert.equal(registryUrlFor(platform, 'https://registry.test'), 'https://registry.test/@dsh-uml-viewer%2fwin32-x64')
  const absent = await readRegistryPackage('dsh-uml-viewer-absent', { fetchImpl, registry: 'https://registry.test' })
  assert.equal(absent.missing, true)
})

test('the collected facts read the manifest, the newest tag, both packages and the Status header', async () => {
  const directory = await scratchRepo('1.2.2')
  try {
    const fetchImpl = registryDouble({
      [main]: packument('1.2.2', ['1.2.0', '1.2.2']),
      [platform]: packument('1.2.2', ['1.2.0', '1.2.2']),
    })
    const observed = await collectFacts({ directory, fetchImpl, registry: 'https://registry.test' })
    assert.equal(observed.repoVersion, '1.2.2')
    assert.deepEqual(observed.tag, { tag: 'v1.2.2', version: '1.2.2' })
    assert.deepEqual(observed.packages.map(entry => entry.name), packageNames)
    assert.deepEqual(observed.docs, { checked: '2026-11-26', tag: 'v1.2.2', version: '1.2.2' })
    assert.equal(compareReleaseFacts(observed).ok, true)
  } finally {
    await rm(directory, { recursive: true, force: true })
  }
})

test('an unreachable registry fails with a clear message instead of a stack trace', async () => {
  const fetchImpl = async () => { throw new Error('getaddrinfo ENOTFOUND registry.npmjs.org') }
  const clock = fakeClock()
  const result = await checkReleaseDrift({
    graceMs: 60_000,
    refreshTags: false,
    now: clock.now,
    sleep: clock.sleep,
    collect: options => readRegistryPackage(main, { ...options, fetchImpl }),
  })
  assert.equal(result.ok, false)
  assert.match(result.error.message, /npm registry is unreachable \(dsh-uml-viewer\): getaddrinfo ENOTFOUND/)
  assert.equal(result.attempts, 3)
})

test('a release that becomes visible inside the grace window is not a false red', async () => {
  const clock = fakeClock()
  let attempts = 0
  const result = await checkReleaseDrift({
    graceMs: 60_000,
    refreshTags: false,
    now: clock.now,
    sleep: clock.sleep,
    collect: async () => {
      attempts += 1
      const versions = attempts === 1 ? ['1.2.1'] : ['1.2.1', '1.2.2']
      return facts({ repoVersion: '1.2.2', tag: 'v1.2.2', latest: attempts === 1 ? '1.2.1' : '1.2.2', versions })
    },
  })
  assert.equal(result.ok, true)
  assert.equal(result.attempts, 2)
})

test('the check gives up after the grace window and reports the last difference', async () => {
  const clock = fakeClock()
  let attempts = 0
  const result = await checkReleaseDrift({
    graceMs: 120_000,
    refreshTags: false,
    now: clock.now,
    sleep: clock.sleep,
    collect: async () => {
      attempts += 1
      return facts({ repoVersion: '1.2.2', tag: 'v1.2.2', latest: '1.2.1', versions: ['1.2.0', '1.2.1'] })
    },
  })
  assert.equal(result.ok, false)
  assert.ok(attempts >= 2)
  assert.ok(result.problems.some(problem => /the registry is behind the tag/.test(problem)))
})

test('the command exits non-zero and prints every value when the registry lags', async () => {
  const { server, url } = await registryServer({
    [main]: packument('1.2.2', ['1.2.0', '1.2.2']),
    [platform]: packument('1.2.2', ['1.2.0', '1.2.2']),
  })
  const doc = await scratchDoc('1.2.3')
  try {
    const result = await runCommand(['scripts/release-drift.mjs', '--registry', url, '--grace-ms', '0', '--tag', 'v1.2.3', '--repo-version', '1.2.3', '--releasing-doc', doc.file])
    assert.equal(result.status, 1, result.stdout + result.stderr)
    assert.match(result.stderr, /release drift detected/)
    assert.match(result.stderr, /npm latest dsh-uml-viewer\s+= 1\.2\.2/)
    assert.match(result.stderr, /repository package\.json version\s+= 1\.2\.3/)
    assert.match(result.stderr, /newest release tag\s+= v1\.2\.3 \(1\.2\.3\)/)
    assert.match(result.stderr, /the tag's version was never published/)
  } finally {
    server.close()
    await rm(doc.directory, { recursive: true, force: true })
  }
})

test('(a) the command exits zero with the in-progress message in the pre-tag window', async () => {
  const { server, url } = await registryServer({
    [main]: packument('1.2.2', ['1.2.0', '1.2.2']),
    [platform]: packument('1.2.2', ['1.2.0', '1.2.2']),
  })
  const doc = await scratchDoc('1.2.2')
  try {
    const result = await runCommand(['scripts/release-drift.mjs', '--registry', url, '--grace-ms', '0', '--tag', 'v1.2.2', '--repo-version', '1.2.3', '--releasing-doc', doc.file])
    assert.equal(result.status, 0, result.stdout + result.stderr)
    assert.match(result.stdout, /release in progress: repository package\.json version 1\.2\.3 is newer than the newest release tag v1\.2\.2/)
    assert.doesNotMatch(result.stderr, /drift/)
  } finally {
    server.close()
    await rm(doc.directory, { recursive: true, force: true })
  }
})

test('the command exits zero when the four facts agree', async () => {
  const { server, url } = await registryServer({
    [main]: packument('1.2.3', ['1.2.0', '1.2.3']),
    [platform]: packument('1.2.3', ['1.2.0', '1.2.3']),
  })
  const doc = await scratchDoc('1.2.3')
  try {
    const result = await runCommand(['scripts/release-drift.mjs', '--registry', url, '--grace-ms', '0', '--tag', 'v1.2.3', '--repo-version', '1.2.3', '--releasing-doc', doc.file])
    assert.equal(result.status, 0, result.stdout + result.stderr)
    assert.match(result.stdout, /release facts agree: 1\.2\.3 = v1\.2\.3/)
    assert.match(result.stdout, /docs\/releasing\.md Status header agrees/)
  } finally {
    server.close()
    await rm(doc.directory, { recursive: true, force: true })
  }
})

test('the Status header names one version and one check date', () => {
  const status = parseStatusHeader('# Release packaging and publishing\n\nStatus (2026-11-26): `v1.2.2` is the latest verified **published** release.\n')
  assert.deepEqual(status, { checked: '2026-11-26', tag: 'v1.2.2', version: '1.2.2' })
  assert.throws(() => parseStatusHeader('Status (2026-11-26): `v1.2.2` shipped.\n'), /has no machine-readable Status header/)
  assert.throws(() => parseStatusHeader('# nothing here\n'), /has no machine-readable Status header/)
})

test('a Status header naming another version fails with both sides named', () => {
  const observed = facts({ repoVersion: '1.2.2', tag: 'v1.2.2', latest: '1.2.2', versions: ['1.2.0', '1.2.2'], docs: { checked: '2026-11-26', tag: 'v1.2.1', version: '1.2.1' } })
  const comparison = compareReleaseFacts(observed)
  assert.equal(comparison.ok, false)
  assert.ok(comparison.problems.includes('docs say v1.2.1, registry is 1.2.2 for dsh-uml-viewer: the Status header is not npm dist-tags.latest'))
  assert.ok(comparison.problems.includes('docs say v1.2.1, newest release tag is v1.2.2: the Status header is not the newest release tag'))
})

test('a stale Status header is advisory in the pre-tag window, like the registry', () => {
  const observed = facts({ repoVersion: '1.2.3', tag: 'v1.2.2', latest: '1.2.2', versions: ['1.2.0', '1.2.2'], docs: { checked: '2026-11-26', tag: 'v1.2.1', version: '1.2.1' } })
  const comparison = compareReleaseFacts(observed)
  assert.equal(comparison.state, 'in-progress')
  assert.equal(comparison.ok, true)
  assert.deepEqual(comparison.problems, [])
  assert.ok(comparison.warnings.includes('docs say v1.2.1, registry is 1.2.2 for dsh-uml-viewer: the Status header is not npm dist-tags.latest'))
})

test('facts without a Status header cannot claim agreement', () => {
  const observed = { ...facts({ repoVersion: '1.2.2', tag: 'v1.2.2', latest: '1.2.2', versions: ['1.2.0', '1.2.2'] }), docs: undefined }
  const comparison = compareReleaseFacts(observed)
  assert.equal(comparison.ok, false)
  assert.ok(comparison.problems.some(problem => /docs\/releasing\.md Status header was not read/.test(problem)))
})

test('the command fails offline when the Status header names the wrong version', async () => {
  const { server, url } = await registryServer({
    [main]: packument('1.2.2', ['1.2.0', '1.2.2']),
    [platform]: packument('1.2.2', ['1.2.0', '1.2.2']),
  })
  const doc = await scratchDoc('1.2.1')
  try {
    const result = await runCommand(['scripts/release-drift.mjs', '--registry', url, '--grace-ms', '0', '--tag', 'v1.2.2', '--repo-version', '1.2.2', '--releasing-doc', doc.file])
    assert.equal(result.status, 1, result.stdout + result.stderr)
    assert.match(result.stderr, /docs\/releasing\.md Status header\s+= v1\.2\.1 \(checked 2026-11-26\)/)
    assert.match(result.stderr, /docs say v1\.2\.1, registry is 1\.2\.2 for dsh-uml-viewer/)
    assert.match(result.stderr, /docs say v1\.2\.1, newest release tag is v1\.2\.2/)
  } finally {
    server.close()
    await rm(doc.directory, { recursive: true, force: true })
  }
})

test('the repository docs/releasing.md carries a machine-readable Status header', async () => {
  const status = await readReleasingStatus()
  assert.equal(releasingDocPath, 'docs/releasing.md')
  assert.equal(status.tag, `v${status.version}`)
  assert.match(status.version, /^\d+\.\d+\.\d+/)
  assert.match(status.checked, /^\d{4}-\d{2}-\d{2}$/)
})
