import { mkdir, readFile, rename, writeFile } from 'node:fs/promises'
import { createHash, randomUUID } from 'node:crypto'
import path from 'node:path'
import type { SessionState } from '@dsh-uml-viewer/contracts'
import { validateSessionState } from '@dsh-uml-viewer/contracts'

const SAFE_ID = /[^A-Za-z0-9._-]/g

export function pluginHome(configHome: string | undefined, configured: (() => string | undefined) | undefined): string | undefined {
  if (configHome) return path.resolve(configHome)
  const fromHost = configured?.()
  if (fromHost) return path.resolve(fromHost)
  const dshHome = process.env.DSH_HOME?.trim()
  if (dshHome) return path.resolve(dshHome, 'dsh-uml-viewer')
  return undefined
}

export class SessionStates {
  private readonly directory: string | undefined
  private readonly writes = new Map<string, Promise<void>>()

  constructor(home: string | undefined) {
    this.directory = home ? path.join(home, 'sessions') : undefined
  }

  private file(sessionId: string): string | undefined {
    if (!this.directory) return undefined
    return path.join(this.directory, `${sessionId.replace(SAFE_ID, '_')}.json`)
  }

  async read(sessionId: string): Promise<SessionState | null> {
    const file = this.file(sessionId)
    if (!file) return null
    try {
      const record = JSON.parse(await readFile(file, 'utf8')) as Record<string, unknown>
      const state = validateSessionState(record)
      if (state.diagram && (!state.loadedDiagram
        || await diagramDigest(state.diagram) !== state.loadedDiagram.sha256)) {
        return { ...state, diagram: null }
      }
      return state
    } catch {
      return null
    }
  }

  write(sessionId: string, state: SessionState): Promise<void> {
    const previous = this.writes.get(sessionId) ?? Promise.resolve()
    const next = previous.catch(() => {}).then(() => this.publish(sessionId, state))
    this.writes.set(sessionId, next)
    void next.finally(() => {
      if (this.writes.get(sessionId) === next) this.writes.delete(sessionId)
    }).catch(() => {})
    return next
  }

  private async publish(sessionId: string, state: SessionState): Promise<void> {
    const file = this.file(sessionId)
    if (!file || !this.directory) return
    await mkdir(this.directory, { recursive: true })
    const temporary = `${file}.${process.pid}.${randomUUID()}.tmp`
    await writeFile(temporary, `${JSON.stringify(state)}\n`)
    await rename(temporary, file)
  }
}

async function diagramDigest(diagram: string): Promise<string | null> {
  try {
    return createHash('sha256').update(await readFile(diagram)).digest('hex')
  } catch {
    return null
  }
}
