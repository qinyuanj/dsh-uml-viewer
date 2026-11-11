/**
 * Request status (#51, ADR 0015): what the panel shows of each panel request
 * of its session. The host keeps the list in process memory and pushes the
 * whole list on the panel stream (`request-status`) when the stream opens and
 * on every change; DSH restarting forgets it.
 *
 * States, per the spec's transition table:
 * - `queued`: DSH admitted the request message;
 * - `running`: the companion started on it (#52);
 * - `done`: it finished (a context switch is done once admitted);
 * - `failed`: it was not admitted, or its run failed; `reason` says why and
 *   `exitCode` is the quality record's exit code when there is one.
 */
import type { QualityRequestOp } from './requests.js'
import type { PanelRequestTarget } from './requests.js'
import { QUALITY_REQUESTS } from './requests.js'
import { parseElementTarget } from './tools.js'

export type RequestState = 'queued' | 'running' | 'done' | 'failed'

/** Every request state, in the order a request moves through them. */
export const REQUEST_STATES: readonly RequestState[] = Object.freeze(['queued', 'running', 'done', 'failed'])

/** What a panel request asks for: a quality refresh, a hide, a context switch or a new proposal. */
export type RequestOp = QualityRequestOp | 'omit' | 'context' | 'new-proposal'

/** Every request op. */
export const REQUEST_OPS: readonly RequestOp[] = Object.freeze([...QUALITY_REQUESTS, 'omit', 'context', 'new-proposal'])

/** One panel request as the panel shows it. */
export interface RequestStatusEntry {
  /** Unique in its session; a hide or new proposal uses its grant id (`uml_ir({request})`). */
  id: string
  op: RequestOp
  /** The element the request names; `null` for a context switch, a new proposal or an unresolved element. */
  target: PanelRequestTarget | null
  state: RequestState
  /** Why it failed; `null` unless `state` is `failed`. */
  reason: string | null
  /** The failed quality record's exit code, when it has one. */
  exitCode: number | null
}

function fail(label: string, message: string): never {
  throw new Error(`${label}: ${message}`)
}

function oneOf<T extends string>(value: unknown, allowed: readonly T[], label: string): T {
  if (typeof value !== 'string' || !(allowed as readonly string[]).includes(value)) {
    fail(label, `must be one of ${allowed.join(', ')}`)
  }
  return value as T
}

/**
 * Validate a request-status id: what an entry carries and what a dismissal
 * (#53) names.
 * @param value the id.
 * @param label the field name used in failure messages.
 * @returns the same id.
 */
export function parseRequestStatusId(value: unknown, label = 'request id'): string {
  if (typeof value !== 'string' || !/^[A-Za-z0-9_-]{1,128}$/.test(value)) {
    fail(label, 'must be 1–128 ASCII letters, digits, underscores or hyphens')
  }
  return value
}

/**
 * Validate one entry.
 * @param value the entry.
 * @param label the field name used in failure messages.
 * @returns the same entry, narrowed.
 */
export function validateRequestStatusEntry(value: unknown, label = 'request status'): RequestStatusEntry {
  if (!value || typeof value !== 'object' || Array.isArray(value)) fail(label, 'must be an object')
  const source = value as Record<string, unknown>
  const id = parseRequestStatusId(source.id, `${label}.id`)
  const op = oneOf(source.op, REQUEST_OPS, `${label}.op`)
  const target = parseElementTarget(source.target ?? null, `${label}.target`)
  const state = oneOf(source.state, REQUEST_STATES, `${label}.state`)
  const reason = source.reason === null || source.reason === undefined ? null : source.reason
  if (reason !== null && typeof reason !== 'string') fail(`${label}.reason`, 'must be a string or null')
  if (state === 'failed' && !reason) fail(`${label}.reason`, 'a failed request must say why')
  if (state !== 'failed' && reason !== null) fail(`${label}.reason`, 'only a failed request has a reason')
  const exitCode = source.exitCode === null || source.exitCode === undefined ? null : source.exitCode
  if (exitCode !== null && (typeof exitCode !== 'number' || !Number.isInteger(exitCode))) {
    fail(`${label}.exitCode`, 'must be an integer or null')
  }
  return { id, op, target, state, reason, exitCode }
}

/**
 * Validate a session's whole list, oldest first; ids are unique.
 * @param value the list.
 * @param label the field name used in failure messages.
 * @returns the same list, narrowed.
 */
export function validateRequestStatusList(value: unknown, label = 'requests'): RequestStatusEntry[] {
  if (!Array.isArray(value)) fail(label, 'must be an array')
  const seen = new Set<string>()
  return value.map((entry, index) => {
    const parsed = validateRequestStatusEntry(entry, `${label}[${index}]`)
    if (seen.has(parsed.id)) fail(`${label}[${index}].id`, `duplicate request id ${parsed.id}`)
    seen.add(parsed.id)
    return parsed
  })
}
