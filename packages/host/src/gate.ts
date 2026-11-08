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
