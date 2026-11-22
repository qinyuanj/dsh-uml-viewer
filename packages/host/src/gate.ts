import { randomBytes } from 'node:crypto'
import type { QualityOperation, QualityToolResult } from '@dsh-uml-viewer/contracts'

export interface QualityRecord {
  operation: QualityOperation
  validResult: boolean
  exitCode: number | null
  reason: string | null
}

export class QualityGate {
  private readonly latest = new Map<string, QualityRecord>()
  private readonly tails = new Map<string, Promise<void>>()

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

export interface IrRecord {
  validResult: boolean
  exitCode: number | null
  reason: string | null
  requestId: string | null
}

export class IrGate {
  private readonly sessions = new Map<string, IrRecord>()

  record(sessionId: string, record: IrRecord): void {
    this.sessions.set(sessionId, record)
  }

  answered(sessionId: string): string | null {
    return this.sessions.get(sessionId)?.requestId ?? null
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

export type GrantKind = 'omit' | 'new-proposal'

export class RequestGrants {
  static readonly LIMIT = 32
  private readonly sessions = new Map<string, Map<string, GrantKind>>()

  issue(sessionId: string, kind: GrantKind): string {
    const id = randomBytes(16).toString('hex')
    let pending = this.sessions.get(sessionId)
    if (!pending) {
      pending = new Map()
      this.sessions.set(sessionId, pending)
    }
    pending.set(id, kind)
    while (pending.size > RequestGrants.LIMIT) pending.delete(pending.keys().next().value!)
    return id
  }

  revoke(sessionId: string, id: string): void {
    const pending = this.sessions.get(sessionId)
    pending?.delete(id)
    if (pending?.size === 0) this.sessions.delete(sessionId)
  }

  consume(sessionId: string, id: string): { ok: true; kind: GrantKind } | { ok: false; reason: string } {
    const pending = this.sessions.get(sessionId)
    const kind = pending?.get(id)
    if (kind !== undefined) {
      this.revoke(sessionId, id)
      return { ok: true, kind }
    }
    const foreign = [...this.sessions.entries()].some(([other, ids]) => other !== sessionId && ids.has(id))
    return { ok: false, reason: foreign
      ? `panel request ${id} was not sent to this session; run uml_ir without request`
      : `panel request ${id} is unknown or was already used by an earlier uml_ir; run uml_ir without request` }
  }

  clear(): void {
    this.sessions.clear()
  }
}
