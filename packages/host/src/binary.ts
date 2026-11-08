import { existsSync, readFileSync } from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

export interface Config {
  /** Absolute executable, or trusted argv prefix (for dev/test launchers). No shell. */
  binary?: string | readonly string[]
}

function packageRoot(): string {
  let dir = path.dirname(fileURLToPath(import.meta.url))
  for (;;) {
    const manifest = path.join(dir, 'package.json')
    if (existsSync(manifest) && JSON.parse(readFileSync(manifest, 'utf8')).name === 'dsh-uml-viewer') return dir
    const parent = path.dirname(dir)
    if (parent === dir) throw new Error('Cannot locate the dsh-uml-viewer package root')
    dir = parent
  }
}

export function binaryArgv(config: Config, pluginRoot?: string): string[] {
  const configured = config.binary ?? process.env.UML_VIEWER_BIN
  if (configured !== undefined) {
    const argv = typeof configured === 'string' ? [configured] : [...configured]
    if (!argv.length || argv.some(v => typeof v !== 'string' || v.includes('\0')) || !path.isAbsolute(argv[0]!)) {
      throw new Error('binary / UML_VIEWER_BIN must name an absolute executable (not a shell command)')
    }
    return argv
  }
  const root = pluginRoot ?? packageRoot()
  const name = process.platform === 'win32' ? 'uml-viewer.exe' : 'uml-viewer'
  for (const profile of ['debug', 'release']) {
    const binary = path.join(root, 'target', profile, name)
    if (existsSync(binary)) return [binary]
  }
  throw new Error('No uml-viewer binary: set UML_VIEWER_BIN to an absolute executable or build target/debug/uml-viewer (target/release is also checked)')
}
