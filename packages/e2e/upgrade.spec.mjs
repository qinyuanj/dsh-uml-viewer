import fs from 'node:fs/promises'
import { openSync, closeSync } from 'node:fs'
import path from 'node:path'
import { spawnSync } from 'node:child_process'
import { createHash } from 'node:crypto'
import { test, expect } from '@playwright/test'
import { packageInputs } from './dsh-web.mjs'
import { launchViewerLane } from './lane.mjs'
import { UPGRADE_PROMPTS, upgradeReply } from './upgrade-reply.mjs'
import { installEvidence } from './upgrade-evidence.mjs'

const initial = packageInputs()
const next = process.env.UML_VIEWER_UPGRADE_MAIN || process.env.UML_VIEWER_UPGRADE_PLATFORM
  ? packageInputs({ packageMain: process.env.UML_VIEWER_UPGRADE_MAIN, packagePlatform: process.env.UML_VIEWER_UPGRADE_PLATFORM }) : null

async function leases(home, version) {
  const dir = path.join(home, 'plugin-state', 'leases', version)
  try {
    return Object.fromEntries(await Promise.all((await fs.readdir(dir)).sort().map(async name =>
      [name, await fs.readFile(path.join(dir, name), 'utf8')])))
  } catch (error) { if (error.code === 'ENOENT') return {}; throw error }
}
const alive = pid => { try { process.kill(pid, 0); return true } catch { return false } }
async function canvasDigest(panel) {
  return createHash('sha256').update(await panel.canvas.evaluate(canvas => canvas.toDataURL())).digest('hex')
}
function binaryVersion(binary) {
  const result = spawnSync(binary, ['--version'], { encoding: 'utf8', timeout: 5000 })
  if (result.error || result.status !== 0 || result.signal || result.stderr) throw new Error('Installed binary identity inspection failed')
  return result.stdout.trim()
}

// OS observation of only the executable owned by this run. A lease's PID is
// the Host owner, so it is insufficient evidence of the resident's identity.
function resident(binary) {
  const quoted = binary.replaceAll("'", "''")
  const script = `$ErrorActionPreference='Stop'; @(Get-CimInstance Win32_Process -Filter "Name='uml-viewer.exe'" | Where-Object { $_.ExecutablePath -eq '${quoted}' } | Select-Object ProcessId,ParentProcessId,ExecutablePath,CreationDate) | ConvertTo-Json -Compress`
  const result = spawnSync('powershell.exe', ['-NoProfile', '-NonInteractive', '-Command', script], {
    encoding: 'utf8', timeout: 5000, maxBuffer: 16384,
  })
  if (result.error || result.status !== 0) throw new Error(`Resident OS observation failed: ${result.error?.message ?? result.stderr}`)
  const parsed = result.stdout.trim() ? JSON.parse(result.stdout) : []
  return Array.isArray(parsed) ? parsed : [parsed]
}

// Every public management action asks for danger-full-access. This answers the
// scratch page's real approval card each time it appears, without widening the
// session policy or reaching into the Host. It returns as soon as the scripted
// completion text is visible.
async function answerApprovals(page, done, timeoutMs = 120_000) {
  const deadline = Date.now() + timeoutMs
  const allow = () => page.locator('[data-approval-key]')
    .getByRole('button', { name: 'Allow once', exact: true }).first()
  while (Date.now() < deadline) {
    if (await done.isVisible().catch(() => false)) return
    const button = allow()
    if (await button.isVisible().catch(() => false)) {
      await button.click().catch(() => {})
      continue
    }
    await page.waitForTimeout(150)
  }
  throw new Error('The isolated management action never completed')
}

// This case adds a real package replacement and two explicit panel lifecycle
// actions to the ordinary open, so it needs more than the lane's default 180 s.
test.setTimeout(300_000)

