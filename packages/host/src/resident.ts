import { realpath } from 'node:fs/promises'
import type { DiscussionContext, ResidentEvent, SceneFrame, SessionState } from '@dsh-uml-viewer/contracts'
import { parseResidentLine, residentAction } from '@dsh-uml-viewer/contracts'
import { binaryArgv } from './binary.js'
import type { Config } from './binary.js'
import type { Agent, HostServices, Policy, SubprocessHandle } from './dsh.js'
import { confinedCommand, samePolicy, validPolicy } from './process.js'
import type { SessionStates } from './state.js'

/** Read-only sessions get no resident process: the panel says why instead of escalating. */
export const READ_ONLY_REASON = 'read-only sandbox: the viewer panel runs `uml-viewer resident` in the session workspace. Switch the session to workspace-write to open it; no elevation was attempted.'

const READY_TIMEOUT_MS = 20_000
const ACTION_TIMEOUT_MS = 20_000
const STOP_TIMEOUT_MS = 3_000

/** A panel cannot be served; the reason is shown in the panel. */
export class Refusal extends Error {}

/** A live stream of resident events with a blocking iterator. */
export class Queue<T> {
  private readonly items: T[] = []
  private readonly waiters: ((result: IteratorResult<T>) => void)[] = []
  private ended = false

  push(item: T): void {
    if (this.ended) return
    const waiter = this.waiters.shift()
    if (waiter) waiter({ value: item, done: false })
    else this.items.push(item)
  }

  end(): void {
    this.ended = true
    for (const waiter of this.waiters.splice(0)) waiter({ value: undefined as never, done: true })
  }

  /** Resolve with the next item, or `done` when the queue ended or the signal aborted. */
  next(signal?: AbortSignal): Promise<IteratorResult<T>> {
    if (this.items.length) return Promise.resolve({ value: this.items.shift()!, done: false })
    if (this.ended || signal?.aborted) return Promise.resolve({ value: undefined as never, done: true })
    return new Promise(resolve => {
      const waiter = (result: IteratorResult<T>) => {
        signal?.removeEventListener('abort', abort)
        resolve(result)
      }
      const abort = () => {
        const index = this.waiters.indexOf(waiter)
        if (index >= 0) this.waiters.splice(index, 1)
        resolve({ value: undefined as never, done: true })
      }
      this.waiters.push(waiter)
      signal?.addEventListener('abort', abort, { once: true })
    })
  }
}

/** One `uml-viewer resident` child per session, shared by every open panel of that session. */
export class ResidentConnection {
  readonly id: string
  readonly cwd: string
  readonly child: SubprocessHandle
  readonly confined: boolean
  actions: readonly string[] = []
  latestFrame: SceneFrame | null = null
  latestState: SessionState | null = null
  /** The discussion context restored with `open`, before any context event. */
  latestContext: DiscussionContext | null = null
  refs = 0
  seq = 0
  private readonly subscribers = new Set<Queue<ResidentEvent>>()
  private readonly pending = new Map<number, (event: ResidentEvent) => void>()
  private buffer = ''
  private settled = false
  readonly exited: Promise<void>

  constructor(id: string, cwd: string, child: SubprocessHandle, confined: boolean) {
    this.id = id
    this.cwd = cwd
    this.child = child
    this.confined = confined
    this.exited = child.done.then(() => undefined, () => undefined)
  }

  /** A fresh event queue for one panel stream. */
  attach(): Queue<ResidentEvent> {
    const queue = new Queue<ResidentEvent>()
    this.subscribers.add(queue)
    return queue
  }

  detach(queue: Queue<ResidentEvent>): void {
    this.subscribers.delete(queue)
    queue.end()
  }

  /** Ingest one stdout chunk, one JSON line at a time. */
  ingest(chunk: string): void {
    this.buffer += chunk
    for (;;) {
      const end = this.buffer.indexOf('\n')
      if (end < 0) return
      const line = this.buffer.slice(0, end).replace(/\r$/, '')
      this.buffer = this.buffer.slice(end + 1)
      if (line.trim()) this.dispatch(line)
    }
  }

  private dispatch(line: string): void {
    let event: ResidentEvent
    try {
      event = parseResidentLine(line)
    } catch (error) {
      this.fail(error instanceof Error ? error.message : String(error))
      return
    }
    if (event.event === 'ready') this.actions = event.actions
    else if (event.event === 'frame') this.latestFrame = event.frame
    else if (event.event === 'state') this.latestState = event.state
    else if (event.event === 'context') this.latestContext = event.context
    const seq = typeof event.seq === 'number' ? event.seq : null
    if (seq !== null) {
      const resolver = this.pending.get(seq)
      if (resolver) {
        this.pending.delete(seq)
        resolver(event)
      }
    }
    for (const queue of this.subscribers) queue.push(event)
  }

