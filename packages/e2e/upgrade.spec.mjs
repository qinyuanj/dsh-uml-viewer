import fs from 'node:fs/promises'
import { openSync, closeSync } from 'node:fs'
import path from 'node:path'
import { spawnSync } from 'node:child_process'
import { createHash } from 'node:crypto'
import { test, expect } from '@playwright/test'
import { packageInputs, packageVersion } from './dsh-web.mjs'
import { launchViewerLane } from './lane.mjs'
import { closeTab } from './lifecycle.mjs'
import { UPGRADE_PROMPTS, upgradeReply } from './upgrade-reply.mjs'
import { installEvidence } from './upgrade-evidence.mjs'

const initial = packageInputs()
const next = process.env.UML_VIEWER_UPGRADE_MAIN || process.env.UML_VIEWER_UPGRADE_PLATFORM
  ? packageInputs({ packageMain: process.env.UML_VIEWER_UPGRADE_MAIN, packagePlatform: process.env.UML_VIEWER_UPGRADE_PLATFORM }) : null
const oldVersion = initial ? packageVersion(initial.main) : null
const nextVersion = next ? packageVersion(next.main) : null

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

test.setTimeout(300_000)

test('installed update keeps the busy vN resident until the UML tab is closed and reopened, then cleans the unused version', async ({ browser }) => {
  test.skip(process.platform !== 'win32', 'Busy executable acceptance is the Windows CI lane.')
  test.skip(!initial || !next, 'Supply both initial and upgrade main/platform tarball pairs.')
  const observed = {}
  const lane = await launchViewerLane(browser, {
    packageMain: initial.main, packagePlatform: initial.platform, reply: upgradeReply(observed, { main: next.main }),
  })
  const evidence = { versions: [oldVersion, nextVersion] }
  const home = path.join(lane.home, 'plugin-state')
  const oldBinary = path.join(home, oldVersion, 'uml-viewer.exe')
  const newBinary = path.join(home, nextVersion, 'uml-viewer.exe')
  try {
    let shop = await lane.openPanel('shop')
    expect(JSON.parse(await fs.readFile(path.join(home, 'current.json'), 'utf8')).version).toBe(oldVersion)
    const old = resident(oldBinary)
    expect(old).toHaveLength(1)
    const oldPid = old[0].ProcessId
    expect(alive(oldPid)).toBe(true)
    evidence.oldResident = old[0]
    evidence.oldBinaryVersion = binaryVersion(oldBinary)
    expect(evidence.oldBinaryVersion).toBe(`uml-viewer ${oldVersion}`)
    evidence.oldLeases = await leases(lane.home, oldVersion)
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
    await lane.sendPrompt(shop, UPGRADE_PROMPTS.remove)
    await answerApprovals(shop.page, shop.page.getByText(/^Isolated removal (applied|did not apply)$/).last())
    evidence.removal = observed.removal
    evidence.afterRemoval = await installEvidence(lane.home)
    expect(observed.removal?.application).toBe('applied')
    lane.installPlatform(next.platform)
    evidence.afterPlatform = await installEvidence(lane.home)
    expect(JSON.parse(await fs.readFile(path.join(home, 'current.json'), 'utf8')).version).toBe(oldVersion)
    await lane.sendPrompt(shop, UPGRADE_PROMPTS.install)
    await answerApprovals(shop.page, shop.page.getByText(/^Isolated install (applied|did not apply)$/).last())
    evidence.install = observed.install
    expect(observed.install?.application).toBe('applied')
    expect(observed.install).toMatchObject({ bundle: 'dsh-uml-viewer', version: nextVersion })
    evidence.afterInstall = await installEvidence(lane.home)
    evidence.hostOutput = lane.hostOutput()
    try {
      await expect.poll(async () => JSON.parse(await fs.readFile(path.join(home, 'current.json'), 'utf8')).version,
        { timeout: 10_000 }).toBe(nextVersion)
    } catch (error) {
      evidence.afterPoll = await installEvidence(lane.home)
      evidence.hostOutputAfterPoll = lane.hostOutput()
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
    expect(await leases(lane.home, oldVersion)).toEqual(evidence.oldLeases)
    expect(resident(newBinary)).toEqual([])
    evidence.retainedState = await shop.summary.innerText()
    await shop.page.mouse.move(1, 1)
    await expect.poll(() => canvasDigest(shop)).toBe(evidence.beforeCanvas)
    evidence.retainedCanvas = await canvasDigest(shop)
    evidence.newBinaryVersion = binaryVersion(newBinary)
    expect(evidence.newBinaryVersion).toBe(`uml-viewer ${nextVersion}`)
    await lane.screenshot(shop, 'upgrade-retains-old')
    await lane.wheel(shop, -2)
    await expect.poll(() => shop.summary.innerText()).not.toBe(state)
    await closeTab(shop)
    await expect(shop.page.locator('.dsh-uml-viewer-panel')).toHaveCount(0)
    await expect.poll(() => alive(oldPid), { timeout: 10_000 }).toBe(false)
    await expect.poll(() => leases(lane.home, oldVersion)).toEqual({})
    shop = await lane.reopenTab(shop)
    await expect(shop.summary).toHaveAttribute('data-kind', 'frame')
    await expect(shop.canvas).toBeVisible()
    await expect.poll(() => resident(newBinary).length).toBe(1)
    evidence.newResident = resident(newBinary)[0]
    expect(evidence.newResident.ProcessId).not.toBe(oldPid)
    evidence.newLeases = await leases(lane.home, nextVersion)
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
