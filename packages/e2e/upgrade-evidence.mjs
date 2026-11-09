import fs from 'node:fs/promises'
import path from 'node:path'

const VERSION = /^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?$/
const BINARY = process.platform === 'win32' ? 'uml-viewer.exe' : 'uml-viewer'
/** Enough activations to cover one upgrade; the journal itself keeps more. */
const ACTIVATIONS_KEPT = 20

async function readVersion(file) {
  try {
    return JSON.parse(await fs.readFile(file, 'utf8')).version ?? null
  } catch (error) {
    return error.code === 'ENOENT' ? null : `unreadable: ${error.code ?? error.message}`
  }
}

async function readJson(file) {
  try {
    return JSON.parse(await fs.readFile(file, 'utf8'))
  } catch {
    return undefined
  }
}

/** The newest activation records a store holds, oldest first; unreadable lines stay visible. */
async function readActivations(directory) {
  try {
    const text = await fs.readFile(path.join(directory, 'activations.jsonl'), 'utf8')
    return text.split('\n').filter(line => line.trim() !== '').slice(-ACTIVATIONS_KEPT).map(line => {
      try {
        return JSON.parse(line)
      } catch {
        return { unreadable: line }
      }
    })
  } catch {
    return []
  }
}

/** The versions a store holds live leases for, so a pin that happened is visible even before current.json moves. */
async function readLeases(directory) {
  const leases = {}
  for (const entry of (await fs.readdir(path.join(directory, 'leases'), { withFileTypes: true }).catch(() => []))
    .filter(item => item.isDirectory()).map(item => item.name).sort()) {
    leases[entry] = (await fs.readdir(path.join(directory, 'leases', entry)).catch(() => [])).sort()
  }
  return leases
}

/** What an activation would read from the installed package: its manifest, and the checkout fallback. */
async function readManifest(directory) {
  const manifest = await readJson(path.join(directory, 'package.json'))
  if (manifest === undefined) return { error: 'ENOENT' }
  const devBinaries = []
  for (const profile of ['debug', 'release']) {
    try {
      await fs.access(path.join(directory, 'target', profile, BINARY))
      devBinaries.push(profile)
    } catch {
      // This checkout profile does not exist.
    }
  }
  return {
    version: typeof manifest.version === 'string' ? manifest.version : null,
    private: manifest.private === true,
    main: typeof manifest.main === 'string' ? manifest.main : null,
    optionalDependencies: manifest.optionalDependencies ?? null,
    devBinaries,
  }
}

/**
 * Deciding facts for an installed upgrade that answered `applied` but left
 * `current.json` alone (CI12, CI13). Read-only and outside the Host:
 *
 * - every plugin version store directly under the scratch DSH home (the lane's
 *   configured `plugin-state`, or a default `<DSH_HOME>/dsh-uml-viewer` if the
 *   reconciled row lost its `home`), with its version directories, its live
 *   leases and its own activation journal — `source: "unpinned"` is an
 *   activation that runs an override or checkout build and pins nothing, which
 *   looks identical to a pin that never happened when only `current.json` is read;
 * - where the profile's main and platform links point now, and every
 *   dsh-uml-viewer copy the virtual store still holds;
 * - the installed main package manifest and its checkout binaries, which decide
 *   whether `activationBinary` reads a packaged manifest (pins) or a private
 *   checkout manifest (never pins).
 */
export async function installEvidence(home, { profile = 'web', platform = `@dsh-uml-viewer/${process.platform}-${process.arch}` } = {}) {
  const stores = []
  for (const dir of (await fs.readdir(home, { withFileTypes: true }).catch(() => [])).filter(entry => entry.isDirectory())) {
    const directory = path.join(home, dir.name)
    const current = await readVersion(path.join(directory, 'current.json'))
    if (current === null) continue
    const versions = (await fs.readdir(directory, { withFileTypes: true }).catch(() => []))
      .filter(entry => entry.isDirectory() && VERSION.test(entry.name)).map(entry => entry.name).sort()
    stores.push({ dir: dir.name, current, versions,
      leases: await readLeases(directory), activations: await readActivations(directory) })
  }
  stores.sort((a, b) => a.dir.localeCompare(b.dir))

  const modules = path.join(home, 'profiles', profile, 'node_modules')
  const links = []
  let main = { error: 'ENOENT' }
  for (const name of ['dsh-uml-viewer', platform]) {
    const link = path.join(modules, ...name.split('/'))
    try {
      const target = await fs.realpath(link)
      links.push({ name, version: await readVersion(path.join(target, 'package.json')), target: path.relative(home, target) })
      if (name === 'dsh-uml-viewer') main = await readManifest(target)
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
  return { stores, links, virtualStore, main }
}
