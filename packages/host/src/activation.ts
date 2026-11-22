import { appendFileSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs'
import path from 'node:path'

export const ACTIVATION_LOG = 'activations.jsonl'
export const ACTIVATION_LOG_LIMIT = 64

export interface ActivationRecord {
  readonly version: string | null
  readonly source: 'pinned' | 'unpinned'
  readonly root: string
  readonly argv: readonly string[]
}

export function recordActivation(home: string | undefined, record: ActivationRecord, now = new Date()): void {
  if (!home) return
  try {
    mkdirSync(home, { recursive: true })
    const file = path.join(home, ACTIVATION_LOG)
    appendFileSync(file, `${JSON.stringify({ at: now.toISOString(), pid: process.pid, ...record })}\n`)
    const kept = readFileSync(file, 'utf8').split('\n').filter(line => line.trim() !== '')
    if (kept.length > ACTIVATION_LOG_LIMIT) {
      writeFileSync(file, `${kept.slice(-ACTIVATION_LOG_LIMIT).join('\n')}\n`)
    }
  } catch {
  }
}
