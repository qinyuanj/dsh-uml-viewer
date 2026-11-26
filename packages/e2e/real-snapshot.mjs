import fs from 'node:fs/promises'
import path from 'node:path'
import { execFileSync } from 'node:child_process'
import { panelCamera, panelViewport } from './canvas.mjs'
import { ROOT, viewerBinary } from './dsh-web.mjs'

/**
 * The repository's own committed quality snapshot, fed to the panel verbatim
 * (#153). The synthetic projects in `projects.mjs` carry hand-written
 * snapshots of one or two entries; this one is the real `.metrics` directory
 * the maintainer refresh writes, so the join between real quality data and the
 * real namespace tree is exercised end to end.
 *
 * The project root is a copy of the repository's own source tree, because the
 * panel must write its IR into the project (`:out "diagram.edn"`) and the
 * lane's scratch home is the only place a run may write. What is copied:
 *
 * - every `crates/<crate>/src` and `packages/<pkg>/src` root the policy
 *   names, plus the manifests the Rust and TypeScript scanners read;
 * - `.metrics` **verbatim** — never regenerated, never edited (the ticket's
 *   input, not an output);
 * - `examples/uml-viewer.policy.edn` with one line changed: `:out` becomes
 *   `diagram.edn` at the project root, the path the lane's mock model displays
 *   and the path the locator frame reads. Source roots, prefixes, levels and
 *   proposals are the committed ones, so the IR is the real diagram.
 *
 * The project is named after the policy's own `:title`, which is what the
 * panel prints in its summary and what `launchViewerLane` expects there.
 */
export const REAL_PROJECT = 'UML viewer'
const POLICY = path.join(ROOT, 'examples/uml-viewer.policy.edn')
const METRICS = path.join(ROOT, '.metrics')
const PACKAGES = ['contracts', 'host', 'client']
const ROOT_FILES = ['Cargo.toml', 'Cargo.lock', 'rust-toolchain.toml']

async function copy(from, to) {
  await fs.cp(from, to, { recursive: true })
}

async function isFile(file) {
  return fs.stat(file).then(s => s.isFile()).catch(() => false)
}

/**
 * `prepareProjects` hook for `launchViewerLane`: adds the real project next to
 * the synthetic ones. Throws when the committed snapshot is absent, so a lane
 * run can never silently degrade into "no data everywhere, every assertion
 * vacuously true".
 */
export async function prepareRealProjects(projects) {
  return build(projects, true)
}

/**
 * The same project with the snapshot **removed**: the negative case, which
 * shows the positive assertions are not always true.
 */
export async function prepareRealProjectsWithoutSnapshot(projects) {
  return build(projects, false)
}

async function build(projects, withSnapshot) {
  const crap = path.join(METRICS, 'crap.edn')
  const dry = path.join(METRICS, 'dry.edn')
  for (const file of [crap, dry]) {
    if (!await isFile(file)) {
      throw new Error(`the repository's committed quality snapshot is missing: ${file}. ` +
        'This acceptance drives the panel from the real .metrics directory; ' +
        'restore it from the maintainer refresh instead of skipping the run.')
    }
  }
  const home = path.dirname(Object.values(projects)[0])
  const root = path.join(home, REAL_PROJECT)
  await fs.mkdir(root, { recursive: true })
  await copy(path.join(ROOT, 'crates'), path.join(root, 'crates'))
  for (const name of PACKAGES) {
    await fs.mkdir(path.join(root, 'packages', name), { recursive: true })
    await copy(path.join(ROOT, 'packages', name, 'src'), path.join(root, 'packages', name, 'src'))
    await fs.copyFile(path.join(ROOT, 'packages', name, 'package.json'), path.join(root, 'packages', name, 'package.json'))
  }
  for (const name of ROOT_FILES) {
    await fs.copyFile(path.join(ROOT, name), path.join(root, name)).catch(() => {})
  }
  // The resident finds the snapshot by walking up for a `.metrics` directory
  // (crates/viewer-session/src/metrics.rs). An empty directory pins that search
  // at the project root, so the run cannot fall back to the repository's own
  // snapshot through the scratch path, and the panel really sees no data.
  if (withSnapshot) await copy(METRICS, path.join(root, '.metrics'))
  else await fs.mkdir(path.join(root, '.metrics'), { recursive: true })
  const policy = await fs.readFile(POLICY, 'utf8')
  const out = ':out "examples/uml-viewer.edn"'
  if (!policy.includes(out)) {
    throw new Error(`${POLICY} no longer declares ${out}; the lane needs the IR at the project root`)
  }
  await fs.writeFile(path.join(root, 'policy.edn'), policy.replace(out, ':out "diagram.edn"'))
  projects[REAL_PROJECT] = root
  return root
}

/**
 * A flattened copy of a project's snapshot: `crap.edn`, `dry.edn` and every
 * file under `.metrics/mutate` (at any depth) gathered into one `mutate/` directory.
 *
 * This exists only to *locate* boxes. `uml-viewer scene --frame` reads mutation
 * files from `.metrics/mutate` without recursing (crates/scene/src/cli.rs,
 * `load_snapshots`), while the resident the panel talks to walks the whole tree
 * (crates/viewer-session/src/metrics.rs). On the repository's own snapshot the
 * two therefore disagree about which classes have data, and the scene lays the
 * diagram out with shorter, fewer boxes than the panel draws, so a box read
 * from the plain oracle frame is not where the panel painted it. Feeding the
 * locator frame the same data in the flat shape its reader understands makes the
 * layout agree with the panel. It changes nothing the spec asserts: every
 * assertion below is made on the panel itself, and `readSnapshot` reads the
 * project's untouched `.metrics`.
 */
