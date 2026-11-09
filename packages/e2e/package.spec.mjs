import fs from 'node:fs/promises'
import path from 'node:path'
import { test, expect } from '@playwright/test'
import { packageInputs } from './dsh-web.mjs'
import { launchViewerLane } from './lane.mjs'

const packages = packageInputs()

// Read file bytes and nanosecond mtimes, including directory membership, so a
// delete/recreate or a same-content rewrite cannot pass preservation acceptance.
async function protectedSnapshot(projects) {
  const entries = {}
  async function visit(file, key) {
    const stat = await fs.lstat(file, { bigint: true })
    if (stat.isSymbolicLink()) throw new Error(`unexpected sample symlink: ${file}`)
    if (stat.isDirectory()) {
      const children = (await fs.readdir(file)).sort()
      entries[key] = { mtimeNs: String(stat.mtimeNs), children }
      for (const child of children) await visit(path.join(file, child), `${key}/${child}`)
    } else {
      entries[key] = { mtimeNs: String(stat.mtimeNs), bytes: (await fs.readFile(file)).toString('base64') }
    }
  }
  for (const [name, root] of Object.entries(projects)) {
    for (const relative of ['policy.edn', 'proposals', '.metrics']) {
      await visit(path.join(root, relative), `${name}/${relative}`)
    }
  }
  return entries
}

async function seedPreservedData(projects) {
  for (const root of Object.values(projects)) {
    await fs.mkdir(path.join(root, 'proposals', 'saved'), { recursive: true })
    await fs.writeFile(path.join(root, 'proposals', 'saved', 'proposal.edn'),
      '; User-authored proposal; keep comments and formatting.\n{:name "Saved proposal" :omit []}\n')
    await fs.mkdir(path.join(root, '.metrics', 'acceptance'), { recursive: true })
    await fs.writeFile(path.join(root, '.metrics', 'acceptance', 'snapshot.bin'),
      Buffer.from([0, 1, 13, 10, 255, 128, 42]))
  }
}

test('installs packaged tarballs, displays a real panel, and preserves project data on CLI removal', async ({ browser }) => {
  test.skip(!packages, 'Set absolute UML_VIEWER_PACKAGE_MAIN and UML_VIEWER_PACKAGE_PLATFORM tarballs to run packaged acceptance.')
  let before
  const lane = await launchViewerLane(browser, {
    packageMain: packages.main,
    packagePlatform: packages.platform,
    async prepareProjects(projects) {
      await seedPreservedData(projects)
      before = await protectedSnapshot(projects)
    },
  })
  try {
    const shop = await lane.openPanel('shop')
    await expect(shop.panel).toBeVisible()
    await expect(shop.canvas).toBeVisible()
    await expect(shop.summary).toHaveAttribute('data-kind', 'frame')
    await expect(shop.summary).toContainText('Shop')
    await expect(shop.summary).toContainText('Discussion: source diagram')
    expect(shop.problems).toEqual([])
    await lane.screenshot(shop, 'packaged-shop')
    await lane.uninstall()
    const after = await protectedSnapshot(lane.projects)
    await fs.writeFile(path.join(lane.home, 'package-preservation.json'), JSON.stringify({ before, after }, null, 2))
    expect(after).toEqual(before)
  } finally {
    await lane.close()
  }
})
