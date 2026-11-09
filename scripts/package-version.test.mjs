import assert from 'node:assert/strict'
import { mkdtemp, mkdir, copyFile, writeFile, readdir, rm, access } from 'node:fs/promises'
import path from 'node:path'
import { pathToFileURL } from 'node:url'
import { spawnSync } from 'node:child_process'
import { test } from 'node:test'

// Run this suite only with an explicit disposable scratch parent.
const parent = process.env.UML_VIEWER_PACKAGE_TEST_SCRATCH
if (!parent || !path.isAbsolute(parent)) throw new Error('Set absolute UML_VIEWER_PACKAGE_TEST_SCRATCH')
const source = path.resolve(import.meta.dirname, '..')
async function fixture(run) {
  const scratch = await mkdtemp(path.join(parent, 'payload-gate-'))
  try {
    for (const dir of ['scripts', 'crates', 'packages', 'lib', 'fonts', 'locale', 'docs']) await mkdir(path.join(scratch, dir), { recursive: true })
    for (const file of ['scripts/package.mjs', 'scripts/package.test.mjs', 'scripts/version.mjs', 'Cargo.toml', 'package.json', 'dsh-plugin.json', 'README.md', 'icon.svg', 'cordis.patch.yml', 'docs/plugin-development.md']) await copyFile(path.join(source, file), path.join(scratch, file))
    for (const dir of ['crates', 'packages']) for (const entry of await readdir(path.join(source, dir), { withFileTypes: true })) if (entry.isDirectory()) {
      await mkdir(path.join(scratch, dir, entry.name))
      const file = dir === 'crates' ? 'Cargo.toml' : 'package.json'
      await copyFile(path.join(source, dir, entry.name, file), path.join(scratch, dir, entry.name, file))
    }
    for (const file of ['lib/host.js', 'lib/client.js', 'fonts/SarasaMonoSC-Regular.ttf', 'fonts/LICENSE-Sarasa.txt', 'locale/en-US.json']) await writeFile(path.join(scratch, file), 'controlled package fixture\n')
    const binary = path.join(scratch, 'identity.cjs')
    await writeFile(binary, "console.log('uml-viewer 0.1.0')\n")
    const output = path.join(scratch, 'out')
    const module = await import(pathToFileURL(path.join(scratch, 'scripts/package.mjs')).href)
    await run({ module, binary, output })
  } finally { await rm(scratch, { recursive: true, force: true }) }
}

test('existing package contract tests pass inside the explicit scratch mirror', async () => {
  await fixture(async ({ binary }) => {
    const mirror = path.dirname(binary)
    const { NODE_TEST_CONTEXT, ...env } = process.env
    const result = spawnSync(process.execPath, ['--test', '--test-reporter=tap', path.join(mirror, 'scripts/package.test.mjs')], {
      cwd: mirror, encoding: 'utf8', env: { ...env, UML_VIEWER_PACKAGE_REPORT: '' },
    })
    assert.equal(result.status, 0, result.stdout + result.stderr)
    assert.match(result.stdout, /# pass 3/)
  })
})

test('exact workspace identity assembles both controlled fixture artifacts', async () => {
  await fixture(async ({ module, binary, output }) => {
    await writeFile(binary, "console.log('uml-viewer 1.0.0')\n")
    const report = await module.assemblePackage({ binary, output }, (file, args, options) => {
      assert.equal(file, binary)
      assert.deepEqual(args, ['--version'])
      assert.equal(options.timeout, 5000)
      assert.equal(options.maxBuffer, 4096)
      assert.equal(options.shell, false)
      return spawnSync(process.execPath, [file, ...args], options)
    })
    assert.equal(report.version, '1.0.0')
    await access(report.main.path)
    await access(report.binary.path)
  })
})

for (const [name, result] of [
  ['missing', { stdout: '' }],
  ['malformed', { stdout: '1.0.0\n' }],
  ['wrong product', { stdout: 'other-tool 1.0.0\n' }],
  ['stderr', { stderr: 'warning' }],
  ['failed', { status: 1 }],
  ['signalled', { signal: 'SIGTERM' }],
  ['timed out', { error: Object.assign(new Error('timed out'), { code: 'ETIMEDOUT' }) }],
  ['overflow', { error: Object.assign(new Error('output too large'), { code: 'ENOBUFS' }) }],
  ['oversized output', { stdout: 'x'.repeat(4097) }],
]) test(`${name} identity leaves both artifacts absent`, async () => {
  await fixture(async ({ module, binary, output }) => {
    await assert.rejects(module.assemblePackage({ binary, output }, () => ({ status: 0, signal: null, stdout: 'uml-viewer 1.0.0\n', stderr: '', ...result })), /version/i)
    await assert.rejects(access(output), { code: 'ENOENT' })
  })
})

for (const [name, script] of [
  ['deadline', 'setInterval(() => {}, 1000)'],
  ['output limit', "process.stdout.write('x'.repeat(8192))"],
  ['nonzero exit', "console.log('uml-viewer 1.0.0'); process.exitCode = 7"],
  ['signal termination', "console.log('uml-viewer 1.0.0'); process.kill(process.pid, 'SIGTERM')"],
]) test(`real Node fixture ${name} refuses both artifacts`, async () => {
  await fixture(async ({ module, binary, output }) => {
    await writeFile(binary, script)
    await assert.rejects(module.assemblePackage({ binary, output }, (file, args, options) => spawnSync(process.execPath, [file, ...args], options)), /version/i)
    await assert.rejects(access(output), { code: 'ENOENT' })
  })
})

test('production CLI cannot package interpreter fixture bytes as a native binary', async () => {
  await fixture(async ({ binary, output }) => {
    const result = spawnSync(process.execPath, [path.join(source, 'scripts/package.mjs'), '--binary', binary, '--out', output], { encoding: 'utf8' })
    assert.notEqual(result.status, 0)
    await assert.rejects(access(output), { code: 'ENOENT' })
  })
})

test('extra output after binary identity refuses both artifacts', async () => {
  await fixture(async ({ module, binary, output }) => {
    await assert.rejects(module.assemblePackage({ binary, output }, () => ({ status: 0, signal: null, stdout: 'uml-viewer 1.0.0\n\n', stderr: '' })), /malformed/)
    await assert.rejects(access(output), { code: 'ENOENT' })
  })
})

test('stale binary identity refuses both artifacts before creating output', async () => {
  await fixture(async ({ module, binary, output }) => {
    await assert.rejects(module.assemblePackage({ binary, output }, (file, args, options) => spawnSync(process.execPath, [file, ...args], options)), /version.*0\.1\.0.*1\.0\.0/i)
    await assert.rejects(access(output), { code: 'ENOENT' })
  })
})