export async function flattenSnapshot(root, into) {
  const metrics = path.join(root, '.metrics')
  const mutate = path.join(metrics, 'mutate')
  // The scene CLI reads <metrics-root>/.metrics/..., so mirror that shape.
  const out = path.join(into, '.metrics')
  await fs.mkdir(path.join(out, 'mutate'), { recursive: true })
  for (const file of ['crap.edn', 'dry.edn']) {
    await fs.copyFile(path.join(metrics, file), path.join(out, file)).catch(() => {})
  }
  const walk = async dir => {
    for (const entry of await fs.readdir(dir, { withFileTypes: true }).catch(() => [])) {
      const full = path.join(dir, entry.name)
      if (entry.isDirectory()) await walk(full)
      else if (entry.name.endsWith('.edn')) {
        const flat = path.relative(mutate, full).split(path.sep).join('_')
        await fs.copyFile(full, path.join(out, 'mutate', flat))
      }
    }
  }
  await walk(mutate)
  return into
}

/** The scene frame for the panel's current view, read from a chosen metrics root. */
export async function locatorFrame(panel, { root, metricsRoot, focus = null }) {
  const camera = await panelCamera(panel)
  const viewport = await panelViewport(panel)
  const output = path.join(metricsRoot, 'locator.frame.json')
  execFileSync(viewerBinary(), [
    'scene', '--frame',
    '--ir', path.join(root, 'diagram.edn'),
    '--metrics-root', metricsRoot,
    '--wide',
    ...(focus ? ['--focus', `[${focus}]`] : []),
    '--width', String(viewport.width), '--height', String(viewport.height),
    '--zoom', String(camera.zoom), '--camera', `${camera.x},${camera.y}`,
    '--extra', '{}',
    '--output', output,
  ], { stdio: ['ignore', 'pipe', 'pipe'] })
  return JSON.parse(await fs.readFile(output, 'utf8'))
}

const normalize = ns => ns.replace(/::/g, '.')

/**
 * What the copied snapshot measures, read back from the project the panel saw:
 * the CRAP entries per namespace, the namespaces the differential mutation
 * snapshot covers, and the duplicate candidates. Everything the spec asserts is
 * derived from these files at run time, so a rename or a refresh moves the
 * expectation with the data instead of breaking a hard-coded count.
 */
export async function readSnapshot(root) {
  const crap = await fs.readFile(path.join(root, '.metrics/crap.edn'), 'utf8')
  const entries = new Map()
  for (const m of crap.matchAll(/:name "([^"]+)", :namespace "([^"]+)", :complexity (\d+), :coverage ([\d.]+), :crap ([\d.]+)/g)) {
    const ns = normalize(m[2])
    if (!entries.has(ns)) entries.set(ns, [])
    entries.get(ns).push({ name: m[1], complexity: Number(m[3]), coverage: Number(m[4]), crap: Number(m[5]) })
  }
  const mutate = new Map()
  const walk = async dir => {
    for (const entry of await fs.readdir(dir, { withFileTypes: true }).catch(() => [])) {
      const full = path.join(dir, entry.name)
      if (entry.isDirectory()) await walk(full)
      else if (entry.name.endsWith('.edn')) {
        const text = await fs.readFile(full, 'utf8')
        const ns = /:namespace "([^"]+)"/.exec(text)
        if (!ns) continue
        const sites = [...text.matchAll(/"(?:[^"\\]|\\.)*" :(killed|survived)/g)].map(m => {
          const member = /defn-?\/([^"\\]+)/.exec(m[0])
          return { member: member ? member[1] : null, outcome: m[1] }
        })
        const key = normalize(ns[1])
        if (!mutate.has(key)) mutate.set(key, { file: path.relative(root, full), sites: [] })
        mutate.get(key).sites.push(...sites)
      }
    }
  }
  await walk(path.join(root, '.metrics/mutate'))
  const dry = await fs.readFile(path.join(root, '.metrics/dry.edn'), 'utf8')
  const namespaces = [...dry.matchAll(/:namespace "([^"]+)"/g)].map(m => normalize(m[1]))
  const outcome = (ns, member) => {
    const sites = mutate.get(ns)?.sites ?? []
    const of = sites.filter(s => s.member !== null && (member === undefined || s.member === member))
    return { killed: of.filter(s => s.outcome === 'killed').length, survived: of.filter(s => s.outcome === 'survived').length }
  }
  return {
    entries,
    mutate,
    /** Killed and survived mutants for one namespace, or for one of its members. */
    outcome,
    /** A namespace the snapshot measures in both metrics: CRAP and mutation. */
    measured: ns => entries.has(ns) && mutate.has(ns),
    dry: { candidates: (dry.match(/:score /g) ?? []).length, namespaces },
  }
}

/**
 * The IR the panel itself generated (`diagram.edn` in the project root), parsed
 * for class ids, their display names and their parent/child shape. The spec
 * uses it to know which box it clicked and whether a box opens a card or drills
 * one level deeper.
 */
export async function readDocument(root) {
  const text = await fs.readFile(path.join(root, 'diagram.edn'), 'utf8')
  const classes = new Map()
  for (const m of text.matchAll(/:id :([\w.\-]+) :lang :\w+ :level (\d+) :name "([^"]+)" :ns "([^"]+)"/g)) {
    classes.set(m[1], { id: m[1], level: Number(m[2]), name: m[3], ns: m[4] })
  }
  return {
    classes,
    /** A class with children is drawn as a container: clicking it drills in. */
    isLeaf: id => ![...classes.keys()].some(other => other.startsWith(`${id}.`)),
  }
}