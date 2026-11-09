import assert from 'node:assert/strict'
import fs from 'node:fs'
import { createRequire } from 'node:module'
import os from 'node:os'
import path from 'node:path'
import test from 'node:test'
import { isolateScratch } from './scratch-scope.mjs'

// CI14: the scratch DSH home sat inside this checkout, whose root manifest is
// the dsh-uml-viewer package with an `exports` map. DSH continues a profile
// lookup "above the profile" at `<home>/profiles/package.json`; from there
// Node's package self-reference reached the checkout's own build.
function checkoutWithScratch() {
  // Canonical path: Node resolves through symlinks (macOS /var -> /private/var).
  const checkout = fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(), 'scratch-scope-')))
  fs.writeFileSync(path.join(checkout, 'package.json'), JSON.stringify({
    name: 'dsh-uml-viewer', private: true, main: 'lib/host.js', exports: { '.': './lib/host.js' },
  }))
  fs.mkdirSync(path.join(checkout, 'lib'))
  fs.writeFileSync(path.join(checkout, 'lib', 'host.js'), '')
  const scratch = path.join(checkout, '.scratch', 'e2e')
  const home = path.join(scratch, 'run-x')
  fs.mkdirSync(path.join(home, 'profiles', 'web'), { recursive: true })
  return { checkout, scratch, home }
}

function resolveAboveProfile(home) {
  try {
    return createRequire(path.join(home, 'profiles', 'package.json')).resolve('dsh-uml-viewer')
  } catch (error) {
    return error.code
  }
}

test('a run home under the checkout reaches its build from above the profile', t => {
  const { checkout, home } = checkoutWithScratch()
  t.after(() => fs.rmSync(checkout, { recursive: true, force: true }))
  assert.equal(resolveAboveProfile(home), path.join(checkout, 'lib', 'host.js'))
})

test('an isolated scratch keeps every run home out of the checkout package', t => {
  const { checkout, scratch, home } = checkoutWithScratch()
  t.after(() => fs.rmSync(checkout, { recursive: true, force: true }))
  isolateScratch(scratch)
  assert.equal(resolveAboveProfile(home), 'MODULE_NOT_FOUND')
  isolateScratch(scratch)
  assert.equal(resolveAboveProfile(home), 'MODULE_NOT_FOUND')
})

test('a scratch that is itself the plugin package is refused', t => {
  const { checkout } = checkoutWithScratch()
  t.after(() => fs.rmSync(checkout, { recursive: true, force: true }))
  assert.throws(() => isolateScratch(checkout), /must not be the dsh-uml-viewer package/)
})
