import { readFile } from 'node:fs/promises'
import { execFile } from 'node:child_process'
import { promisify } from 'node:util'
import path from 'node:path'
import { pathToFileURL } from 'node:url'
import { root } from './version.mjs'
import { platforms, tagVersion } from './release.mjs'

const exec = promisify(execFile)

// The published lockstep set: the main package and every platform package.
export const mainPackageName = 'dsh-uml-viewer'
export const platformPackageNames = platforms.map(entry => `@dsh-uml-viewer/${entry.platform}`)
export const packageNames = [mainPackageName, ...platformPackageNames]
export const registryUrl = 'https://registry.npmjs.org'

// docs/releasing.md opens with a Status header that restates the published
// version. That is a claim about an external fact, so the command that checks
// the package checks the header too, against the same two authorities.
// The grammar is fixed so the parser reads it deterministically: a header the
// parser cannot read fails the check instead of silently dropping the claim.
export const releasingDocPath = 'docs/releasing.md'
export const statusHeaderPattern = /^Status \((\d{4}-\d{2}-\d{2})\): `(v[^`\s]+)` is the latest verified \*\*published\*\* release\.$/m

// The measured propagation window between the two packages was 67 seconds and
// npm itself warns that a publication may take a few minutes to become
// visible, so a check that runs right after a release must wait a little.
export const defaultGraceMs = 5 * 60 * 1000
export const backoffMs = [15_000, 30_000, 45_000, 60_000, 60_000, 60_000]
export const networkRetryLimit = 3

export class RegistryError extends Error {
  constructor(message, { packageName } = {}) {
    super(message)
    this.name = 'RegistryError'
    this.packageName = packageName
  }
}

export async function manifestVersion(directory = root) {
  const file = path.join(directory, 'package.json')
  const json = JSON.parse(await readFile(file, 'utf8'))
  if (typeof json.version !== 'string' || json.version === '') throw new Error(`${file} has no version field`)
  return json.version
}

export function parseStatusHeader(text, file = releasingDocPath) {
  const match = statusHeaderPattern.exec(text ?? '')
  if (!match) {
    throw new Error(file + ' has no machine-readable Status header: expected one line "Status (YYYY-MM-DD): `vX.Y.Z` is the latest verified **published** release."')
  }
  const [, checked, tag] = match
  let version
  try {
    version = tagVersion(tag)
  } catch {
    throw new Error(file + ' Status header names ' + tag + ', which is not a v<semver> release tag')
  }
  return { checked, tag, version }
}

export async function readReleasingStatus(file = path.join(root, releasingDocPath)) {
  let text
  try {
    text = await readFile(file, 'utf8')
  } catch (error) {
    throw new Error(file + ' cannot be read, so its Status header cannot be checked: ' + (error?.message ?? error))
  }
  return parseStatusHeader(text, file)
}

export async function listReleaseTags(directory = root) {
  const { stdout } = await exec('git', ['tag', '--list', 'v*', '--sort=-v:refname'], { cwd: directory })
  return stdout.split(/\r?\n/).map(line => line.trim()).filter(Boolean)
}

export function newestReleaseTag(tags) {
  for (const tag of tags) {
    let version
    try { version = tagVersion(tag) } catch { continue }
    return { tag, version }
  }
  throw new Error('No v* release tag found; fetch the full tag list (git fetch --tags) before checking')
}

export function compareVersions(left, right) {
  const parse = value => {
    const match = /^(\d+)\.(\d+)\.(\d+)(?:[-+](.*))?$/.exec(value ?? '')
    return match ? { parts: [Number(match[1]), Number(match[2]), Number(match[3])], pre: match[4] ?? '' } : undefined
  }
  const a = parse(left)
  const b = parse(right)
  if (!a || !b) return 0
  for (let index = 0; index < 3; index += 1) {
    if (a.parts[index] !== b.parts[index]) return a.parts[index] < b.parts[index] ? -1 : 1
  }
  if (a.pre === b.pre) return 0
  if (a.pre === '') return 1
  if (b.pre === '') return -1
  return a.pre < b.pre ? -1 : 1
}

export function registryUrlFor(name, registry = registryUrl) {
  const base = registry.replace(/\/+$/, '')
  return `${base}/${name.startsWith('@') ? name.replace('/', '%2f') : name}`
}

export async function readRegistryPackage(name, { fetchImpl = fetch, registry = registryUrl } = {}) {
  const url = registryUrlFor(name, registry)
  let response
  try {
    response = await fetchImpl(url, { headers: { accept: 'application/json' } })
  } catch (error) {
    throw new RegistryError(`npm registry is unreachable (${name}): ${error?.message ?? error}`, { packageName: name })
  }
  if (response.status === 404) return { name, latest: undefined, versions: [], missing: true }
  if (!response.ok) throw new RegistryError(`npm registry request for ${name} failed with HTTP ${response.status}`, { packageName: name })
  let document
  try {
    document = await response.json()
  } catch (error) {
    throw new RegistryError(`npm registry returned an unreadable document for ${name}: ${error?.message ?? error}`, { packageName: name })
  }
  const latest = document?.['dist-tags']?.latest
  if (typeof latest !== 'string' || latest === '') {
    throw new RegistryError(`npm registry document for ${name} has no dist-tags.latest`, { packageName: name })
  }
  return { name, latest, versions: Object.keys(document?.versions ?? {}), missing: false }
}

// Three states, decided by the repository version against the newest tag:
//   in-progress  the version was bumped and main was pushed before the tag;
//                nothing is tagged or released yet, so this never fails.
//   tagged       repository version == newest tag: the strict comparison runs.
//   behind       the tag exists but the tree does not carry its version: drift.
export function releaseState({ repoVersion, tag }) {
  const order = compareVersions(repoVersion, tag.version)
  if (order > 0) return 'in-progress'
  if (order < 0) return 'behind'
  return 'tagged'
}

export function compareReleaseFacts({ repoVersion, tag, packages, docs }) {
  const state = releaseState({ repoVersion, tag })
  const problems = []
  const warnings = []
  if (state === 'behind') {
    problems.push(`repository package.json version ${repoVersion} is older than the newest release tag ${tag.tag} (${tag.version}): the tree does not carry the tagged version`)
  }
  // The registry is always compared with the newest tag's version. In the
  // pre-tag window that comparison is advisory only: a release in progress is
  // not drift, so it can never redden main.
  const sink = state === 'in-progress' ? warnings : problems
  for (const entry of packages) {
    if (entry.missing) {
      sink.push(`npm has no package ${entry.name}: it was never published`)
      continue
    }
    if (entry.latest !== tag.version) {
      const direction = compareVersions(entry.latest, tag.version) < 0
        ? 'the registry is behind the tag'
        : 'the registry is ahead of the tag'
      sink.push(`npm latest for ${entry.name} is ${entry.latest}, not the tag version ${tag.version} (${direction})`)
    }
    if (!entry.versions.includes(tag.version)) {
      sink.push(`npm has no version ${tag.version} for ${entry.name}: the tag's version was never published`)
    }
  }
  // The Status header restates the published version, so it is compared with
  // the same two authorities as the package. A header that was never read is a
  // problem in every state: the claim is then unchecked, not true.
  if (!docs) {
    problems.push(`${releasingDocPath} Status header was not read, so its published-version claim is unchecked`)
  } else {
    const latest = packages.find(entry => entry.name === mainPackageName)?.latest
    if (latest && docs.version !== latest) {
      sink.push(`docs say ${docs.tag}, registry is ${latest} for ${mainPackageName}: the Status header is not npm dist-tags.latest`)
    }
    if (docs.version !== tag.version) {
      sink.push(`docs say ${docs.tag}, newest release tag is ${tag.tag}: the Status header is not the newest release tag`)
    }
  }
  return { ok: problems.length === 0, state, problems, warnings }
}

