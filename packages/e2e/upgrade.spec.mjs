import fs from 'node:fs/promises'
import { openSync, closeSync } from 'node:fs'
import path from 'node:path'
import { spawnSync } from 'node:child_process'
import { createHash } from 'node:crypto'
import { test, expect } from '@playwright/test'
import { packageInputs } from './dsh-web.mjs'
import { launchViewerLane } from './lane.mjs'

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

// Read the actual public tool result, rather than assuming a scripted tool call
// succeeded. Messages transports may wrap the tool's JSON inside text blocks.
function managementResult(value) {
  if (typeof value === 'string') {
    try { return managementResult(JSON.parse(value)) } catch { return null }
  }
  if (!value || typeof value !== 'object') return null
  if (typeof value.application === 'string') return value
  for (const child of Object.values(value)) {
    const found = managementResult(child)
    if (found) return found
  }
  return null
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

// Public pinned tool -> PluginManager.removeBundle -> HMR.reconcileProfile.
// Listing is included before the destructive operation, as the tool requires.
function removalReply(observed) {
  return request => {
    if (!request.tools?.length) return null
    const messages = request.messages ?? []
    const start = messages.findIndex(message => JSON.stringify(message.content).includes('Perform isolated package upgrade'))
    if (start < 0) return null
    if (!request.tools.some(tool => tool.name === 'plugin_manager')) return { text: 'Missing public plugin_manager tool' }
    const tail = messages.slice(start)
    const uses = tail.flatMap(message => Array.isArray(message.content) ? message.content : [])
      .filter(part => part.type === 'tool_use' && part.name === 'plugin_manager')
    if (!uses.length) return { tool: { name: 'plugin_manager', input: { action: 'list_bundles', limit: 100 } } }
    const remove = uses.find(part => part.input?.action === 'remove_bundle')
    if (!remove) return { tool: { name: 'plugin_manager', input: { action: 'remove_bundle', target: 'dsh-uml-viewer' } } }
    const result = tail.flatMap(message => Array.isArray(message.content) ? message.content : [])
      .find(part => part.type === 'tool_result' && part.tool_use_id === remove.id)
    observed.removal = managementResult(result)
    return { text: observed.removal?.application === 'applied' ? 'Isolated removal applied' : 'Isolated removal did not apply' }
  }
}

// This case adds a real package replacement and two explicit panel lifecycle
// actions to the ordinary open, so it needs more than the lane's default 180 s.
test.setTimeout(300_000)

test('installed update keeps the busy vN resident until explicit Close/Open, then cleans the unused version', async ({ browser }) => {
  test.skip(process.platform !== 'win32', 'Busy executable acceptance is the Windows CI lane.')
  test.skip(!initial || !next, 'Supply both initial and upgrade main/platform tarball pairs.')
  const observed = {}
  const lane = await launchViewerLane(browser, {
    packageMain: initial.main, packagePlatform: initial.platform, reply: removalReply(observed),
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
    await lane.sendPrompt(shop, 'Perform isolated package upgrade')
    // The list and the removal each ask for the escalation they need. This is
    // the scratch application's own approval card, and a policy that already
    // allows the operation simply shows none.
    const applied = shop.page.getByText('Isolated removal applied', { exact: true }).last()
    await answerApprovals(shop.page, applied)
    await expect(applied).toBeVisible()
    expect(observed.removal?.application).toBe('applied')
    evidence.removal = observed.removal
    // A completed manager removal has dropped mappings. Re-add BOTH real
    // tarballs; the changed bundle selection is an explicit HMR watch trigger.
    // No host restart, page reload, private ctx or installed-file copying.
    lane.installPackages(next)
    await expect.poll(async () => JSON.parse(await fs.readFile(path.join(home, 'current.json'), 'utf8')).version,
      { timeout: 10_000 }).toBe('1.0.1')
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