  /** Send one action and resolve with the first event that answers its `seq`. */
  request(action: { action: string } & Record<string, unknown>, timeoutMs = ACTION_TIMEOUT_MS): Promise<ResidentEvent> {
    const stdin = this.child.stdin
    if (!stdin) return Promise.reject(new Error('the resident session has no stdin channel'))
    const seq = ++this.seq
    return new Promise<ResidentEvent>((resolve, reject) => {
      const timer = setTimeout(() => {
        this.pending.delete(seq)
        reject(new Error(`uml-viewer resident did not answer ${action.action} within ${Math.round(timeoutMs / 1000)}s`))
      }, timeoutMs)
      this.pending.set(seq, event => {
        clearTimeout(timer)
        resolve(event)
      })
      try {
        stdin.write(`${residentAction(action, seq)}\n`)
      } catch (error) {
        clearTimeout(timer)
        this.pending.delete(seq)
        reject(error instanceof Error ? error : new Error(String(error)))
      }
    })
  }

  /** Report a protocol or process failure to every panel watching this session. */
  fail(reason: string): void {
    for (const queue of this.subscribers) {
      queue.push({ version: 1, seq: null, event: 'error', message: reason })
      queue.end()
    }
    this.subscribers.clear()
  }

  end(): void {
    this.settled = true
    for (const queue of this.subscribers) queue.end()
    this.subscribers.clear()
  }

  get isSettled(): boolean {
    return this.settled
  }
}

/**
 * Owns the resident children. One per session; it starts when the first panel
 * stream (or `uml_display`) needs it and stops when the last one releases it.
 */
export class ResidentSessions {
  private readonly connections = new Map<string, ResidentConnection>()
  private readonly opening = new Map<string, Promise<ResidentConnection>>()

  constructor(
    private readonly services: HostServices,
    private readonly config: Config,
    private readonly states: SessionStates,
  ) {}

  private writablePolicy(agent: Agent): Policy {
    const policy = this.services.sandboxPolicy.resolve({ session: agent.session })
    if (policy.mode === 'read-only') throw new Refusal(READ_ONLY_REASON)
    if (!validPolicy(policy)) throw new Refusal('Invalid authoritative sandbox policy; no resident was started')
    return policy as Policy
  }

  /** Start or join this session's resident, and open its waiting canvas. */
  async open(agent: Agent, signal: AbortSignal): Promise<ResidentConnection> {
    // Reuse is still an open: a session that became read-only must not acquire
    // another panel stream merely because a resident already exists.
    this.writablePolicy(agent)
    const id = agent.session.id
    const existing = this.connections.get(id)
    if (existing && !existing.isSettled) {
      existing.refs += 1
      return existing
    }
    const inflight = this.opening.get(id)
    if (inflight) {
      const joined = await inflight
      this.writablePolicy(agent)
      joined.refs += 1
      return joined
    }
    const started = this.startFresh(agent, signal)
    this.opening.set(id, started)
    try {
      return await started
    } finally {
      this.opening.delete(id)
    }
  }

  private async startFresh(agent: Agent, signal: AbortSignal): Promise<ResidentConnection> {
    const connection = await this.start(agent, signal)
    connection.refs = 1
    this.connections.set(connection.id, connection)
    try {
      const state = await this.states.read(connection.id)
      connection.latestContext = state?.context ?? null
      const opened = await connection.request({ action: 'open', waiting: true, ...(state ? { state } : {}) }, READY_TIMEOUT_MS)
      if (opened.event === 'error') throw new Refusal(`uml-viewer resident could not open the project: ${opened.message}`)
      if (opened.event !== 'frame') throw new Error(`uml-viewer resident answered open with ${opened.event}`)
      connection.latestFrame = opened.frame
      return connection
    } catch (error) {
      await this.stop(connection)
      throw error
    }
  }

