// Release packaging guards (#33): one native runner per delivered platform, a
// single version authority, exactly one platform-independent main archive, and
// a publish order that puts the four platform packages before the main package.
//
// The assembler (scripts/package.mjs) runs the supplied binary as `--version`,
// so a platform package must be built on the OS and CPU it ships to; no foreign
// target binary is ever executed on this host.
import { readFile, writeFile, mkdir, copyFile, readdir } from 'node:fs/promises'
import path from 'node:path'
import { createHash } from 'node:crypto'
import { pathToFileURL } from 'node:url'
import { releaseVersion } from './version.mjs'

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

async function digest(file) {
  return createHash('sha256').update(await readFile(file)).digest('hex')
}

function canonical(value) {
  return Object.fromEntries(Object.entries(value ?? {}).sort(([left], [right]) => left.localeCompare(right, 'en')))
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
// the same main archive inventory on every runner, and no stray or stale
// archive. Byte identity of the main archive is recorded per runner rather than
// required, because checkout line endings are not pinned for packaged text.
export async function verifyInventory({ artifactsDir, version }) {
  const expected = version ?? await releaseVersion()
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
    byPlatform.set(record.platform, { file, directory, record, platformFile })
  }
  for (const entry of platforms) {
    if (!byPlatform.has(entry.platform)) throw new Error(`Missing platform package ${entry.platform}`)
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
    }
  })
  artifacts.push({
    name: 'dsh-uml-viewer', platform: null, runner: mainPlatform,
    file: path.basename(mainFile), path: mainFile, sha256: mainHash,
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
