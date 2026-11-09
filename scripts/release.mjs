// Release packaging guards (#33): one native runner per delivered platform, a
// single version authority, exactly one platform-independent main archive, and
// a publish order that puts the four platform packages before the main package.
//
// The assembler (scripts/package.mjs) runs the supplied binary as `--version`,
// so a platform package must be built on the OS and CPU it ships to; no foreign
// target binary is ever executed on this host.
//
// `verify` does not trust the assembler report: it opens every archive that
// would be published with the bounded gzip/tar reader (scripts/archive.mjs),
// checks the real file list against the report, and checks the archive's own
// package.json against the workspace, the platform table and the platform
// report. The archives are data here; no packaged binary is executed.
import { readFile, writeFile, mkdir, copyFile, readdir } from 'node:fs/promises'
import path from 'node:path'
import { createHash } from 'node:crypto'
import { pathToFileURL } from 'node:url'
import { entryPrefix, readArchive } from './archive.mjs'
import { releaseVersion, root } from './version.mjs'

// Runner labels come from GitHub's hosted-runner reference (GitHub Docs,
// "Supported runners and hardware resources"): `macos-latest` is arm64, so
// Intel macOS needs an explicit `-intel` label. The `main` entry is the single
// runner whose assembled main archive is published.
export const platforms = [
  { platform: 'win32-x64', os: 'win32', cpu: 'x64', runner: 'windows-latest', binary: 'target/release/uml-viewer.exe', main: false },
  { platform: 'linux-x64', os: 'linux', cpu: 'x64', runner: 'ubuntu-latest', binary: 'target/release/uml-viewer', main: true },
  { platform: 'darwin-arm64', os: 'darwin', cpu: 'arm64', runner: 'macos-latest', binary: 'target/release/uml-viewer', main: false },
  { platform: 'darwin-x64', os: 'darwin', cpu: 'x64', runner: 'macos-15-intel', binary: 'target/release/uml-viewer', main: false },
]
export const mainPlatform = platforms.find(entry => entry.main).platform

const reportName = 'assemble-report.json'

