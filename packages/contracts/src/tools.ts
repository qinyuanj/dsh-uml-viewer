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
  /** A no-refresh panel request id (#29), from the visible request message. */
  request?: string
}

/** A host-issued panel request id: 32 lowercase hex digits (128 random bits). */
const REQUEST_ID = /^[0-9a-f]{32}$/

/**
 * Validate a panel request id.
 * @param value the id.
 * @param label the field name used in failure messages.
 */
export function parseRequestId(value: unknown, label: string): string {
  if (typeof value !== 'string' || !REQUEST_ID.test(value)) throw new Error(`${label} must be a panel request id (32 lowercase hex digits)`)
  return value
}

/** Windows device names cannot name a file, with or without an extension. */
const DEVICE_NAME = /^(con|prn|aux|nul|com[1-9]|lpt[1-9])(\..*)?$/i

/**
 * Validate one project-relative path argument.
 *
 * This is the ONE rule for every project-relative path the plugin accepts or
 * receives: the model's `policy`/`output`/`targets`, the panel request files the
 * host queues and the resident's `quality-target` answer (#review M1a). Beyond
 * absolute paths, `..`, `.`, empty segments and a bare drive prefix it refuses
 * what Windows would silently reinterpret — an NTFS alternate data stream
 * (`a:b`), a reserved device name (`CON`, `con.edn`) and a segment that ends in
 * a dot or a space — so no caller can reintroduce a second, weaker spelling.
 *
 * @param field the argument name used in messages.
 * @param value the model's value.
 * @returns the same normalized relative path.
 */
export function normalizeRelativePath(field: string, value: unknown): string {
  if (typeof value !== 'string' || !value.trim() || value.includes(String.fromCharCode(0))) {
    throw new Error(`${field} must be a nonempty path without NUL`)
  }
  if (/[\u0001-\u001f\u007f]/.test(value)) throw new Error(`${field} must not contain control characters`)
  const parts = value.replaceAll('\\', '/').split('/')
  if (value.startsWith('/') || /^[a-zA-Z]:/.test(value) || parts.includes('..') || parts.includes('.') || parts.includes('')) {
    throw new Error(`${field} must be a normalized relative path inside the session cwd`)
  }
  if (value.includes(':')) throw new Error(`${field} must not name an alternate data stream`)
  for (const part of parts) {
    if (part.endsWith('.') || part.endsWith(' ')) throw new Error(`${field} must not end a path segment in a dot or a space`)
    if (DEVICE_NAME.test(part)) throw new Error(`${field} must not be a Windows device name`)
  }
  return value
}

