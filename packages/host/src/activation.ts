import { appendFileSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs'
import path from 'node:path'

/**
 * One line per plugin activation, appended to the plugin home. #32 made the
 * activation, not the installation, own the executable, so the first question
 * of any upgrade report is which build an activation runs and whether it could
 * pin one at all: an override or a checkout build is unversioned and pins
 * nothing, which otherwise looks exactly like a pin that never happened.
 *
 * The journal is a diagnostic, never state: it is best-effort, bounded, and no
 * activation may fail because of it.
 */
export const ACTIVATION_LOG = 'activations.jsonl'
/** Older activations are dropped so the journal stays a bounded tail. */
export const ACTIVATION_LOG_LIMIT = 64

export interface ActivationRecord {
  /** The version this activation pinned, or null when it runs an unversioned executable. */
  readonly version: string | null
  /** Whether the executable is a pinned version copy or an override/dev build. */
  readonly source: 'pinned' | 'unpinned'
  /** The package directory the executable was resolved from. */
  readonly root: string
  /** The exact argv this activation pinned; `argv[0]` is the executable. */
  readonly argv: readonly string[]
}

/** Append this activation to `<home>/activations.jsonl`. Never throws. */
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
    // A diagnostic must not change whether the panel can start.
  }
}