// A release tag is the only publishing trigger: it must be v<workspace version>.
export function tagVersion(tag) {
  const match = /^v((?:0|[1-9]\d*)\.(?:0|[1-9]\d*)\.(?:0|[1-9]\d*)(?:-[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)?(?:\+[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)?)$/.exec(tag ?? '')
  if (!match) throw new Error(`Release tag ${JSON.stringify(tag)} is not v<workspace version>`)
  return match[1]
}

export function releaseTag(tag, version) {
  const tagged = tagVersion(tag)
  if (tagged !== version) throw new Error(`Release tag ${tag} (${tagged}) does not match Cargo workspace version ${version}`)
  return tagged
}

function digestBytes(bytes) {
  return createHash('sha256').update(bytes).digest('hex')
}

async function digest(file) {
  return digestBytes(await readFile(file))
}

function canonical(value) {
  return Object.fromEntries(Object.entries(value ?? {}).sort(([left], [right]) => left.localeCompare(right, 'en')))
}

// What the installable main package must really contain: the plugin runtime plus
// the packaged Sarasa face and its license. The assembler already refuses to run
// without them; this checks the archive a user would download.
const mainPayload = [
  'README.md', 'package.json', 'icon.svg', 'cordis.patch.yml', 'dsh-plugin.json',
  'docs/plugin-development.md', 'lib/host.js', 'lib/client.js',
  'fonts/SarasaMonoSC-Regular.ttf', 'fonts/LICENSE-Sarasa.txt',
]

// A main archive member is publishable only if the source package.json `files`
// list covers it; npm always adds package.json and README.md, nothing else.
function publishedMainFile(name, declared) {
  if (name === 'package.json' || name === 'README.md') return true
  return declared.some(item => name === item || name.startsWith(`${item}/`))
}

// The assembler report lists archive file names; an entry may also pin a
// per-file SHA-256, which is then checked against the real bytes.
function reportedInventory(list, { file, label }) {
  if (!Array.isArray(list) || list.length === 0) throw new Error(`${file}: assembler report has no ${label} archive inventory`)
  const listed = new Map()
  for (const item of list) {
    const name = typeof item === 'string' ? item : item?.name
    if (typeof name !== 'string' || name === '') throw new Error(`${file}: ${label} archive inventory has an entry without a file name`)
    if (listed.has(name)) throw new Error(`${file}: ${label} archive inventory lists ${name} twice`)
    listed.set(name, typeof item === 'object' && item !== null && typeof item.sha256 === 'string' ? item.sha256 : undefined)
  }
  return listed
}

// Opens one archive and returns its real members below package/. The reader
// refuses a non-gzip file, a corrupt stream, a bad header checksum, a duplicate,
// an unsafe or absolute path, a link or other non-regular entry, and anything
// over the archive limits.
async function openArchive(file, { label, limits }) {
  const entries = await readArchive(file, limits)
  const files = new Map()
  for (const [name, entry] of entries) {
    if (!name.startsWith(entryPrefix)) throw new Error(`${file}: ${label} archive entry ${JSON.stringify(name)} is outside ${entryPrefix}`)
    files.set(name.slice(entryPrefix.length), entry)
  }
  if (files.size === 0) throw new Error(`${file}: ${label} archive has no files`)
  const manifestEntry = files.get('package.json')
  if (!manifestEntry) throw new Error(`${file}: ${label} archive has no package/package.json`)
  if (manifestEntry.size === 0) throw new Error(`${file}: ${label} package/package.json is empty`)
  let manifest
  try {
    manifest = JSON.parse(manifestEntry.bytes.toString('utf8'))
  } catch {
    throw new Error(`${file}: ${label} package/package.json is not valid JSON`)
  }
  return { file, label, files, manifest, names: [...files.keys()].sort() }
}

function verifyReportedInventory(archive, reported) {
  const { file, label, files, names } = archive
  const listed = reportedInventory(reported, archive)
  const missing = [...listed.keys()].filter(name => !files.has(name)).sort()
  if (missing.length > 0) throw new Error(`${file}: ${label} archive is missing reported ${missing.join(', ')}`)
  const unreported = names.filter(name => !listed.has(name))
  if (unreported.length > 0) throw new Error(`${file}: ${label} archive contains unreported ${unreported.join(', ')}`)
  for (const [name, sha256] of listed) {
    if (sha256 === undefined) continue
    if (digestBytes(files.get(name).bytes) !== sha256) {
      throw new Error(`${file}: ${label} archive entry ${name} does not match its reported sha256`)
    }
  }
}

// A platform package is the native binary and its manifest, at the table's OS
// and CPU. The binary entry must exist as a regular, non-empty file; it is never
// executed, because a foreign target binary cannot run on this host.
async function verifyPlatformArchive(item, { entry, version, limits }) {
  const archive = await openArchive(item.platformFile, { label: 'platform', limits })
  const { file, files, manifest } = archive
  if (manifest.name !== `@dsh-uml-viewer/${entry.platform}`) {
    throw new Error(`${file}: platform package name ${JSON.stringify(manifest.name)} is not @dsh-uml-viewer/${entry.platform}`)
  }
  if (manifest.version !== version) {
    throw new Error(`${file}: platform package version ${JSON.stringify(manifest.version)} != workspace version ${version}`)
  }
  if (JSON.stringify(manifest.os) !== JSON.stringify([entry.os])) {
    throw new Error(`${file}: platform package os ${JSON.stringify(manifest.os)} is not ${JSON.stringify([entry.os])} for ${entry.platform}`)
  }
  if (JSON.stringify(manifest.cpu) !== JSON.stringify([entry.cpu])) {
    throw new Error(`${file}: platform package cpu ${JSON.stringify(manifest.cpu)} is not ${JSON.stringify([entry.cpu])} for ${entry.platform}`)
  }
  const binary = `bin/uml-viewer${entry.os === 'win32' ? '.exe' : ''}`
  const payload = files.get(binary)
  if (!payload) throw new Error(`${file}: platform archive has no ${binary}`)
  if (payload.size === 0) throw new Error(`${file}: platform archive ${binary} is empty`)
  verifyReportedInventory(archive, item.record.inventory?.platform)
  return archive.names
}

// The main package is the plugin bundle: its own manifest must be the
// installable one, its payload must be exactly what the source package.json
// publishes, and its real file list must equal the assembler's report.
async function verifyMainArchive(file, { version, optionalDependencies, source, reported, limits }) {
  const archive = await openArchive(file, { label: 'main', limits })
  const { files, manifest, names } = archive
  if (manifest.name !== source.name) {
    throw new Error(`${file}: main package name ${JSON.stringify(manifest.name)} is not ${JSON.stringify(source.name)}`)
  }
  if (manifest.version !== version) {
    throw new Error(`${file}: main package version ${JSON.stringify(manifest.version)} != workspace version ${version}`)
  }
  if (manifest.private !== undefined) throw new Error(`${file}: main package.json is private, so the main package is never publishable`)
  for (const stripped of ['scripts', 'devDependencies', 'packageManager']) {
    if (manifest[stripped] !== undefined) throw new Error(`${file}: main package.json must not ship ${stripped}`)
  }
  const expectedDependencies = canonical(optionalDependencies)
  if (JSON.stringify(canonical(manifest.optionalDependencies)) !== JSON.stringify(expectedDependencies)) {
    throw new Error(`${file}: main package optional dependencies must pin exactly ${JSON.stringify(expectedDependencies)}`)
  }
  if (JSON.stringify(manifest.os) !== JSON.stringify(source.os)) {
    throw new Error(`${file}: main package os ${JSON.stringify(manifest.os)} != source ${JSON.stringify(source.os)}`)
  }
  if (JSON.stringify(manifest.cpu) !== JSON.stringify(source.cpu)) {
    throw new Error(`${file}: main package cpu ${JSON.stringify(manifest.cpu)} != source ${JSON.stringify(source.cpu)}`)
  }
  for (const name of mainPayload) {
    const entry = files.get(name)
    if (!entry) throw new Error(`${file}: main archive has no ${name}`)
    if (entry.size === 0) throw new Error(`${file}: main archive ${name} is empty`)
  }
  if (!Array.isArray(source.files) || source.files.length === 0) {
    throw new Error(`${file}: the source package.json declares no files to publish`)
  }
  for (const name of names) {
    if (!publishedMainFile(name, source.files)) {
      throw new Error(`${file}: main archive contains ${name}, which the source package.json files list does not publish`)
    }
  }
  verifyReportedInventory(archive, reported)
  return names
}

function expectedArchives(version) {
  return new Map([
    [`dsh-uml-viewer-${version}.tgz`, null],
    ...platforms.map(entry => [`dsh-uml-viewer-${entry.platform}-${version}.tgz`, entry.platform]),
  ])
}

async function walk(directory) {
  const found = []
  for (const entry of await readdir(directory, { withFileTypes: true })) {
    const absolute = path.join(directory, entry.name)
    if (entry.isDirectory()) found.push(...await walk(absolute))
    else if (entry.isFile()) found.push(absolute)
    else throw new Error(`Release input must be a regular file: ${absolute}`)
  }
  return found.sort()
}

// Stage one runner's upload: always the current-platform archive, and the main
// archive only on the single main runner. The assembler report hash is verified
// against the copied bytes so a stale scratch directory cannot be uploaded.
export async function stageArchives({ packageDir, platform, uploadDir, main = false }) {
  const entry = platforms.find(item => item.platform === platform)
  if (!entry) throw new Error(`Unknown platform ${platform}; expected one of ${platforms.map(item => item.platform).join(', ')}`)
  const report = JSON.parse(await readFile(path.join(packageDir, 'package-report.json'), 'utf8'))
  if (report.platform !== platform) throw new Error(`Assembled report is for ${report.platform}, not ${platform}`)
  await mkdir(uploadDir, { recursive: true })
  const stageOne = async (artifact, label) => {
    if (typeof artifact?.path !== 'string' || typeof artifact?.sha256 !== 'string') {
      throw new Error(`Assembler report has no ${label} archive`)
    }
    const file = path.basename(artifact.path)
    const destination = path.join(uploadDir, file)
    await copyFile(artifact.path, destination)
    if (await digest(destination) !== artifact.sha256) {
      throw new Error(`Staged ${file} does not match the assembler report hash`)
    }
    return { file, sha256: artifact.sha256 }
  }
  const artifacts = {
    main: { file: path.basename(report.main.path), sha256: report.main.sha256 },
    platform: await stageOne(report.binary, 'current-platform'),
  }
  const inventory = { main: [...report.main.files], platform: [...report.binary.files] }
  const staged = ['platform']
  if (main) {
    if (platform !== mainPlatform) throw new Error(`The main archive must be staged on ${mainPlatform}, not ${platform}`)
    artifacts.main = await stageOne(report.main, 'main')
    staged.unshift('main')
  }
  const record = {
    platform, runner: entry.runner, version: report.version,
    optionalDependencies: report.optionalDependencies, artifacts, inventory, staged,
  }
  await writeFile(path.join(uploadDir, reportName), JSON.stringify(record, null, 2) + '\n')
  return record
}

// Verify everything that must be true before a publish: four distinct platform
// archives at the workspace version, one main archive built on the main runner,
// the same main archive inventory on every runner, no stray or stale archive,
// and the real contents of every staged archive. Byte identity of the main
// archive is recorded per runner rather than required, because checkout line
// endings are not pinned for packaged text.
export async function verifyInventory({ artifactsDir, version, limits }) {
  const expected = version ?? await releaseVersion()
  const source = JSON.parse(await readFile(path.join(root, 'package.json'), 'utf8'))
  const files = await walk(artifactsDir)
  const reports = files.filter(file => path.basename(file) === reportName)
  if (reports.length !== platforms.length) {
    throw new Error(`Expected exactly ${platforms.length} assemble reports, found ${reports.length}`)
  }
  const expectedDependencies = Object.fromEntries(platforms.map(entry => [`@dsh-uml-viewer/${entry.platform}`, expected]))
  const byPlatform = new Map()
  for (const file of reports) {
    const record = JSON.parse(await readFile(file, 'utf8'))
    const entry = platforms.find(item => item.platform === record.platform)
    if (!entry) throw new Error(`${file}: unexpected platform ${JSON.stringify(record.platform)}`)
    if (byPlatform.has(record.platform)) throw new Error(`Duplicate assemble report for ${record.platform}`)
    if (record.version !== expected) throw new Error(`${file}: platform package version ${record.version} != workspace version ${expected}`)
    if (JSON.stringify(canonical(record.optionalDependencies)) !== JSON.stringify(canonical(expectedDependencies))) {
      throw new Error(`${file}: optional dependencies must pin exactly ${JSON.stringify(canonical(expectedDependencies))}`)
    }
    if (typeof record.artifacts?.main?.sha256 !== 'string') throw new Error(`${file}: assembler report has no main archive hash`)
    if (!record.staged?.includes('platform')) throw new Error(`${file}: the current-platform archive was not staged`)
    const directory = path.dirname(file)
    const platformFile = path.join(directory, record.artifacts.platform?.file ?? '')
    const platformName = path.basename(platformFile)
    if (platformName !== `dsh-uml-viewer-${record.platform}-${expected}.tgz`) {
      throw new Error(`${file}: platform archive ${platformName} is not dsh-uml-viewer-${record.platform}-${expected}.tgz`)
    }
    if (!files.includes(platformFile)) throw new Error(`${platformFile}: platform archive is missing`)
    if (await digest(platformFile) !== record.artifacts.platform.sha256) {
      throw new Error(`${platformFile}: recorded sha256 does not match the staged bytes`)
    }
    const expectedPlatformFiles = [`bin/uml-viewer${entry.os === 'win32' ? '.exe' : ''}`, 'package.json'].sort()
    const platformInventory = [...record.inventory?.platform ?? []].sort()
    if (JSON.stringify(platformInventory) !== JSON.stringify(expectedPlatformFiles)) {
      throw new Error(`${file}: platform archive inventory ${platformInventory.join(', ')} != ${expectedPlatformFiles.join(', ')}`)
    }
    byPlatform.set(record.platform, { file, directory, record, platformFile, entry })
  }
  for (const entry of platforms) {
    if (!byPlatform.has(entry.platform)) throw new Error(`Missing platform package ${entry.platform}`)
  }
  // Every platform archive that would be published is opened here; the report's
  // file list alone is not proof that the archive holds those files.
  for (const item of byPlatform.values()) {
    item.verified = await verifyPlatformArchive(item, { entry: item.entry, version: expected, limits })
  }

  const mainRecords = [...byPlatform.values()].map(item => item.record).filter(record => record.staged.includes('main'))
  if (mainRecords.length !== 1) throw new Error(`Exactly one runner must stage the main archive, found ${mainRecords.length}`)
  if (mainRecords[0].platform !== mainPlatform) throw new Error(`The main archive must be staged on ${mainPlatform}, not ${mainRecords[0].platform}`)
  const inventories = platforms.map(entry => ({
    platform: entry.platform,
    files: [...byPlatform.get(entry.platform).record.inventory?.main ?? []].sort(),
  }))
  const mainInventory = inventories[0].files
  if (mainInventory.length === 0) throw new Error(`${byPlatform.get(mainPlatform).file}: assembler report has no main archive inventory`)
  for (const candidate of inventories) {
    if (JSON.stringify(candidate.files) !== JSON.stringify(mainInventory)) {
      throw new Error(`Main archive inventory differs between runners: ${candidate.platform} has ${candidate.files.join(', ')}`)
    }
  }
  const mainHashes = platforms.map(entry => ({
    platform: entry.platform, runner: entry.runner,
    sha256: byPlatform.get(entry.platform).record.artifacts.main.sha256,
  }))
  const mainHash = byPlatform.get(mainPlatform).record.artifacts.main.sha256
  const mainDirectory = byPlatform.get(mainPlatform)
  const mainFile = path.join(mainDirectory.directory, mainDirectory.record.artifacts.main.file)
  if (mainDirectory.record.artifacts.main.file !== `dsh-uml-viewer-${expected}.tgz`) {
    throw new Error(`Main archive ${mainDirectory.record.artifacts.main.file} is not dsh-uml-viewer-${expected}.tgz`)
  }
  if (!files.includes(mainFile)) throw new Error(`${mainFile}: main archive is missing`)
  if (await digest(mainFile) !== mainHash) {
    throw new Error(`${mainFile}: recorded sha256 does not match the staged bytes`)
  }
  // The one main archive that ships is opened as well: it must hold the plugin
  // payload the source package publishes, at the workspace version, and exactly
  // the files every runner reported.
  const verifiedMain = await verifyMainArchive(mainFile, {
    version: expected, optionalDependencies: expectedDependencies, source, reported: mainInventory, limits,
  })

  const names = expectedArchives(expected)
  const archives = files.filter(file => file.endsWith('.tgz'))
  if (archives.length !== platforms.length + 1) {
    throw new Error(`Expected exactly ${platforms.length + 1} release archives, found ${archives.length}: ${archives.map(file => path.basename(file)).join(', ')}`)
  }
  for (const archive of archives) {
    if (!names.has(path.basename(archive))) throw new Error(`Unexpected or stale release archive ${path.basename(archive)}`)
  }

  const artifacts = platforms.map(entry => {
    const item = byPlatform.get(entry.platform)
    return {
      name: `@dsh-uml-viewer/${entry.platform}`, platform: entry.platform, runner: entry.runner,
      file: item.record.artifacts.platform.file, path: item.platformFile, sha256: item.record.artifacts.platform.sha256,
      // The file list read out of the opened archive, not the report's claim.
      files: item.verified,
    }
  })
  artifacts.push({
    name: 'dsh-uml-viewer', platform: null, runner: mainPlatform,
    file: path.basename(mainFile), path: mainFile, sha256: mainHash,
    files: verifiedMain,
  })
  // Platform packages first: the main package's optional dependencies must
  // already exist when it becomes installable.
  return {
    version: expected,
    reproducibleMain: { file: path.basename(mainFile), sha256: mainHash, files: mainInventory, runners: platforms.length },
    mainHashes,
    artifacts,
    publishOrder: artifacts,
  }
}

function option(args, name, required = true) {
  const index = args.indexOf(name)
  const value = index < 0 ? undefined : args[index + 1]
  if (value) return value
  if (required) throw new Error(`Missing ${name}`)
  return undefined
}

async function main(args) {
  const [command] = args
  if (command === 'stage') {
    const record = await stageArchives({
      packageDir: option(args, '--package-dir'),
      platform: option(args, '--platform'),
      uploadDir: option(args, '--upload-dir'),
      main: args.includes('--main'),
    })
    console.log(JSON.stringify(record, null, 2))
    return
  }
  if (command === 'verify') {
    const inventory = await verifyInventory({ artifactsDir: option(args, '--artifacts') })
    const out = option(args, '--out', false)
    if (out) await writeFile(out, JSON.stringify(inventory, null, 2) + '\n')
    console.log(JSON.stringify(inventory, null, 2))
    return
  }
  if (command === 'plan') {
    const inventory = await verifyInventory({ artifactsDir: option(args, '--artifacts') })
    const lines = inventory.publishOrder.map(artifact => artifact.path).join('\n') + '\n'
    const out = option(args, '--out', false)
    if (out) await writeFile(out, lines)
    else process.stdout.write(lines)
    return
  }
  if (command === 'check-tag') {
    const version = await releaseVersion()
    console.log(`Release tag ${args[1]} matches Cargo workspace version ${releaseTag(args[1], version)}`)
    return
  }
  throw new Error('Usage: node scripts/release.mjs <stage|verify|plan|check-tag> [options]')
}

if (process.argv[1] && import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href) {
  await main(process.argv.slice(2))
}
