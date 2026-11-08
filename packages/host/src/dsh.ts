import type { IrResult } from '@dsh-uml-viewer/contracts'

/** Narrow, structural DSH boundary, verified with Host Service Inspect on
 * 0.2.2-rc.1. No value imports of DSH service packages at plugin load time. */
export interface Session { readonly id: string }
export interface Agent { readonly id: string; readonly session: Session }
export interface ToolRunContext { agent?: Agent; signal: AbortSignal }
export interface Policy {
  mode: 'read-only' | 'workspace-write' | 'danger-full-access'
  workspaceRoot: string
  additionalWritableRoots?: readonly string[]
  sessionId?: string
}
export interface ConfinedArgv {
  argv: string[]
  enforcement: 'full' | 'partial'
  denialSignatures?: readonly string[]
  runnerFailureRules?: readonly {
    allowedExitCodes?: readonly number[]
    fatalSignatures: readonly string[]
    informationalLines?: readonly string[]
  }[]
}
export interface SpawnSpec {
  argv: readonly string[]
  cwd: string
  stdio: { stdin: 'ignore'; stdout: { maxBytes: number }; stderr: { maxBytes: number } }
  graceMs: number
  signal: AbortSignal
}
interface OutputReader { readFrom(offset: number): { text: string; lossy: boolean } }
export interface ToolDefinition {
  name: string
  description: string
  parameters: unknown
  timeoutMs: number
  isConcurrencySafe(args: unknown): boolean
  output: { schema: unknown; render(args: unknown, value: unknown): { type: 'text'; text: string }[] }
  execute(args: unknown, exec: ToolRunContext): Promise<IrResult>
}
export interface HostServices {
  tools: { register(definition: ToolDefinition): () => void }
  workingDirectory: { get(session: Session): string; ensure(agent: Agent, signal: AbortSignal): Promise<string> }
  sandboxPolicy: { resolve(request: { session: Session }): Policy }
  subprocess: { spawn(spec: SpawnSpec): {
    collected: { stdout?: OutputReader; stderr?: OutputReader }
    done: Promise<{ exitCode: number | null; signal: string | null }>
  } }
  sandbox?: { confine(argv: readonly string[], policy: Policy, signal: AbortSignal): Promise<ConfinedArgv> }
}
