import type { QualityOperation, QualityToolResult } from '@dsh-uml-viewer/contracts'

export interface QualityRecord {
  operation: QualityOperation
  validResult: boolean
  exitCode: number | null
  reason: string | null
}

/**
 * Per-session FIFO for quality calls plus the latest quality outcome.
 *
 * `uml_ir` is refused only after a quality call in that session ended without
 * `validResult: true`; with no quality call, IR remains available for requests
 * that need no refresh (for example :omit or a proposal). Every completed
 * quality invocation replaces the previous one, including refusals and
 * malformed invocations.
 */
export class QualityGate {
  private readonly latest = new Map<string, QualityRecord>()
  private readonly tails = new Map<string, Promise<void>>()

  /** Queue work behind earlier quality calls in the same session, in call order. */
  run<T>(sessionId: string, task: () => Promise<T>): Promise<T> {
    const previous = this.tails.get(sessionId) ?? Promise.resolve()
    const result = previous.then(task, task)
    const tail = result.then(() => undefined, () => undefined)
    this.tails.set(sessionId, tail)
    void tail.then(() => {
      if (this.tails.get(sessionId) === tail) this.tails.delete(sessionId)
    })
    return result
  }

  record(sessionId: string, result: QualityToolResult): void {
    this.latest.set(sessionId, {
      operation: result.operation,
      validResult: result.validResult,
      exitCode: result.exitCode,
      reason: result.reason,
    })
  }

  forget(sessionId: string): void {
    this.latest.delete(sessionId)
    this.tails.delete(sessionId)
  }

  check(sessionId: string): { ok: true; record: QualityRecord | null } | { ok: false; reason: string } {
    const record = this.latest.get(sessionId)
    if (!record || record.validResult) return { ok: true, record: record ?? null }
    const exit = record.exitCode === null ? 'no exit code' : `exit ${record.exitCode}`
    return {
      ok: false,
      reason: `the latest quality call in this session failed (${record.operation}, ${exit}${record.reason ? `: ${record.reason}` : ''}); fix the cause and run that quality tool again before uml_ir`,
    }
  }
}

/** What the panel showed before a fresh resident starts. */
export interface StoredView { diagram: string | null; declutter: string | null }

export interface IrRecord {
  validResult: boolean
  exitCode: number | null
  reason: string | null
}

/**
 * The per-session IR gate: `uml_display` may run only after the latest
 * `uml_ir` in the same session succeeded.
 *
 * This is a host-process record of this plugin's own tool calls, so it is
 * conservative after a restart or a session fork: the first `uml_display`
 * there is refused until `uml_ir` runs again. It never reports a stale success.
 */
export class IrGate {
  private readonly sessions = new Map<string, IrRecord>()

  record(sessionId: string, record: IrRecord): void {
    this.sessions.set(sessionId, record)
  }

  /** Forget a session, e.g. when its agent is disposed. */
  forget(sessionId: string): void {
    this.sessions.delete(sessionId)
  }

  check(sessionId: string): { ok: true; record: IrRecord } | { ok: false; reason: string } {
    const record = this.sessions.get(sessionId)
    if (!record) {
      return { ok: false, reason: 'uml_display needs a successful uml_ir in this session first; run uml_ir, then call uml_display' }
    }
    if (!record.validResult) {
      const exit = record.exitCode === null ? 'no exit code' : `exit ${record.exitCode}`
      return { ok: false, reason: `the latest uml_ir in this session failed (${exit}${record.reason ? `: ${record.reason}` : ''}); fix the project and run uml_ir again before uml_display` }
    }
    return { ok: true, record }
  }
}
