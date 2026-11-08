export type QualityOperation = 'crap' | 'mutate' | 'dry'

/** ADR 0001 version-1 fields. The operation value distinguishes IR from the
 * quality operations, so only the latter can participate in the #20 gate. */
export interface ToolResultRecord<Operation extends 'ir' | 'display' | QualityOperation = 'ir' | 'display' | QualityOperation> {
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
export interface IrResult extends ToolResultRecord<'ir'> {
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

/**
 * Validate one project-relative path argument.
 * @param field the argument name used in messages.
 * @param value the model's value.
 * @returns the same normalized relative path.
 */
export function normalizeRelativePath(field: string, value: unknown): string {
  if (typeof value !== 'string' || !value.trim() || value.includes(String.fromCharCode(0))) {
    throw new Error(`${field} must be a nonempty path without NUL`)
  }
  const parts = value.replaceAll('\\', '/').split('/')
  if (value.startsWith('/') || /^[a-zA-Z]:/.test(value) || parts.includes('..') || parts.includes('')) {
    throw new Error(`${field} must be a normalized relative path inside the session cwd`)
  }
  return value
}

export function parseIrRequest(value: unknown): IrRequest {
  if (!value || typeof value !== 'object' || Array.isArray(value)) throw new Error('IR arguments must be an object')
  const request: IrRequest = {}
  for (const [key, field] of Object.entries(value)) {
    if (key !== 'policy' && key !== 'output') throw new Error(`Unknown IR argument: ${key}`)
    request[key] = normalizeRelativePath(key, field)
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


/**
 * What the panel shows: the diagram the latest successful IR wrote.
 *
 * A display is not a child process, so its ADR 0001 record keeps
 * `exitCode: null` and `validResult` reports whether the panel now shows the
 * requested diagram.
 */
export interface DisplayRequest {
  path?: string
}

export interface DisplayResult extends ToolResultRecord<'display'> {
  operation: 'display'
  path: string | null
  status: 'ok' | 'error' | 'refused'
  reason: string | null
}

export function parseDisplayRequest(value: unknown): DisplayRequest {
  if (!value || typeof value !== 'object' || Array.isArray(value)) throw new Error('uml_display arguments must be an object')
  const request: DisplayRequest = {}
  for (const [key, field] of Object.entries(value)) {
    if (key !== 'path') throw new Error(`Unknown uml_display argument: ${key}`)
    request.path = normalizeRelativePath(key, field)
  }
  return request
}

export const DISPLAY_PARAMETERS = {
  type: 'object', additionalProperties: false,
  properties: {
    path: { type: 'string', description: 'Normalized diagram (IR) path relative to the session cwd, with no .. segments. Omit to show the diagram the latest successful uml_ir wrote.' },
  },
} as const

export const DISPLAY_RESULT_SCHEMA = {
  type: 'object', additionalProperties: false,
  required: ['version', 'operation', 'projectRoot', 'path', 'exitCode', 'validResult', 'crapScope', 'configured', 'tool', 'status', 'reason'],
  properties: {
    version: { type: 'integer', const: 1 }, operation: { type: 'string', const: 'display' }, projectRoot: { type: 'string' },
    path: { oneOf: [{ type: 'string' }, { type: 'null' }] },
    exitCode: { type: 'null', const: null }, validResult: { type: 'boolean' },
    crapScope: { type: 'null', const: null }, configured: { type: 'boolean' }, tool: { type: 'string' },
    status: { type: 'string', enum: ['ok', 'error', 'refused'] }, reason: { oneOf: [{ type: 'string' }, { type: 'null' }] },
  },
}

/**
 * Check one display record against its schema.
 * @param value the record the tool returned.
 * @returns the same record, narrowed.
 */
export function validateDisplayResult(value: unknown): DisplayResult {
  if (!value || typeof value !== 'object' || Array.isArray(value)) throw new Error('display result: must be an object')
  const source = value as Record<string, unknown>
  if (source.version !== 1) throw new Error('display result.version: must be 1')
  if (source.operation !== 'display') throw new Error('display result.operation: must be display')
  if (!['ok', 'error', 'refused'].includes(source.status as string)) throw new Error('display result.status: must be ok, error or refused')
  if (typeof source.projectRoot !== 'string') throw new Error('display result.projectRoot: must be a string')
  if (source.path !== null && typeof source.path !== 'string') throw new Error('display result.path: must be a string or null')
  if (source.exitCode !== null) throw new Error('display result.exitCode: must be null')
  if (typeof source.validResult !== 'boolean') throw new Error('display result.validResult: must be a boolean')
  if (source.crapScope !== null) throw new Error('display result.crapScope: must be null')
  if (typeof source.configured !== 'boolean') throw new Error('display result.configured: must be a boolean')
  if (typeof source.tool !== 'string') throw new Error('display result.tool: must be a string')
  if (source.reason !== null && typeof source.reason !== 'string') throw new Error('display result.reason: must be a string or null')
  return source as unknown as DisplayResult
}
