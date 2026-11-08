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
