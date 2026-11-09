import { chmodSync, constants, copyFileSync, existsSync, mkdirSync, readFileSync, statSync } from 'node:fs'
import { createRequire } from 'node:module'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

export interface Config {
  /** Absolute executable, or trusted argv prefix (for dev/test launchers). No shell. */
  binary?: string | readonly string[]
  /** Override the plugin's own data directory (defaults to `<DSH home>/dsh-uml-viewer`). */
  home?: string
}

/** The installed package directory that holds `fonts/`, `lib/` and the manifests. */
export function packageRoot(): string {
  let dir = path.dirname(fileURLToPath(import.meta.url))
  for (;;) {
    const manifest = path.join(dir, 'package.json')
    if (existsSync(manifest) && JSON.parse(readFileSync(manifest, 'utf8')).name === 'dsh-uml-viewer') return dir
    const parent = path.dirname(dir)
    if (parent === dir) throw new Error('Cannot locate the dsh-uml-viewer package root')
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
export function activationBinary(config: Config, home: string | undefined, platform: BinaryPlatform = process): string[] {
  const target = `${platform.platform}-${platform.arch}`
  if (!['win32-x64', 'linux-x64', 'darwin-arm64', 'darwin-x64'].includes(target)) {
    throw new Error(`Unsupported uml-viewer platform: ${platform.platform}/${platform.arch}; supported: win32/x64, linux/x64, darwin/arm64, darwin/x64`)
  }
  const root = platform.root ?? packageRoot()
  if (config.binary !== undefined || process.env.UML_VIEWER_BIN !== undefined) return binaryArgv(config, root)
  const manifest = JSON.parse(readFileSync(path.join(root, 'package.json'), 'utf8'))
  // Checkout development remains private and uses its local Cargo output.
  if (manifest.private === true && !manifest.optionalDependencies) return binaryArgv(config, root)
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
  const directory = path.join(home, 'versions', manifest.version)
  const binary = path.join(directory, name)
  mkdirSync(directory, { recursive: true })
  try {
    // Never overwrite a version already used by this or another host process.
    copyFileSync(source, binary, constants.COPYFILE_EXCL)
    if (platform.platform !== 'win32') chmodSync(binary, 0o755)
  } catch (error) {
    if ((error as NodeJS.ErrnoException).code !== 'EEXIST') throw error
  }
  return [binary]
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
