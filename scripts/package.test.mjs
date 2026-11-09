import assert from 'node:assert/strict'
import { mkdtemp, rm, readFile, writeFile } from 'node:fs/promises'
import path from 'node:path'
import { test } from 'node:test'
import { spawnSync } from 'node:child_process'
import { releaseVersion } from './version.mjs'

const root = path.resolve(import.meta.dirname, '..')
const run = args => spawnSync(process.execPath, args, { cwd: root, encoding: 'utf8' })

test('release version comes from Cargo workspace and every manifest agrees', () => {
  const result = run(['scripts/version.mjs', '--check'])
  assert.equal(result.status, 0, result.stdout + result.stderr)
})

test('release version rejects an invalid SemVer prerelease before assembling packages', async () => {
  const scratch = await mkdtemp(path.join(root, '.scratch-version-test-'))
  try {
    await writeFile(path.join(scratch, 'Cargo.toml'), '[workspace.package]\nversion = "1.0.0-01"\n')
    await assert.rejects(releaseVersion(scratch), /must be a SemVer/)
  } finally { await rm(scratch, { recursive: true, force: true }) }
})

test('package build refuses absent native input without emitting archives', async () => {
  const scratch = await mkdtemp(path.join(root, '.scratch-package-test-'))
  try {
    const result = run(['scripts/package.mjs', '--binary', path.join(scratch, 'missing.exe'), '--out', scratch])
    assert.notEqual(result.status, 0)
    assert.match(result.stderr, /binary.*(missing|exist)/i)
  } finally { await rm(scratch, { recursive: true, force: true }) }
})

// Run against real assembled archives, never fixture executables. The build command
// emits the archive inventory, then this public seam verifies that exact payload.
if (process.env.UML_VIEWER_PACKAGE_REPORT) test('assembled archives contain only the release runtime payload', async () => {
  const report = JSON.parse(await readFile(process.env.UML_VIEWER_PACKAGE_REPORT, 'utf8'))
  assert.equal(report.platform, `${process.platform}-${process.arch}`)
  for (const artifact of [report.main, report.binary]) {
    const listed = spawnSync('tar', ['-tzf', artifact.path], { encoding: 'utf8' })
    assert.equal(listed.status, 0, listed.stderr)
    const names = listed.stdout.trim().split(/\r?\n/).filter(name => !name.endsWith('/')).sort()
    assert.deepEqual(names, artifact.files.map(name => `package/${name}`).sort())
    assert.ok(!names.some(name => /(?:^|\/)(?:src|target|node_modules|\.metrics)(?:\/|$)|\.edn$/.test(name)))
  }
  const extract = (archive, name) => {
    const result = spawnSync('tar', ['-xOf', archive, `package/${name}`], { encoding: 'utf8', maxBuffer: 32 * 1024 * 1024 })
    assert.equal(result.status, 0, result.stderr)
    return result.stdout
  }
  const main = JSON.parse(extract(report.main.path, 'package.json'))
  assert.equal(main.name, 'dsh-uml-viewer')
  const sourceManifest = JSON.parse(await readFile(path.join(root, 'package.json'), 'utf8'))
  assert.equal(main.version, sourceManifest.version)
  assert.equal(main.private, undefined)
  assert.equal(main.devDependencies, undefined)
  assert.equal(main.scripts, undefined)
  assert.equal(main.engines.dsh, '>=0.2.2-rc.1 <0.3.0')
  assert.equal(main.dsh.client.platform, 'web')
  assert.equal(main.exports['.'], './lib/host.js')
  assert.equal(main.exports['./client'], './lib/client.js')
  assert.ok(Object.values(main.optionalDependencies).every(version => version === main.version))
  const native = JSON.parse(extract(report.binary.path, 'package.json'))
  assert.equal(native.name, `@dsh-uml-viewer/${report.platform}`)
  assert.equal(native.version, main.version)
  assert.deepEqual(native.os, [process.platform])
  assert.deepEqual(native.cpu, [process.arch])
  const manifest = JSON.parse(extract(report.main.path, 'dsh-plugin.json'))
  assert.equal(manifest.facets.host.entry, 'lib/host.js')
  assert.deepEqual(manifest.contributes['x-components'][0].tools,
    ['uml_ir', 'uml_crap', 'uml_mutate', 'uml_dry', 'uml_display'])
  assert.deepEqual(manifest.permissions.map(permission => permission.scope).sort(),
    ['dsh-home', 'filesystem-read', 'filesystem-write', 'subprocess'])
  const patch = extract(report.main.path, 'cordis.patch.yml')
  assert.equal((patch.match(/- id:/g) ?? []).length, 1)
  assert.match(patch, /name: dsh-uml-viewer/)
  assert.deepEqual(Object.keys(report.optionalDependencies).sort(), [
    '@dsh-uml-viewer/darwin-arm64', '@dsh-uml-viewer/darwin-x64',
    '@dsh-uml-viewer/linux-x64', '@dsh-uml-viewer/win32-x64',
  ])
  assert.ok(report.main.files.includes('fonts/SarasaMonoSC-Regular.ttf'))
  assert.ok(report.main.files.includes('fonts/LICENSE-Sarasa.txt'))
  assert.ok(report.main.files.includes('lib/client.js'))
  assert.ok(report.main.files.includes('lib/host.js'))
  assert.deepEqual(report.binary.files, ['bin/uml-viewer' + (process.platform === 'win32' ? '.exe' : ''), 'package.json'])
})
