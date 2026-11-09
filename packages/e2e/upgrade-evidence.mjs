import fs from 'node:fs/promises'
import path from 'node:path'

const VERSION = /^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?$/

async function readVersion(file) {
  try {
    return JSON.parse(await fs.readFile(file, 'utf8')).version ?? null
  } catch (error) {
    return error.code === 'ENOENT' ? null : `unreadable: ${error.code ?? error.message}`
  }
}

/**
 * Deciding facts for an installed upgrade that answered `applied` but left
 * `current.json` alone (CI12). Read-only and outside the Host: every plugin
 * version store directly under the scratch DSH home (the lane's configured
 * `plugin-state`, or a default `<DSH_HOME>/dsh-uml-viewer` if the reconciled
 * row lost its `home`), where the profile's links point now, and every
 * dsh-uml-viewer copy the virtual store still holds. A stale 1.0.0 copy that
 * survives removal is what a cached resolution of the old link re-imports.
 */
export async function installEvidence(home, { profile = 'web', platform = `@dsh-uml-viewer/${process.platform}-${process.arch}` } = {}) {
  const stores = []
  for (const dir of (await fs.readdir(home, { withFileTypes: true }).catch(() => [])).filter(entry => entry.isDirectory())) {
    const directory = path.join(home, dir.name)
    const current = await readVersion(path.join(directory, 'current.json'))
    if (current === null) continue
    const versions = (await fs.readdir(directory, { withFileTypes: true }))
      .filter(entry => entry.isDirectory() && VERSION.test(entry.name)).map(entry => entry.name).sort()
    stores.push({ dir: dir.name, current, versions })
  }
  stores.sort((a, b) => a.dir.localeCompare(b.dir))

  const modules = path.join(home, 'profiles', profile, 'node_modules')
  const links = []
  for (const name of ['dsh-uml-viewer', platform]) {
    const link = path.join(modules, ...name.split('/'))
    try {
      const target = await fs.realpath(link)
      links.push({ name, version: await readVersion(path.join(target, 'package.json')), target: path.relative(home, target) })
    } catch (error) {
      links.push({ name, error: error.code ?? error.message })
    }
  }

  const virtualStore = []
  const store = path.join(modules, '.pnpm')
  for (const entry of (await fs.readdir(store).catch(() => [])).sort()) {
    if (!entry.startsWith('dsh-uml-viewer@') && !entry.startsWith('@dsh-uml-viewer+')) continue
    const name = entry.startsWith('@') ? entry.slice(0, entry.indexOf('@', 1)).replace('+', '/') : 'dsh-uml-viewer'
    virtualStore.push({ entry, version: await readVersion(path.join(store, entry, 'node_modules', ...name.split('/'), 'package.json')) })
  }
  return { stores, links, virtualStore }
}
