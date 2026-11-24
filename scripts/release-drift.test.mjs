import assert from 'node:assert/strict'
import { mkdtemp, rm, writeFile } from 'node:fs/promises'
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
  readRegistryPackage,
  registryUrlFor,
  releaseState,
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

function facts({ repoVersion, tag, latest, versions }) {
  return {
    repoVersion,
    tag: { tag, version: tag.replace(/^v/, '') },
    packages: packageNames.map(name => ({ name, latest, versions, missing: false })),
  }
}

function fakeClock() {
  let value = 0
  return { now: () => value, sleep: async ms => { value += ms } }
}

async function scratchRepo(version) {
  const directory = await mkdtemp(path.join(root, '.scratch-drift-test-'))
  await writeFile(path.join(directory, 'package.json'), `${JSON.stringify({ name: 'dsh-uml-viewer', version }, null, 2)}\n`)
  const identity = ['-c', 'user.email=fixture@example.com', '-c', 'user.name=fixture']
  const git = (...args) => execFileSync('git', [...identity, ...args], { cwd: directory, stdio: 'ignore' })
  git('init', '-q')
  git('add', '.')
  git('commit', '-q', '-m', 'fixture')
  git('tag', '-a', `v${version}`, '-m', `v${version}`)
  return directory
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

test('the three facts agree in the tagged state', () => {
  const observed = facts({ repoVersion: '1.2.2', tag: 'v1.2.2', latest: '1.2.2', versions: ['1.2.0', '1.2.2'] })
  const comparison = compareReleaseFacts(observed)
  assert.equal(comparison.state, 'tagged')
  assert.deepEqual(comparison.problems, [])
  assert.equal(comparison.ok, true)
})

test('the drift report prints the three values and every difference', () => {
  const observed = facts({ repoVersion: '1.2.3', tag: 'v1.2.3', latest: '1.2.2', versions: ['1.2.2'] })
  const report = formatDrift({ attempts: 2, elapsedMs: 15_000, facts: observed, problems: compareReleaseFacts(observed).problems })
  assert.match(report, /release drift detected after 2 attempt/)
  assert.match(report, /npm latest dsh-uml-viewer\s+= 1\.2\.2/)
  assert.match(report, /npm latest @dsh-uml-viewer\/win32-x64\s+= 1\.2\.2/)
  assert.match(report, /repository package\.json version\s+= 1\.2\.3/)
  assert.match(report, /newest release tag\s+= v1\.2\.3 \(1\.2\.3\)/)
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

test('the collected facts read the manifest, the newest tag and both packages', async () => {
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

test('the command exits non-zero and prints the three values when the registry lags', async () => {
  const { server, url } = await registryServer({
    [main]: packument('1.2.2', ['1.2.0', '1.2.2']),
    [platform]: packument('1.2.2', ['1.2.0', '1.2.2']),
  })
  try {
    const result = await runCommand(['scripts/release-drift.mjs', '--registry', url, '--grace-ms', '0', '--tag', 'v1.2.3', '--repo-version', '1.2.3'])
    assert.equal(result.status, 1, result.stdout + result.stderr)
    assert.match(result.stderr, /release drift detected/)
    assert.match(result.stderr, /npm latest dsh-uml-viewer\s+= 1\.2\.2/)
    assert.match(result.stderr, /repository package\.json version\s+= 1\.2\.3/)
    assert.match(result.stderr, /newest release tag\s+= v1\.2\.3 \(1\.2\.3\)/)
    assert.match(result.stderr, /the tag's version was never published/)
  } finally {
    server.close()
  }
})

test('(a) the command exits zero with the in-progress message in the pre-tag window', async () => {
  const { server, url } = await registryServer({
    [main]: packument('1.2.2', ['1.2.0', '1.2.2']),
    [platform]: packument('1.2.2', ['1.2.0', '1.2.2']),
  })
  try {
    const result = await runCommand(['scripts/release-drift.mjs', '--registry', url, '--grace-ms', '0', '--tag', 'v1.2.2', '--repo-version', '1.2.3'])
    assert.equal(result.status, 0, result.stdout + result.stderr)
    assert.match(result.stdout, /release in progress: repository package\.json version 1\.2\.3 is newer than the newest release tag v1\.2\.2/)
    assert.doesNotMatch(result.stderr, /drift/)
  } finally {
    server.close()
  }
})

test('the command exits zero when the three facts agree', async () => {
  const { server, url } = await registryServer({
    [main]: packument('1.2.3', ['1.2.0', '1.2.3']),
    [platform]: packument('1.2.3', ['1.2.0', '1.2.3']),
  })
  try {
    const result = await runCommand(['scripts/release-drift.mjs', '--registry', url, '--grace-ms', '0', '--tag', 'v1.2.3', '--repo-version', '1.2.3'])
    assert.equal(result.status, 0, result.stdout + result.stderr)
    assert.match(result.stdout, /release facts agree: 1\.2\.3 = v1\.2\.3/)
  } finally {
    server.close()
  }
})