  private async start(agent: Agent, signal: AbortSignal): Promise<ResidentConnection> {
    const policy = this.writablePolicy(agent)
    const cwd = await realpath(await this.services.workingDirectory.ensure(agent, signal))
    const command = await confinedCommand(this.services, [...binaryArgv(this.config), 'resident'], policy as Policy, signal)
    if (command.error) throw new Refusal(command.error.reason)
    const current = this.services.sandboxPolicy.resolve({ session: agent.session })
    if (!validPolicy(current) || !samePolicy(policy, current)) {
      throw new Refusal(`Sandbox policy changed during preparation (${current?.mode}); open the panel again`)
    }
    signal.throwIfAborted()
    const child = this.services.subprocess.spawn({
      argv: command.argv!,
      cwd,
      stdio: { stdin: 'pipe', stdout: 'pipe', stderr: { maxBytes: 64 * 1024 } },
      graceMs: 2000,
    })
    const connection = new ResidentConnection(agent.session.id, cwd, child, Boolean(command.confined))
    // This connection is not in `connections` until startFresh returns. Every
    // failure before then must therefore clean up the child right here.
    try {
      if (!child.stdout) throw new Error('DSH subprocess provider gave no resident stdout stream')
      // Attach before the first byte can arrive: the ready line is pushed to this
      // queue and the panel queues that follow get the recorded state instead.
      const ready = connection.attach()
      child.stdout.on('data', chunk => connection.ingest(typeof chunk === 'string' ? chunk
        : Buffer.from(chunk as Uint8Array).toString('utf8')))
      child.stdout.on('error', error => connection.fail(`resident stdout failed: ${String(error)}`))
      void child.done.then(outcome => {
        if (!connection.isSettled) {
          connection.fail(`uml-viewer resident exited (${outcome.exitCode === null ? `signal ${outcome.signal}` : `exit ${outcome.exitCode}`})`)
        }
        connection.end()
        this.connections.delete(connection.id)
      }, error => connection.fail(`uml-viewer resident failed: ${String(error)}`))
      try {
        await this.waitForReady(connection, ready)
      } finally {
        connection.detach(ready)
      }
      return connection
    } catch (error) {
      connection.end()
      child.terminate()
      await Promise.race([connection.exited, new Promise(resolve => setTimeout(resolve, STOP_TIMEOUT_MS))])
      throw error
    }
  }

  private async waitForReady(connection: ResidentConnection, queue: Queue<ResidentEvent>): Promise<void> {
    const first = await Promise.race([
      queue.next(),
      new Promise<IteratorResult<ResidentEvent>>(resolve => {
        void connection.exited.then(() => resolve({ value: undefined as never, done: true }))
      }),
      new Promise<IteratorResult<ResidentEvent>>((_resolve, reject) => setTimeout(() => reject(new Error('uml-viewer resident did not report ready within 20s')), READY_TIMEOUT_MS)),
    ])
    if (first.done) throw new Refusal('uml-viewer resident closed before it was ready')
    if (first.value.event === 'error') throw new Refusal(`uml-viewer resident refused to start: ${first.value.message}`)
    if (first.value.event !== 'ready') throw new Error(`uml-viewer resident started with ${first.value.event} instead of ready`)
  }

  /** Release one holder; the child stops with the last one. */
  async release(connection: ResidentConnection): Promise<void> {
    connection.refs -= 1
    if (connection.refs <= 0) await this.stop(connection)
  }

  /**
   * Send one action to this session's resident (starting and stopping it when
   * no panel is open). A display also re-applies the persisted proposal
   * context: showing a diagram resets the resident to the source view, while
   * the session's discussion context may still be a proposal.
   */
  async refresh(agent: Agent, signal: AbortSignal, action: { action: string } & Record<string, unknown>,
    restoreContext = false): Promise<ResidentEvent> {
    const connection = await this.open(agent, signal)
    try {
      let event = await connection.request(action, ACTION_TIMEOUT_MS)
      const context = connection.latestContext
      if (restoreContext && context?.kind === 'proposal' && context.proposalId && event.event === 'frame') {
        const switched = await connection.request({ action: 'context', proposalId: context.proposalId }, ACTION_TIMEOUT_MS)
        if (switched.event === 'error') return switched
        // `context` answers with a context event; `reload` returns the frame
        // now rendered in that discussion context.
        event = await connection.request({ action: 'reload' }, ACTION_TIMEOUT_MS)
      }
      return event
    } finally {
      await this.release(connection)
    }
  }

  private async stop(connection: ResidentConnection): Promise<void> {
    if (connection.isSettled) return
    this.connections.delete(connection.id)
    try {
      const state = await connection.request({ action: 'save' }, STOP_TIMEOUT_MS)
      if (state.event === 'state') await this.states.write(connection.id, state.state)
    } catch {
      // A panel closing is not a place to fail: the view state is best effort.
    }
    try {
      await connection.request({ action: 'quit' }, STOP_TIMEOUT_MS)
    } catch {
      // A child that already ended needs no quit.
    }
    connection.end()
    // `quit` exits 0 on its own; terminate only a child that does not.
    const exited = await Promise.race([connection.exited.then(() => true),
      new Promise<boolean>(resolve => setTimeout(() => resolve(false), STOP_TIMEOUT_MS))])
    if (!exited) {
      connection.child.terminate()
      await Promise.race([connection.exited, new Promise(resolve => setTimeout(resolve, STOP_TIMEOUT_MS))])
    }
  }

  async dispose(): Promise<void> {
    for (const connection of [...this.connections.values()]) {
      this.connections.delete(connection.id)
      connection.end()
      connection.child.terminate()
      await Promise.race([connection.exited, new Promise(resolve => setTimeout(resolve, STOP_TIMEOUT_MS))])
    }
  }
}
