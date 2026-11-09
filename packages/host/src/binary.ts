import { existsSync, readFileSync, statSync } from 'node:fs'
import { createRequire } from 'node:module'
import path from 'node:path'
import { fileURLToPath } from 'node:url'
import { pinVersion } from './versions.js'

const unpinned = (argv: string[], root: string): ActivatedBinary => ({
  argv, version: null, source: 'unpinned', root, retain: () => () => {}, release: () => {},
})

export interface Config {
  /** Absolute executable, or trusted argv prefix (for dev/test launchers). No shell. */
  binary?: string | readonly string[]
  /** Override the plugin's own data directory (defaults to `<DSH home>/dsh-uml-viewer`). */
  home?: string
}

/**
 * The package directory that holds this module: `fonts/`, `lib/` and the manifests.
 *
 * The walk never leaves the `node_modules` entry that holds an installed
 * module. Above it lies whatever project hosts the profile, and a manifest
 * named dsh-uml-viewer there (CI14: a scratch DSH home inside a checkout) is
 * not this module's package; adopting it would run that checkout's build.
 */
export function packageRoot(moduleFile: string = fileURLToPath(import.meta.url)): string {
  let dir = path.dirname(moduleFile)
  for (;;) {
    const manifest = path.join(dir, 'package.json')
    if (existsSync(manifest) && JSON.parse(readFileSync(manifest, 'utf8')).name === 'dsh-uml-viewer') return dir
    const parent = path.dirname(dir)
    if (parent === dir || path.basename(parent) === 'node_modules') {
      throw new Error(`Cannot locate the dsh-uml-viewer package root of ${moduleFile}`)
    }
    dir = parent
  }
}

export interface BinaryPlatform {
  platform: string
  arch: string
  /** Main package directory; defaults to the installed plugin. */
  root?: string
}

/** Resolve once at activation. Callers retain this argv for the plugin lifetime. */
export function activationBinary(config: Config, home: string | undefined, platform: BinaryPlatform = process): ActivatedBinary {
  const target = `${platform.platform}-${platform.arch}`
  if (!['win32-x64', 'linux-x64', 'darwin-arm64', 'darwin-x64'].includes(target)) {
    throw new Error(`Unsupported uml-viewer platform: ${platform.platform}/${platform.arch}; supported: win32/x64, linux/x64, darwin/arm64, darwin/x64`)
  }
  const root = platform.root ?? packageRoot()
  // Trusted overrides and checkout builds are not versioned copies: nothing to lease.
  if (config.binary !== undefined || process.env.UML_VIEWER_BIN !== undefined) return unpinned(binaryArgv(config, root), root)
  const manifest = JSON.parse(readFileSync(path.join(root, 'package.json'), 'utf8'))
  // Checkout development remains private and uses its local Cargo output.
  if (manifest.private === true && !manifest.optionalDependencies) return unpinned(binaryArgv(config, root), root)
  const dependency = `@dsh-uml-viewer/${target}`
  if (manifest.optionalDependencies?.[dependency] !== manifest.version) {
    throw new Error(`Packaged dsh-uml-viewer is missing the exact ${dependency}@${manifest.version} optional dependency declaration`)
  }
  if (typeof manifest.version !== 'string' || !/^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?$/.test(manifest.version)) {
    throw new Error('Packaged dsh-uml-viewer has an invalid version')
  }
  let source: string
  const name = platform.platform === 'win32' ? 'uml-viewer.exe' : 'uml-viewer'
  try {
    const dependencyManifest = createRequire(path.join(root, 'package.json')).resolve(`${dependency}/package.json`)
    const installed = JSON.parse(readFileSync(dependencyManifest, 'utf8'))
    if (installed.version !== manifest.version) throw new Error(`installed version is ${installed.version}`)
    source = path.join(path.dirname(dependencyManifest), 'bin', name)
    if (!statSync(source).isFile()) throw new Error('executable payload is not a file')
  } catch (error) {
    throw new Error(`Cannot load packaged binary ${dependency}@${manifest.version}: ${error instanceof Error ? error.message : String(error)}. Reinstall dsh-uml-viewer with optional dependencies enabled.`)
  }
  if (!home) throw new Error('Cannot pin packaged uml-viewer binary: configure home or provide dshHomePath / DSH_HOME')
  // #32: `<home>/<version>/`, verified and leased for this activation. Every
  // resident and tool call of the activation runs this copy, so an update
  // only reaches panels opened by a later (fresh) activation.
  const pinned = pinVersion(home, manifest.version, source, name, platform.platform)
  return { argv: [pinned.binary], version: manifest.version, source: 'pinned', root, retain: pinned.retain, release: pinned.release }
}

/** The executable an activation pinned, and the hold it keeps on that version. */
export interface ActivatedBinary {
  readonly argv: string[]
  /** The manifest version this activation pinned, or null for an unversioned executable. */
  readonly version: string | null
  /** Whether `argv` is a pinned version copy or an override/checkout build that pins nothing. */
  readonly source: 'pinned' | 'unpinned'
  /** The package directory the executable was resolved from. */
  readonly root: string
  /** A resident holds the pinned version until its actual process exit. */
  retain(): () => void
  /** Drop the activation's version lease after its residents stopped. */
  release(): void
}

export function binaryArgv(config: Config, root?: string): string[] {
  const configured = config.binary ?? process.env.UML_VIEWER_BIN
  if (configured !== undefined) {
    const argv = typeof configured === 'string' ? [configured] : [...configured]
    if (!argv.length || argv.some(v => typeof v !== 'string' || v.includes('\0')) || !path.isAbsolute(argv[0]!)) {
      throw new Error('binary / UML_VIEWER_BIN must name an absolute executable (not a shell command)')
    }
    return argv
  }
  const directory = root ?? packageRoot()
  const name = process.platform === 'win32' ? 'uml-viewer.exe' : 'uml-viewer'
  for (const profile of ['debug', 'release']) {
    const binary = path.join(directory, 'target', profile, name)
    if (existsSync(binary)) return [binary]
  }
  throw new Error('No uml-viewer binary: set UML_VIEWER_BIN to an absolute executable or build target/debug/uml-viewer (target/release is also checked)')
}
