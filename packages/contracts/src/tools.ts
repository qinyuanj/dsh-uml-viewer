export type QualityOperation = 'crap' | 'mutate' | 'dry'

/** ADR 0001 version-1 fields. The operation value distinguishes IR from the
 * quality operations, so only the latter can participate in the #20 gate. */
export interface ToolResultRecord<Operation extends 'ir' | QualityOperation = 'ir' | QualityOperation> {
  version: 1
  operation: Operation
  projectRoot: string
  exitCode: number | null
  validResult: boolean
  crapScope: 'targets' | 'whole-tree' | null
  configured: boolean
  tool: string
}

/** A refusal has no child exit code; output is bounded and truncation is explicit. */
export interface IrResult extends ToolResultRecord {
  operation: 'ir'
  status: 'ok' | 'error' | 'refused'
  reason: string | null
  stdout: string
  stderr: string
  truncated: boolean
}

export interface IrRequest {
  policy?: string
  output?: string
}

export function parseIrRequest(value: unknown): IrRequest {
  if (!value || typeof value !== 'object' || Array.isArray(value)) throw new Error('IR arguments must be an object')
  const request: IrRequest = {}
  for (const [key, field] of Object.entries(value)) {
    if (key !== 'policy' && key !== 'output') throw new Error(`Unknown IR argument: ${key}`)
    if (typeof field !== 'string' || !field.trim() || field.includes(String.fromCharCode(0))) throw new Error(`${key} must be a nonempty path without NUL`)
    const parts = field.replaceAll('\\', '/').split('/')
    if (field.startsWith('/') || /^[a-zA-Z]:/.test(field) || parts.includes('..') || parts.includes('')) {
      throw new Error(`${key} must be a normalized relative path inside the session cwd`)
    }
    request[key] = field
  }
  return request
}

// Keep plain JSON Schema: shared with DSH without importing a host runtime.
export const IR_PARAMETERS = {
  type: 'object', additionalProperties: false,
  properties: {
    policy: { type: 'string', description: 'Normalized policy path relative to the session cwd, with no .. segments. Defaults to examples/<directory>.policy.edn, the first sorted examples/*.policy.edn, then policy.edn.' },
    output: { type: 'string', description: 'Normalized IR output path relative to the session cwd, with no .. segments. Omit to use the policy :out, else diagram.edn.' },
  },
} as const

export const IR_RESULT_SCHEMA = {
  type: 'object', additionalProperties: false,
  required: ['version', 'operation', 'projectRoot', 'exitCode', 'validResult', 'crapScope', 'configured', 'tool', 'status', 'reason', 'stdout', 'stderr', 'truncated'],
  properties: {
    version: { type: 'integer', const: 1 }, operation: { type: 'string', const: 'ir' }, projectRoot: { type: 'string' },
    exitCode: { oneOf: [{ type: 'integer' }, { type: 'null' }] }, validResult: { type: 'boolean' },
    crapScope: { type: 'null', const: null }, configured: { type: 'boolean' }, tool: { type: 'string' },
    status: { type: 'string', enum: ['ok', 'error', 'refused'] }, reason: { oneOf: [{ type: 'string' }, { type: 'null' }] },
    stdout: { type: 'string' }, stderr: { type: 'string' }, truncated: { type: 'boolean' },
  },
}

/** The stdout line of `uml-viewer quality <crap|mutate|dry>` (docs/quality-dispatch.md):
 * the version-1 record plus `reason`. A non-null `reason` means the dispatcher
 * refused before running `tool` (exit 1); null means the tool ran and
 * `exitCode` is its real exit (128+n after signal n). */
export interface QualityResult extends ToolResultRecord<QualityOperation> {
  exitCode: number
  reason: string | null
}

const QUALITY_KEYS = ['version', 'operation', 'projectRoot', 'exitCode', 'validResult', 'crapScope', 'configured', 'tool', 'reason'] as const

/** Parse one `uml-viewer quality` stdout line; throws on any other shape. */
export function parseQualityResult(line: string): QualityResult {
  const value: unknown = JSON.parse(line)
  if (!value || typeof value !== 'object' || Array.isArray(value)) throw new Error('quality result must be an object')
  const record = value as Record<string, unknown>
  for (const key of Object.keys(record)) {
    if (!(QUALITY_KEYS as readonly string[]).includes(key)) throw new Error(`unknown quality result field: ${key}`)
  }
  for (const key of QUALITY_KEYS) if (!(key in record)) throw new Error(`missing quality result field: ${key}`)
  const { version, operation, projectRoot, exitCode, validResult, crapScope, configured, tool, reason } = record
  if (version !== 1) throw new Error('quality result version must be 1')
  if (operation !== 'crap' && operation !== 'mutate' && operation !== 'dry') throw new Error('quality result operation must be crap, mutate or dry')
  if (typeof projectRoot !== 'string' || !projectRoot) throw new Error('projectRoot must be a nonempty string')
  if (typeof exitCode !== 'number' || !Number.isInteger(exitCode) || exitCode < 0 || exitCode > 255) throw new Error('exitCode must be an integer 0-255')
  if (typeof validResult !== 'boolean' || typeof configured !== 'boolean') throw new Error('validResult and configured must be booleans')
  if (crapScope !== null && crapScope !== 'targets' && crapScope !== 'whole-tree') throw new Error('crapScope must be targets, whole-tree or null')
  if (operation !== 'crap' && crapScope !== null) throw new Error('crapScope is null outside crap')
  if (typeof tool !== 'string' || !tool) throw new Error('tool must be a nonempty string')
  if (reason !== null && typeof reason !== 'string') throw new Error('reason must be a string or null')
  if (reason !== null && validResult) throw new Error('a refusal is never a valid result')
  return { version, operation, projectRoot, exitCode, validResult, crapScope, configured, tool, reason }
}