export async function collectFacts({
  directory = root,
  fetchImpl = fetch,
  registry = registryUrl,
  tag,
  repoVersion,
  releasingDoc,
} = {}) {
  const resolvedTag = tag
    ? { tag, version: tagVersion(tag) }
    : newestReleaseTag(await listReleaseTags(directory))
  const resolvedRepoVersion = repoVersion ?? await manifestVersion(directory)
  // The header is local and cheap, so it is read before the registry: a header
  // that cannot be read fails at once, not after the grace window.
  const docs = await readReleasingStatus(releasingDoc ?? path.join(directory, releasingDocPath))
  const packages = []
  for (const name of packageNames) {
    packages.push(await readRegistryPackage(name, { fetchImpl, registry }))
  }
  return { repoVersion: resolvedRepoVersion, tag: resolvedTag, packages, docs }
}

async function refreshReleaseTags(directory) {
  try {
    await exec('git', ['fetch', '--tags', '--quiet'], { cwd: directory, timeout: 20_000 })
  } catch {
    // A tag that is not pushed yet simply stays invisible; the next attempt
    // reads whatever the checkout holds.
  }
}

export async function checkReleaseDrift(options = {}) {
  const {
    collect = collectFacts,
    graceMs = defaultGraceMs,
    sleep = ms => new Promise(resolve => setTimeout(resolve, ms)),
    now = () => Date.now(),
    refreshTags = true,
    onAttempt,
  } = options
  const started = now()
  const deadline = started + graceMs
  let attempt = 0
  let networkFailures = 0
  let last
  for (;;) {
    attempt += 1
    if (attempt > 1 && refreshTags) await refreshReleaseTags(options.directory ?? root)
    try {
      const facts = await collect(options)
      last = { facts, ...compareReleaseFacts(facts) }
      networkFailures = 0
      if (last.ok) return { ok: true, attempts: attempt, elapsedMs: now() - started, ...last }
    } catch (error) {
      if (!(error instanceof RegistryError)) throw error
      last = { error }
      networkFailures += 1
    }
    if (onAttempt) onAttempt({ attempt, last })
    const remaining = deadline - now()
    if (remaining <= 0 || networkFailures >= networkRetryLimit) break
    const wait = Math.min(backoffMs[Math.min(attempt - 1, backoffMs.length - 1)], remaining)
    await sleep(wait)
  }
  return { ok: false, attempts: attempt, elapsedMs: now() - started, ...last }
}

