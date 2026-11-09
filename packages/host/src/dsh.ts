import type { DisplayResult, IrResult, QualityToolResult } from '@dsh-uml-viewer/contracts'

/** Narrow, structural DSH boundary, verified with Host Service Inspect on
 * 0.2.2-rc.1. No value imports of DSH service packages at plugin load time. */
export interface Session { readonly id: string }

export interface PromptSection {
  readonly name: string
  readonly order: number
  readonly text: string | ((context: unknown) => string)
  readonly interpolate?: boolean
}

export interface SystemPrompt {
  section(section: PromptSection): () => void
  getSectionOrder?(name: string): number
}

export interface Agent {
  readonly id: string
  readonly session: Session
  /** The agent-scoped context; the panel's prompt section belongs to it. */
  readonly ctx?: { readonly systemPrompt?: SystemPrompt }
}

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

/** The parts of DSH's subprocess stdio the panel needs: live pipes and bounded collect writers. */
export type StdinMode = 'ignore' | 'pipe'
export type OutputMode = 'pipe' | { readonly maxBytes: number; readonly spill?: { readonly maxBytes: number } }

export interface SpawnSpec {
  argv: readonly string[]
  cwd: string
  stdio: { stdin: StdinMode; stdout: OutputMode; stderr: OutputMode; control?: 'pipe' }
  graceMs: number
  signal?: AbortSignal
}

export interface WritableLike {
  write(chunk: string): boolean
  end?(): void
}

export interface ReadableLike {
  on(event: 'data', listener: (chunk: unknown) => void): unknown
  on(event: 'error' | 'end' | 'close', listener: (error?: unknown) => void): unknown
}

interface OutputReader { readFrom(offset: number): { text: string; lossy: boolean } }

export interface SubprocessHandle {
  stdin?: WritableLike | undefined
  stdout?: ReadableLike | undefined
  stderr?: ReadableLike | undefined
  collected: { stdout?: OutputReader; stderr?: OutputReader }
  done: Promise<{ exitCode: number | null; signal: string | null }>
  terminate(): void
  waitForExit?(signal?: AbortSignal): Promise<boolean>
}

/** The invocation contribution the registry takes; only what this plugin registers. */
export interface TypertContribution {
  readonly package: string
  readonly face: 'host'
  readonly schemas: readonly unknown[]
  readonly model: { readonly services: readonly unknown[]; readonly events: readonly unknown[]; readonly objects: readonly unknown[] }
  readonly invocations: readonly unknown[]
}

export interface WebRoute {
  kind: 'exact' | 'prefix'
  path: string
  resource?(request: Request): Promise<Response> | Response
}

export interface ToolDefinition {
  name: string
  description: string
  parameters: unknown
  timeoutMs: number
  isConcurrencySafe(args: unknown): boolean
  output: {
    schema: unknown
    render(args: unknown, value: unknown): { type: 'text'; text: string }[]
  }
  execute(args: unknown, exec: ToolRunContext): Promise<IrResult | DisplayResult | QualityToolResult>
}

export interface HostServices {
  tools: { register(definition: ToolDefinition): () => void }
  workingDirectory: { get(session: Session): string; ensure(agent: Agent, signal: AbortSignal): Promise<string> }
  sandboxPolicy: { resolve(request: { session: Session }): Policy }
  subprocess: { spawn(spec: SpawnSpec): SubprocessHandle }
  sandbox?: { confine(argv: readonly string[], policy: Policy, signal: AbortSignal): Promise<ConfinedArgv> }
  /** Optional: the panel's typed Remote is only published in a profile that has the registry. */
  typert?: { register(contribution: TypertContribution): () => Promise<void> | void }
  /** Optional: the packaged Sarasa face is served over HTTP when a web server exists. */
  webServer?: { register(route: WebRoute): () => void }
  /** Optional: DSH's own home-path helper, so plugin state lands beside the profile. */
  dshHomePath?: (...segments: string[]) => string
}
