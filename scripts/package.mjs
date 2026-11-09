import { readFile, writeFile, mkdir, readdir, stat } from 'node:fs/promises'
import path from 'node:path'
import { gzipSync } from 'node:zlib'
import { createHash } from 'node:crypto'
import { checkVersions, root } from './version.mjs'

const supported = ['win32-x64', 'linux-x64', 'darwin-arm64', 'darwin-x64']
const platform = `${process.platform}-${process.arch}`
const args = process.argv.slice(2)
function option(name) {
  const index = args.indexOf(name)
  if (index < 0 || !args[index + 1]) throw new Error(`Usage: node scripts/package.mjs --binary <absolute native binary> --out <absolute output directory>; missing ${name}`)
  return args[index + 1]
}
const binary = option('--binary')
const output = option('--out')
if (!path.isAbsolute(binary) || !path.isAbsolute(output)) throw new Error('--binary and --out must be absolute paths')
if (!supported.includes(platform)) throw new Error(`Unsupported package platform ${platform}; supported: ${supported.join(', ')}`)
if (!(await stat(binary).catch(() => null))?.isFile()) throw new Error(`Native binary does not exist: ${binary}`)
const version = await checkVersions()
const pkg = JSON.parse(await readFile(path.join(root, 'package.json'), 'utf8'))
const optionalDependencies = Object.fromEntries(supported.map(key => [`@dsh-uml-viewer/${key}`, version]))
const { scripts, devDependencies, private: developmentOnly, packageManager, ...runtime } = pkg
const mainManifest = { ...runtime, version, optionalDependencies }
const binaryName = process.platform === 'win32' ? 'uml-viewer.exe' : 'uml-viewer'
const platformManifest = {
  name: `@dsh-uml-viewer/${platform}`, version, description: `Native UML viewer for ${platform}`,
  license: pkg.license, repository: pkg.repository, os: [process.platform], cpu: [process.arch],
  files: ['bin'], exports: { './package.json': './package.json' },
}
const json = value => Buffer.from(JSON.stringify(value, null, 2) + '\n')
const mainFiles = new Map([['package.json', json(mainManifest)]])
for (const name of ['README.md', 'icon.svg', 'cordis.patch.yml', 'dsh-plugin.json', 'docs/plugin-development.md']) mainFiles.set(name, await readFile(path.join(root, name)))
async function include(directory) {
  for (const entry of await readdir(path.join(root, directory), { withFileTypes: true })) {
    const name = `${directory}/${entry.name}`
    if (entry.isDirectory()) await include(name)
    else if (entry.isFile() && !name.endsWith('.map')) mainFiles.set(name, await readFile(path.join(root, name)))
    else if (!entry.isFile()) throw new Error(`Package input must be a regular file: ${name}`)
  }
}
for (const directory of ['lib', 'fonts', 'locale']) await include(directory)
for (const required of ['lib/host.js', 'lib/client.js', 'fonts/SarasaMonoSC-Regular.ttf', 'fonts/LICENSE-Sarasa.txt']) {
  if (!mainFiles.has(required)) throw new Error(`Missing built package input ${required}; run pnpm build first`)
}
const binaryFiles = new Map([['package.json', json(platformManifest)], [`bin/${binaryName}`, await readFile(binary)]])

// npm-compatible ustar with fixed metadata, sorted names and gzip timestamp 0.
// No npm lifecycle hooks, registry access, local dependency rewriting or host mtimes.
function archive(files) {
  const blocks = []
  for (const [name, bytes] of [...files].sort(([left], [right]) => left.localeCompare(right, 'en'))) {
    const header = Buffer.alloc(512)
    const field = (offset, length, value) => header.write(value, offset, length, 'ascii')
    const octal = (offset, length, value) => field(offset, length, value.toString(8).padStart(length - 1, '0') + '\0')
    const filename = `package/${name}`
    if (Buffer.byteLength(filename) > 100) throw new Error(`ustar path exceeds 100 bytes: ${filename}`)
    field(0, 100, filename)
    octal(100, 8, name.startsWith('bin/') ? 0o755 : 0o644)
    octal(108, 8, 0); octal(116, 8, 0); octal(124, 12, bytes.length); octal(136, 12, 0)
    header.fill(32, 148, 156)
    field(156, 1, '0'); field(257, 6, 'ustar\0'); field(263, 2, '00')
    const checksum = header.reduce((sum, byte) => sum + byte, 0)
    field(148, 8, checksum.toString(8).padStart(6, '0') + '\0 ')
    blocks.push(header, bytes, Buffer.alloc((512 - bytes.length % 512) % 512))
  }
  blocks.push(Buffer.alloc(1024))
  return gzipSync(Buffer.concat(blocks), { level: 9 })
}
await mkdir(output, { recursive: true })
async function emit(name, files) {
  const bytes = archive(files)
  const destination = path.join(output, name)
  await writeFile(destination, bytes)
  return { path: destination, sha256: createHash('sha256').update(bytes).digest('hex'), files: [...files.keys()].sort() }
}
const report = {
  version, platform, optionalDependencies,
  main: await emit(`dsh-uml-viewer-${version}.tgz`, mainFiles),
  binary: await emit(`dsh-uml-viewer-${platform}-${version}.tgz`, binaryFiles),
}
await writeFile(path.join(output, 'package-report.json'), JSON.stringify(report, null, 2) + '\n')
console.log(JSON.stringify(report, null, 2))
