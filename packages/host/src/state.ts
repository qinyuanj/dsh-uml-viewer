import { mkdir, readFile, rename, writeFile } from 'node:fs/promises'
import path from 'node:path'
import type { SessionState } from '@dsh-uml-viewer/contracts'
import { validateSessionState } from '@dsh-uml-viewer/contracts'

const SAFE_ID = /[^A-Za-z0-9._-]/g

/** The plugin's own data directory under the DSH home. */
export function pluginHome(configHome: string | undefined, configured: (() => string | undefined) | undefined): string | undefined {
  if (configHome) return path.resolve(configHome)
  const fromHost = configured?.()
  if (fromHost) return path.resolve(fromHost)
  const dshHome = process.env.DSH_HOME?.trim()
  if (dshHome) return path.resolve(dshHome, 'dsh-uml-viewer')
  return undefined
}

/**
 * Per-session view/discussion state, in the plugin's own directory. DSH's
 * session log stays untouched: this is state the log never carries.
 */
export class SessionStates {
  private readonly directory: string | undefined

  constructor(home: string | undefined) {
    this.directory = home ? path.join(home, 'sessions') : undefined
  }

  private file(sessionId: string): string | undefined {
    if (!this.directory) return undefined
    return path.join(this.directory, `${sessionId.replace(SAFE_ID, '_')}.json`)
  }

  /** Read the stored state; a missing or malformed file is simply "no state". */
  async read(sessionId: string): Promise<SessionState | null> {
    const file = this.file(sessionId)
    if (!file) return null
    try {
      return validateSessionState(JSON.parse(await readFile(file, 'utf8')))
    } catch {
      return null
    }
  }

  /** Write atomically (temporary file plus rename) so a crash cannot truncate it. */
  async write(sessionId: string, state: SessionState): Promise<void> {
    const file = this.file(sessionId)
    if (!file || !this.directory) return
    await mkdir(this.directory, { recursive: true })
    const temporary = `${file}.${process.pid}.tmp`
    await writeFile(temporary, `${JSON.stringify(state)}\n`)
    await rename(temporary, file)
  }
}