export function parseIrRequest(value: unknown): IrRequest {
  if (!value || typeof value !== 'object' || Array.isArray(value)) throw new Error('IR arguments must be an object')
  const request: IrRequest = {}
  for (const [key, field] of Object.entries(value)) {
    if (key === 'request') {
      request.request = parseRequestId(field, 'request')
      continue
    }
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
    request: { type: 'string', pattern: '^[0-9a-f]{32}$', description: 'The :request id of the UML viewer panel request (:omit or :new-proposal) this IR answers. Such a request needs no quality refresh, so uml_ir runs even after a failed quality call; each id is single-use and valid only in the session that received it. Omit it for any other IR.' },
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

/**
 * A quality tool invocation as the agent sees it. The binary always reports a
 * real exit; the host uses `null` only when it refused or could not spawn.
 */
export interface QualityToolResult extends ToolResultRecord<QualityOperation> {
  exitCode: number | null
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

/** Check the tool value DSH receives, including host refusals with no child exit. */
export function validateQualityToolResult(value: unknown, expected?: QualityOperation): QualityToolResult {
  if (!value || typeof value !== 'object' || Array.isArray(value)) throw new Error('quality tool result must be an object')
  const source = value as Record<string, unknown>
  const operation = source.operation
  if (operation !== 'crap' && operation !== 'mutate' && operation !== 'dry') {
    throw new Error('quality tool result operation must be crap, mutate or dry')
  }
  if (expected !== undefined && operation !== expected) {
    throw new Error(`quality tool result operation must be ${expected}`)
  }
  if (source.version !== 1) throw new Error('quality tool result version must be 1')
  if (typeof source.projectRoot !== 'string') throw new Error('quality tool result projectRoot must be a string')
  if (source.exitCode !== null && (!Number.isInteger(source.exitCode)
    || (source.exitCode as number) < 0 || (source.exitCode as number) > 255)) {
    throw new Error('quality tool result exitCode must be an integer 0-255 or null')
  }
  if (typeof source.validResult !== 'boolean' || typeof source.configured !== 'boolean') {
    throw new Error('quality tool result validResult and configured must be booleans')
  }
  if (source.crapScope !== null && source.crapScope !== 'targets' && source.crapScope !== 'whole-tree') {
    throw new Error('quality tool result crapScope must be targets, whole-tree or null')
  }
  if (operation !== 'crap' && source.crapScope !== null) {
    throw new Error('quality tool result crapScope is null outside crap')
  }
  if (typeof source.tool !== 'string' || !source.tool) throw new Error('quality tool result tool must be nonempty')
  if (source.reason !== null && typeof source.reason !== 'string') {
    throw new Error('quality tool result reason must be a string or null')
  }
  if (source.reason !== null && source.validResult) {
    throw new Error('a quality refusal is never a valid result')
  }
  return source as unknown as QualityToolResult
}

function targetPaths(field: string, value: unknown, required: boolean): string[] {
  if (value === undefined && !required) return []
  if (!Array.isArray(value)) throw new Error(`${field} must be an array of project-relative paths`)
  if (required && value.length === 0) throw new Error(`${field} needs at least one project-relative path`)
  return value.map((target, index) => {
    const normalized = normalizeRelativePath(`${field}[${index}]`, target)
    if (normalized.startsWith('-')) {
      throw new Error(`${field}[${index}] must name a file or directory, not a command option`)
    }
    return normalized
  })
}

/**
 * Translate model-facing quality arguments into argv appended after
 * `uml-viewer quality <operation>`. All mutation targets stay in one argv:
 * mutator shares one coverage run and baseline across them.
 */
export function parseQualityToolRequest(operation: QualityOperation, value: unknown): string[] {
  if (!value || typeof value !== 'object' || Array.isArray(value)) {
    throw new Error(`${operation} arguments must be an object`)
  }
  const request = value as Record<string, unknown>
  if (operation === 'dry') {
    for (const key of Object.keys(request)) throw new Error(`Unknown uml_dry argument: ${key}`)
    return []
  }
  if (operation === 'crap') {
    for (const key of Object.keys(request)) if (key !== 'targets') throw new Error(`Unknown uml_crap argument: ${key}`)
    return targetPaths('targets', request.targets, false)
  }
  for (const key of Object.keys(request)) {
    if (key !== 'targets' && key !== 'mutateAll') throw new Error(`Unknown uml_mutate argument: ${key}`)
  }
  if (request.mutateAll !== undefined && typeof request.mutateAll !== 'boolean') {
    throw new Error('mutateAll must be a boolean')
  }
  const targets = targetPaths('targets', request.targets, true)
  return request.mutateAll === true ? ['--mutate-all', ...targets] : targets
}

const TARGETS_PARAMETER = {
  type: 'array', items: { type: 'string' },
  description: 'Project-relative files or directories inside the session cwd, in order, with no .. segments or command options.',
} as const

export const CRAP_PARAMETERS = {
  type: 'object', additionalProperties: false,
  properties: {
    targets: { ...TARGETS_PARAMETER,
      description: 'Optional files for a configured CRAP operation that declares target scope. The standard CRAP route still measures its declared whole tree.' },
  },
} as const

export const MUTATE_PARAMETERS = {
  type: 'object', additionalProperties: false, required: ['targets'],
  properties: {
    targets: { ...TARGETS_PARAMETER, minItems: 1,
      description: 'Every target file or directory for this differential mutation, in one call. They share one coverage run and baseline; each still gets its snapshot. Do not split them across calls.' },
    mutateAll: { type: 'boolean',
      description: 'Re-test all mutants in exactly these targets (the binary’s --mutate-all) instead of reusing previous outcomes.' },
  },
} as const

export const DRY_PARAMETERS = {
  type: 'object', additionalProperties: false,
  properties: {},
} as const

export const QUALITY_RESULT_SCHEMA = {
  type: 'object', additionalProperties: false,
  required: ['version', 'operation', 'projectRoot', 'exitCode', 'validResult', 'crapScope', 'configured', 'tool', 'reason'],
  properties: {
    version: { type: 'integer', const: 1 },
    operation: { type: 'string', enum: ['crap', 'mutate', 'dry'] },
    projectRoot: { type: 'string' },
    exitCode: { oneOf: [{ type: 'integer' }, { type: 'null' }] },
    validResult: { type: 'boolean' },
    crapScope: { oneOf: [{ type: 'string', enum: ['targets', 'whole-tree'] }, { type: 'null' }] },
    configured: { type: 'boolean' }, tool: { type: 'string' },
    reason: { oneOf: [{ type: 'string' }, { type: 'null' }] },
  },
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