export function formatFacts(facts) {
  const lines = []
  for (const entry of facts.packages) {
    const latest = entry.missing ? '(package missing on the registry)' : entry.latest
    lines.push(`  npm latest ${entry.name.padEnd(30)} = ${latest}`)
  }
  lines.push(`  repository package.json version     = ${facts.repoVersion}`)
  lines.push(`  newest release tag                  = ${facts.tag.tag} (${facts.tag.version})`)
  const header = facts.docs ? `${facts.docs.tag} (checked ${facts.docs.checked})` : '(not read)'
  lines.push('  ' + `${releasingDocPath} Status header`.padEnd(41) + ' = ' + header)
  return lines.join('\n')
}

export function formatDrift(result) {
  const seconds = Math.round((result.elapsedMs ?? 0) / 1000)
  const lines = [`release drift detected after ${result.attempts} attempt(s) over ${seconds}s:`]
  if (result.facts) lines.push(formatFacts(result.facts))
  if (result.problems?.length) {
    lines.push('differences:')
    for (const problem of result.problems) lines.push(`  - ${problem}`)
  }
  return lines.join('\n')
}

export function formatInProgress(result) {
  const { facts } = result
  const lines = [
    `release in progress: repository package.json version ${facts.repoVersion} is newer than the newest release tag ${facts.tag.tag} (${facts.tag.version}); nothing is tagged yet, so there is no release to check against npm.`,
    formatFacts(facts),
  ]
  if (result.warnings?.length) {
    lines.push('warnings about the newest tag (advisory, not a failure):')
    for (const warning of result.warnings) lines.push(`  - ${warning}`)
  }
  return lines.join('\n')
}

export function formatUnreachable(result) {
  return [
    `release drift check could not read the npm registry after ${result.attempts} attempt(s) over ${Math.round((result.elapsedMs ?? 0) / 1000)}s:`,
    `  - ${result.error?.message ?? 'unknown registry failure'}`,
    'The published versions are unknown, so no release claim can be made. Retry when the registry answers.',
  ].join('\n')
}

function option(args, name) {
  const index = args.indexOf(name)
  if (index < 0) return undefined
  const value = args[index + 1]
  if (!value) throw new Error(`Missing value for ${name}`)
  return value
}

async function main(args) {
  const graceOption = option(args, '--grace-ms')
  const graceMs = graceOption === undefined ? defaultGraceMs : Number(graceOption)
  if (!Number.isFinite(graceMs) || graceMs < 0) throw new Error('--grace-ms must be a non-negative number of milliseconds')
  const registry = option(args, '--registry') ?? registryUrl
  const tag = option(args, '--tag')
  const repoVersion = option(args, '--repo-version')
  const releasingDoc = option(args, '--releasing-doc')
  let result
  try {
    result = await checkReleaseDrift({
      graceMs,
      registry,
      collect: options => collectFacts({ ...options, registry, tag, repoVersion, releasingDoc }),
    })
  } catch (error) {
    // A header the parser cannot read is neither drift nor a network failure:
    // the claim is unchecked, so the command fails and says why.
    console.error(`release drift check cannot read its inputs: ${error?.message ?? error}`)
    return 1
  }
  if (result.ok) {
    if (result.state === 'in-progress') {
      console.log(formatInProgress(result))
      return 0
    }
    const published = result.facts.packages.map(entry => `${entry.name}@${entry.latest}`).join(', ')
    console.log(`release facts agree: ${result.facts.repoVersion} = ${result.facts.tag.tag} (${published}); ${releasingDocPath} Status header agrees`)
    return 0
  }
  if (result.error) {
    console.error(formatUnreachable(result))
    return 1
  }
  console.error(formatDrift(result))
  return 1
}

if (process.argv[1] && import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href) {
  process.exitCode = await main(process.argv.slice(2))
}