test('installed update keeps the busy vN resident until explicit Close/Open, then cleans the unused version', async ({ browser }) => {
  test.skip(process.platform !== 'win32', 'Busy executable acceptance is the Windows CI lane.')
  test.skip(!initial || !next, 'Supply both initial and upgrade main/platform tarball pairs.')
  const observed = {}
  const lane = await launchViewerLane(browser, {
    packageMain: initial.main, packagePlatform: initial.platform, reply: upgradeReply(observed, { main: next.main }),
  })
  const evidence = { versions: ['1.0.0', '1.0.1'] }
  const home = path.join(lane.home, 'plugin-state')
  const oldBinary = path.join(home, '1.0.0', 'uml-viewer.exe')
  const newBinary = path.join(home, '1.0.1', 'uml-viewer.exe')
  try {
    let shop = await lane.openPanel('shop')
    expect(JSON.parse(await fs.readFile(path.join(home, 'current.json'), 'utf8')).version).toBe('1.0.0')
    const old = resident(oldBinary)
    expect(old).toHaveLength(1)
    const oldPid = old[0].ProcessId
    expect(alive(oldPid)).toBe(true)
    evidence.oldResident = old[0]
    evidence.oldBinaryVersion = binaryVersion(oldBinary)
    expect(evidence.oldBinaryVersion).toBe('uml-viewer 1.0.0')
    evidence.oldLeases = await leases(lane.home, '1.0.0')
    expect(Object.keys(evidence.oldLeases).length).toBeGreaterThanOrEqual(1)
    // An actual mapped Windows executable must refuse write access.
    expect(() => { const fd = openSync(oldBinary, 'r+'); closeSync(fd) }).toThrow()
    await lane.pan(shop)
    await expect.poll(() => shop.summary.innerText()).not.toBe(shop.before)
    await shop.page.mouse.move(1, 1)
    const state = await shop.summary.innerText()
    evidence.beforeState = state
    evidence.beforeCanvas = await canvasDigest(shop)
    const origin = new URL(lane.host.url).origin
    const hostPid = lane.host.pid
    // The pinned manager documents an update as removal then installation.
    // 1. Public plugin_manager list_bundles + remove_bundle. The list and the
    // removal each ask for the escalation they need through the scratch
    // application's own approval card; a policy that allows it shows none.
    await lane.sendPrompt(shop, UPGRADE_PROMPTS.remove)
    // Either scripted outcome ends the wait; the tool's own result decides.
    await answerApprovals(shop.page, shop.page.getByText(/^Isolated removal (applied|did not apply)$/).last())
    evidence.removal = observed.removal
    evidence.afterRemoval = await installEvidence(lane.home)
    expect(observed.removal?.application).toBe('applied')
    // 2. The 1.0.1 platform tarball as a plain dependency. With the bundle
    // removed nothing maps it, so replacing it cannot be `package-moved`; it
    // activates nothing by itself (CI11: an external CLI add never did).
    lane.installPlatform(next.platform)
    evidence.afterPlatform = await installEvidence(lane.home)
    expect(JSON.parse(await fs.readFile(path.join(home, 'current.json'), 'utf8')).version).toBe('1.0.0')
    // 3. Public plugin_manager install_bundle with the absolute 1.0.1 main
    // tarball. The profile's direct dependencies then differ only in
    // `dsh-uml-viewer` (the manager refuses any other count as
    // ambiguous-install); the plugin's activation still requires the exact
    // 1.0.1 platform package it finds. The manager selects the bundle and
    // reconciles the live profile itself, answering `applied` only after the
    // bundle's rows started.
    await lane.sendPrompt(shop, UPGRADE_PROMPTS.install)
    await answerApprovals(shop.page, shop.page.getByText(/^Isolated install (applied|did not apply)$/).last())
    // Record before asserting, so a refused install leaves its diagnostic.
    evidence.install = observed.install
    expect(observed.install?.application).toBe('applied')
    expect(observed.install).toMatchObject({ bundle: 'dsh-uml-viewer', version: '1.0.1' })
    evidence.afterInstall = await installEvidence(lane.home)
    // No host restart, page reload, private ctx or installed-file copying.
    try {
      await expect.poll(async () => JSON.parse(await fs.readFile(path.join(home, 'current.json'), 'utf8')).version,
        { timeout: 10_000 }).toBe('1.0.1')
    } catch (error) {
      // CI12: which store the activation pinned, and which package copy the
      // re-inserted row could import, decide between a lost `home` and a
      // stale 1.0.0 module (docs/e2e-harness.md); recorded before rethrowing.
      evidence.afterPoll = await installEvidence(lane.home)
      throw error
    }
    expect(new URL(lane.host.url).origin).toBe(origin)
    expect(lane.host.pid).toBe(hostPid)
    expect(alive(hostPid)).toBe(true)
    shop = await lane.reattachPanel(shop)
    await expect(shop.summary).toHaveText(state)
    await expect(shop.canvas).toBeVisible()
    expect(resident(oldBinary)).toEqual(old)
    expect(alive(oldPid)).toBe(true)
    expect(await leases(lane.home, '1.0.0')).toEqual(evidence.oldLeases)
    expect(resident(newBinary)).toEqual([])
    evidence.retainedState = await shop.summary.innerText()
    await shop.page.mouse.move(1, 1)
    await expect.poll(() => canvasDigest(shop)).toBe(evidence.beforeCanvas)
    evidence.retainedCanvas = await canvasDigest(shop)
    evidence.newBinaryVersion = binaryVersion(newBinary)
    expect(evidence.newBinaryVersion).toBe('uml-viewer 1.0.1')
    await lane.screenshot(shop, 'upgrade-retains-old')
    // A new zoom gesture after replacement proves that the old child still
    // serves actions through the real canvas, beyond a retained DOM summary.
    await lane.wheel(shop, -2)
    await expect.poll(() => shop.summary.innerText()).not.toBe(state)
    await shop.panel.getByRole('button', { name: 'Close viewer', exact: true }).click()
    await expect(shop.panel.getByRole('button', { name: 'Open viewer', exact: true })).toBeVisible()
    await expect.poll(() => alive(oldPid), { timeout: 10_000 }).toBe(false)
    await expect.poll(() => leases(lane.home, '1.0.0')).toEqual({})
    await shop.panel.getByRole('button', { name: 'Open viewer', exact: true }).click()
    await expect(shop.summary).toHaveAttribute('data-kind', 'frame')
    await expect(shop.canvas).toBeVisible()
    await expect.poll(() => resident(newBinary).length).toBe(1)
    evidence.newResident = resident(newBinary)[0]
    expect(evidence.newResident.ProcessId).not.toBe(oldPid)
    evidence.newLeases = await leases(lane.home, '1.0.1')
    expect(Object.keys(evidence.newLeases)).toHaveLength(1)
    const owner = records => JSON.parse(Object.values(records)[0]).pid
    expect(owner(evidence.newLeases)).toBe(owner(evidence.oldLeases))
    expect(alive(owner(evidence.newLeases))).toBe(true)
    await expect.poll(() => fs.stat(path.dirname(oldBinary)).then(() => true, error => {
      if (error.code === 'ENOENT') return false
      throw error
    }), { timeout: 10_000 }).toBe(false)
    evidence.oldDirectoryRemoved = true
    await lane.screenshot(shop, 'upgrade-opens-new')
    expect(shop.problems).toEqual([])
  } finally {
    await fs.writeFile(path.join(lane.home, 'upgrade-acceptance.json'), JSON.stringify(evidence, null, 2))
    await lane.close()
  }
})
