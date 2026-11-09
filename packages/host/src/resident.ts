import { realpath } from 'node:fs/promises'
import { StringDecoder } from 'node:string_decoder'
import type { DiscussionContext, QualityTarget, ResidentEvent, SceneFrame, SessionState } from '@dsh-uml-viewer/contracts'
import { parseResidentLine, RESIDENT_LINE_MAX_BYTES, residentAction, validateQualityTarget } from '@dsh-uml-viewer/contracts'
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
/**
 * How long a session's resident survives its last panel stream ending without
 * an explicit close (#32, D13). A carrier loss — a gateway reconnect, an HMR
 * client swap, the sidebar/main reparent — must not switch a session that is
 * still in use to a newer plugin version, so the resident (and its version
 * lease) waits this long for the same session to re-attach. A real page reload
 * or app restart is a fresh open and legitimately gets the newer version; this
 * is only the transport-loss window, so it stays small and bounded.
 */
export const WITHDRAWAL_GRACE_MS = 30_000

/**
 * What one stalled panel stream may hold unread before its queue is trimmed
 * (#review H2): 64 events or 8 MiB, whichever comes first. Without a cap a
 * single panel that stops reading accumulates every frame of the session, each
 * up to `RESIDENT_LINE_MAX_BYTES`, inside the host process.
 */
export const PANEL_QUEUE_MAX_ITEMS = 64
export const PANEL_QUEUE_MAX_BYTES = 8 * 1024 * 1024
/** The two per-panel queue caps; a test may shrink them. */
export interface QueueSize { items: number; bytes: number }
/** The size counted for an event the host synthesized rather than parsed. */
const SYNTHETIC_EVENT_BYTES = 256

const SOURCE_CONTEXT: DiscussionContext = { kind: 'real', edn: '{:context :real}' }
/** Actions whose answer can change the view or the discussion context. */
const SAVED_ACTIONS = new Set(['open', 'viewport', 'camera', 'display', 'reload', 'drill-in', 'back',
  'namespace-tree', 'select', 'locate', 'context', 'declutter'])
export const STALE_CONTEXT_REASON = 'previous proposal no longer exists; showing source diagram'

/** A display refresh: the final event, plus why a persisted context was dropped. */
export interface RefreshOutcome {
  event: ResidentEvent
  note: string | null
}

/** A panel cannot be served; the reason is shown in the panel. */
export class Refusal extends Error {}

/**
 * How a queue bounds its unread items (#review H2). `stale` names the items a
 * newer one supersedes, so dropping those loses no information; nothing else is
 * ever dropped silently.
 */
export interface QueueLimits<T> {
  /** Most items held while the reader is not consuming. */
  items: number
  /** Most bytes held while it is not consuming, as reported by `size`. */
  bytes: number
  /** The size of one item in bytes. */
  size(item: T): number
  /** True when a newer item makes this one obsolete. */
  stale(item: T): boolean
  /** The final item: why this stream ended instead of holding more. */
  overflow(held: { items: number; bytes: number }): T
}

/** A live stream of resident events with a blocking iterator. */
export class Queue<T> {
  private readonly items: T[] = []
  private readonly waiters: ((result: IteratorResult<T>) => void)[] = []
  private bytes = 0
  private ended = false

  /**
   * @param limits the size bound; without one the queue is unbounded, which is
   *   only right for a reader that always consumes (the readiness handshake).
   */
  constructor(private readonly limits?: QueueLimits<T>) {}

  push(item: T): void {
    if (this.ended) return
    const waiter = this.waiters.shift()
    if (waiter) {
      waiter({ value: item, done: false })
      return
    }
    this.items.push(item)
    this.bytes += this.limits?.size(item) ?? 0
    this.trim()
  }

  end(): void {
    this.ended = true
    for (const waiter of this.waiters.splice(0)) waiter({ value: undefined as never, done: true })
  }

  /**
   * Bring the buffer back under both caps. The oldest superseded item goes
   * first, so a stalled panel keeps the newest frames. When nothing left is
   * superseded — a `ready`, a refusal, an `error`, a context switch — the
   * stream ends instead: the backlog stays, one last event says why, and
   * dropping one of those would have hidden a state change or a failure.
   */
  private trim(): void {
    const limits = this.limits
    if (!limits) return
    while (this.items.length > limits.items || this.bytes > limits.bytes) {
      const index = this.items.findIndex(item => limits.stale(item))
      if (index < 0) {
        const last = limits.overflow({ items: this.items.length, bytes: this.bytes })
        this.items.push(last)
        this.bytes += limits.size(last)
        this.end()
        return
      }
      const dropped = this.items.splice(index, 1)[0]!
      this.bytes -= limits.size(dropped)
    }
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
  /**
   * A panel called the Remote `close` for this session: the next release that
   * leaves no holder stops the child at once instead of entering the
   * withdrawal grace. Re-attaching clears it — a new live panel is not closed.
   */
  closeRequested = false
  seq = 0
  private readonly subscribers = new Set<Queue<ResidentEvent>>()
  private readonly pending = new Map<number, {
    resolve: (event: ResidentEvent) => void
    reject: (error: Error) => void
    quietError: boolean
    quiet: boolean
  }>()
  /** The resident line each parsed event came from, for queue byte accounting. */
  private readonly lineBytes = new WeakMap<ResidentEvent, number>()
  /** Decodes stdout bytes so a UTF-8 character split across chunks survives. */
  private readonly decoder = new StringDecoder('utf8')
  /** The unterminated tail of stdout and its UTF-8 size. */
  private buffer = ''
  private bufferBytes = 0
  /** Set once the stream is unusable; later bytes and requests are refused. */
  private broken: string | null = null
  private settled = false
  readonly exited: Promise<void>
  /** One JSONL record's limit in UTF-8 bytes, newline (and CR) excluded. */
  readonly maxLineBytes: number
  /** This connection's per-panel queue caps (#review H2). */
  readonly queueMax: QueueSize

  constructor(id: string, cwd: string, child: SubprocessHandle, confined: boolean,
    maxLineBytes = RESIDENT_LINE_MAX_BYTES,
    queueMax: QueueSize = { items: PANEL_QUEUE_MAX_ITEMS, bytes: PANEL_QUEUE_MAX_BYTES }) {
    this.id = id
    this.cwd = cwd
    this.child = child
    this.confined = confined
    this.maxLineBytes = maxLineBytes
    this.queueMax = queueMax
    this.exited = child.done.then(() => undefined, () => undefined)
  }

  /**
   * A fresh, bounded event queue for one panel stream. A panel that stops
   * reading loses only frames older than the newest it will draw; anything
   * else ends that stream with a reason instead of being dropped.
   */
  attach(): Queue<ResidentEvent> {
    const queue = new Queue<ResidentEvent>({
      items: this.queueMax.items,
      bytes: this.queueMax.bytes,
      size: event => this.lineBytes.get(event) ?? SYNTHETIC_EVENT_BYTES,
      stale: event => event.event === 'frame',
      overflow: held => ({
        version: 1,
        seq: null,
        event: 'error',
        message: `the panel fell behind: ${held.items} unread events use ${held.bytes} bytes, past this stream's `
          + `${this.queueMax.items} events / ${this.queueMax.bytes} bytes; the host ended this panel stream instead of buffering more`,
      }),
    })
    this.subscribers.add(queue)
    return queue
  }

  detach(queue: Queue<ResidentEvent>): void {
    this.subscribers.delete(queue)
    queue.end()
  }

  /**
   * Ingest one stdout chunk, one JSON line at a time. Byte chunks are decoded
   * incrementally, so a character split between chunks is not corrupted. A
   * record over `maxLineBytes` is never buffered whole: the unterminated tail
   * is counted as it arrives, and crossing the limit breaks the connection
   * (pending requests reject, panels get an error, the child is terminated).
   */
  ingest(chunk: string | Uint8Array): void {
    if (this.broken !== null) return
    const text = typeof chunk === 'string' ? chunk : this.decoder.write(Buffer.from(chunk))
    let start = 0
    for (;;) {
      const end = text.indexOf('\n', start)
      const piece = text.slice(start, end < 0 ? text.length : end)
      const pieceBytes = Buffer.byteLength(piece, 'utf8')
      if (end < 0) {
        // Count before keeping: an endless line stops at the limit, not at OOM.
        // One trailing CR may still belong to a CRLF terminator.
        const tail = this.bufferBytes + pieceBytes
        if (tail > this.maxLineBytes + (piece.endsWith('\r') ? 1 : 0)) {
          this.overflow(tail)
          return
        }
        this.buffer += piece
        this.bufferBytes = tail
        return
      }
      let line = this.buffer + piece
      let bytes = this.bufferBytes + pieceBytes
      this.buffer = ''
      this.bufferBytes = 0
      if (line.endsWith('\r')) {
        line = line.slice(0, -1)
        bytes -= 1
      }
      if (bytes > this.maxLineBytes) {
        this.overflow(bytes)
        return
      }
      start = end + 1
      if (line.trim()) this.dispatch(line, bytes)
      if (this.broken !== null) return
    }
  }

  /** A record over the transport cap: the stream cannot be resynchronized safely. */
  private overflow(bytes: number): void {
    this.breakStream(`uml-viewer resident wrote a JSONL record over ${this.maxLineBytes} bytes `
      + `(at least ${bytes}); the resident was stopped`)
    this.child.terminate()
  }

  /** Mark the stream unusable, drop buffered bytes and fail every waiter. */
  private breakStream(reason: string): void {
    if (this.broken !== null) return
    this.broken = reason
    this.buffer = ''
    this.bufferBytes = 0
    this.fail(reason)
    // A broken stream is settled: no panel may join it while the child exits.
    this.end()
  }

  /** Reject every request still waiting for an answer. */
  private rejectPending(reason: string): void {
    const waiting = [...this.pending.values()]
    this.pending.clear()
    for (const pending of waiting) pending.reject(new Error(reason))
  }

  private dispatch(line: string, bytes: number): void {
    let event: ResidentEvent
    try {
      event = parseResidentLine(line)
    } catch (error) {
      this.fail(error instanceof Error ? error.message : String(error))
      return
    }
    // The queue caps count bytes; a parsed event carries the size of its line.
    this.lineBytes.set(event, bytes)
    if (event.event === 'ready') this.actions = event.actions
    else if (event.event === 'frame') this.latestFrame = event.frame
    else if (event.event === 'state') this.latestState = event.state
    else if (event.event === 'context') this.latestContext = event.context
    const seq = typeof event.seq === 'number' ? event.seq : null
    let quiet = false
    if (seq !== null) {
      const pending = this.pending.get(seq)
      if (pending) {
        this.pending.delete(seq)
        quiet = pending.quiet || (pending.quietError && event.event === 'error')
        pending.resolve(event)
      }
    }
    // A fallback request's expected error is handled by its caller; showing it
    // in every panel would turn a recovered display into a visible failure.
    if (!quiet) for (const queue of this.subscribers) queue.push(event)
  }

  /** Send one action and resolve with the first event that answers its `seq`. */
  request(action: { action: string } & Record<string, unknown>, timeoutMs = ACTION_TIMEOUT_MS,
    options: { quietError?: boolean; quiet?: boolean } = {}): Promise<ResidentEvent> {
    if (this.broken !== null) return Promise.reject(new Error(this.broken))
    const stdin = this.child.stdin
    if (!stdin) return Promise.reject(new Error('the resident session has no stdin channel'))
    const seq = ++this.seq
    return new Promise<ResidentEvent>((resolve, reject) => {
      const timer = setTimeout(() => {
        this.pending.delete(seq)
        reject(new Error(`uml-viewer resident did not answer ${action.action} within ${Math.round(timeoutMs / 1000)}s`))
      }, timeoutMs)
      this.pending.set(seq, {
        quietError: options.quietError === true,
        quiet: options.quiet === true,
        resolve: event => {
          clearTimeout(timer)
          resolve(event)
        },
        reject: error => {
          clearTimeout(timer)
          reject(error)
        },
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

  /**
   * Report a protocol or process failure to every panel watching this session.
   * Their streams end, and any request still waiting rejects with the reason
   * instead of waiting for its timeout.
   */
  fail(reason: string): void {
    this.rejectPending(reason)
    for (const queue of this.subscribers) {
      queue.push({ version: 1, seq: null, event: 'error', message: reason })
      queue.end()
    }
    this.subscribers.clear()
  }

  end(): void {
    this.settled = true
    this.rejectPending('the resident session ended')
    for (const queue of this.subscribers) queue.end()
    this.subscribers.clear()
  }

  get isSettled(): boolean {
    return this.settled
  }
}

/**
 * Owns the resident children and their bounded withdrawal windows. One child
 * per session; it starts when the first panel stream (or `uml_display`) needs
 * it and stops at the last holder's release, at an explicit close, when its
 * withdrawal grace expires, or at root shutdown.
 */
export class ResidentPool {
  readonly connections = new Map<string, ResidentConnection>()
  readonly opening = new Map<string, Promise<ResidentConnection>>()
  /** Armed withdrawal timers, by session id; only a live connection has one. */
  readonly graces = new Map<string, NodeJS.Timeout>()
  /** The withdrawal window every connection of this pool gets; tests shorten it. */
  readonly graceMs: number

  constructor(graceMs = WITHDRAWAL_GRACE_MS) {
    this.graceMs = graceMs
  }
}

/** Persist one live view; best effort, never an action failure. */
export async function saveView(states: SessionStates, connection: ResidentConnection): Promise<void> {
  try {
    // A stale/missing diagram reopens waiting. Preserve its last displayed
    // snapshot rather than replacing it with the blank canvas on resize.
    if (connection.latestFrame?.waiting && await states.read(connection.id)) return
    const saved = await connection.request({ action: 'save' }, ACTION_TIMEOUT_MS, { quiet: true })
    if (saved.event === 'state') await states.write(connection.id, saved.state)
  } catch {
    // A view-state write must not turn a successful action into an error.
  }
}

/** DSH shutting down: save and stop every resident a root pool still owns. */
export async function disposePool(states: SessionStates, pool: ResidentPool): Promise<void> {
  // No withdrawal window may outlive the root: clear every timer before the
  // children stop, so an expiry cannot race the shutdown and nothing is left.
  for (const timer of pool.graces.values()) clearTimeout(timer)
  pool.graces.clear()
  await Promise.allSettled([...pool.opening.values()])
  for (const connection of [...pool.connections.values()]) {
    pool.connections.delete(connection.id)
    await saveView(states, connection)
    connection.end()
    connection.child.terminate()
    await Promise.race([connection.exited, new Promise(resolve => setTimeout(resolve, STOP_TIMEOUT_MS))])
  }
}

/** Why a holder let go of a resident. */
export type ReleaseReason =
  /** A panel stream ended without an explicit close — a carrier loss. */
  | 'withdrawn'
  /** The holder is done with the child (a tool call, or a closed panel). */
  | 'closed'

export class ResidentSessions {
  private readonly pool: ResidentPool

  constructor(
    private readonly services: HostServices,
    private readonly config: Config,
    private readonly states: SessionStates,
    pool = new ResidentPool(),
  ) {
    this.pool = pool
  }

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
    const existing = this.pool.connections.get(id)
    if (existing && !existing.isSettled) {
      // A panel is attaching again. It joins the same child and the same
      // version, and its hold cancels the withdrawal window: one holder, one
      // ref, and no expiry may stop the resident under the new stream.
      this.clearGrace(id)
      existing.closeRequested = false
      existing.refs += 1
      return existing
    }
    const inflight = this.pool.opening.get(id)
    if (inflight) {
      const joined = await inflight
      this.writablePolicy(agent)
      joined.refs += 1
      return joined
    }
    const started = this.startFresh(agent, signal)
    this.pool.opening.set(id, started)
    try {
      return await started
    } finally {
      this.pool.opening.delete(id)
    }
  }

  private async startFresh(agent: Agent, signal: AbortSignal): Promise<ResidentConnection> {
    const connection = await this.start(agent, signal)
    connection.refs = 1
    this.pool.connections.set(connection.id, connection)
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
      // Raw bytes go to the connection's incremental decoder: decoding each
      // chunk alone would corrupt a UTF-8 character split between chunks.
      child.stdout.on('data', chunk => connection.ingest(typeof chunk === 'string' ? chunk : chunk as Uint8Array))
      child.stdout.on('error', error => connection.fail(`resident stdout failed: ${String(error)}`))
      void child.done.then(outcome => {
        if (!connection.isSettled) {
          connection.fail(`uml-viewer resident exited (${outcome.exitCode === null ? `signal ${outcome.signal}` : `exit ${outcome.exitCode}`})`)
        }
        connection.end()
        // A dead child needs no window: no grace timer may outlive its resident.
        this.clearGrace(connection.id)
        // A replacement may already serve this session; remove only this one.
        if (this.pool.connections.get(connection.id) === connection) this.pool.connections.delete(connection.id)
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

  /**
   * Release one holder. A withdrawn panel stream keeps the child (and its
   * version lease) alive for the pool's grace, so a carrier loss does not
   * switch a session that is still in use to another version; every other
   * release that leaves no holder stops the child at once.
   */
  async release(connection: ResidentConnection, reason: ReleaseReason = 'closed'): Promise<void> {
    if (connection.refs > 0) connection.refs -= 1
    if (connection.refs > 0) return
    if (reason === 'withdrawn' && !connection.closeRequested) {
      this.armGrace(connection)
      return
    }
    await this.stop(connection)
  }

  /**
   * A panel's explicit close (the Remote `close`, a user closing the panel).
   * That is the definite close a withdrawn stream is not: the resident stops
   * now when no holder is left, and the holder's own stream ending stops it.
   */
  async close(agent: Agent): Promise<boolean> {
    const id = agent.session.id
    this.clearGrace(id)
    const connection = this.pool.connections.get(id)
    if (!connection) return false
    connection.closeRequested = true
    if (connection.refs <= 0) await this.stop(connection)
    return true
  }

  /**
   * Hold the resident for a bounded window after its last panel stream ended.
   * The window is one timer per session: re-arming replaces it, an explicit
   * close, a re-attach, a child exit or root shutdown clears it, and it never
   * keeps the host process alive on its own.
   */
  private armGrace(connection: ResidentConnection): void {
    this.clearGrace(connection.id)
    const timer = setTimeout(() => {
      this.pool.graces.delete(connection.id)
      // A re-attach or a child that exited meanwhile wins over the expiry.
      if (connection.refs > 0 || connection.isSettled) return
      // Nobody came back: save the view and stop the resident (stop saves too).
      void this.stop(connection).catch(() => {})
    }, this.pool.graceMs)
    timer.unref()
    this.pool.graces.set(connection.id, timer)
  }

  private clearGrace(id: string): void {
    const timer = this.pool.graces.get(id)
    if (timer === undefined) return
    clearTimeout(timer)
    this.pool.graces.delete(id)
  }

  /**
   * Send one action to this session's resident (starting and stopping it when
   * no panel is open). A display also re-applies the persisted proposal
   * context: showing a diagram resets the resident to the source view, while
   * the session's discussion context may still be a proposal. A proposal that
   * no longer exists is stale view state, not a failed display: keep the source
   * frame, drop the persisted context and say so in the result's note.
   */
  async refresh(agent: Agent, signal: AbortSignal, action: { action: string } & Record<string, unknown>,
    restoreContext = false): Promise<RefreshOutcome> {
    const connection = await this.open(agent, signal)
    try {
      let event = await connection.request(action, ACTION_TIMEOUT_MS)
      let note: string | null = null
      const context = connection.latestContext
      if (restoreContext && context?.kind === 'proposal' && context.proposalId && event.event === 'frame') {
        // This error is the stale-state signal, not a panel failure: handle it
        // here so the display can recover and no watching panel sees it.
        const switched = await connection.request({ action: 'context', proposalId: context.proposalId },
          ACTION_TIMEOUT_MS, { quietError: true })
        if (switched.event === 'error') {
          note = STALE_CONTEXT_REASON
          await this.clearStaleContext(connection)
          // Tell the resident and any open panel that the source is the context
          // now. The frame from the display itself is already the source frame.
          try {
            await connection.request({ action: 'context', proposalId: null }, ACTION_TIMEOUT_MS, { quietError: true })
          } catch {
            // A resident that cannot reset the context still displayed the source.
          }
          return { event, note }
        }
        // `context` answers with a context event; `reload` returns the frame
        // now rendered in that discussion context.
        event = await connection.request({ action: 'reload' }, ACTION_TIMEOUT_MS)
      }
      return { event, note }
    } finally {
      // Save the final view, including any restored discussion context.
      if (SAVED_ACTIONS.has(action.action)) await this.save(connection)
      await this.release(connection)
    }
  }

  /**
   * What this session's resident says one panel target covers (#28): the
   * files are resolved from the diagram the resident has loaded, never from
   * a later read of the IR file and never from client input. A quiet request:
   * the answer belongs to the caller, no watching panel sees it and nothing
   * is persisted.
   */
  async qualityTarget(agent: Agent, signal: AbortSignal, id: string): Promise<QualityTarget> {
    const connection = await this.open(agent, signal)
    try {
      if (connection.latestFrame?.waiting) {
        throw new Error('no diagram: the panel shows nothing yet; run uml_ir and uml_display first')
      }
      const answer = await connection.request({ action: 'quality-target', id }, ACTION_TIMEOUT_MS, { quiet: true })
      if (answer.event === 'error') throw new Error(answer.message)
      if (answer.event !== 'quality-target') {
        throw new Error(`uml-viewer resident answered quality-target with ${answer.event}`)
      }
      // The carrier validated the line; this is the caller's own check that
      // the answer is about the element it asked for, inside the bounds.
      return validateQualityTarget(answer.target, 'quality target', id)
    } finally {
      await this.release(connection)
    }
  }

  /** Persist what the resident currently shows; best effort, never an action failure. */
  private save(connection: ResidentConnection): Promise<void> {
    return saveView(this.states, connection)
  }

  /**
   * What this session's resident shows now (#29): its latest frame and
   * discussion context, read without sending an action. The view requests
   * name an element of this frame and quote this context.
   */
  async shownView(agent: Agent, signal: AbortSignal): Promise<{ frame: SceneFrame | null; context: DiscussionContext }> {
    const connection = await this.open(agent, signal)
    try {
      return { frame: connection.latestFrame, context: connection.latestContext ?? SOURCE_CONTEXT }
    } finally {
      await this.release(connection)
    }
  }

  /** Persisted view state that names a vanished proposal must stop naming it. */
  private async clearStaleContext(connection: ResidentConnection): Promise<void> {
    connection.latestContext = SOURCE_CONTEXT
    try {
      const state = await this.states.read(connection.id)
      if (state) {
        await this.states.write(connection.id, { ...state, context: SOURCE_CONTEXT, proposalId: null, openLayer: null })
      }
    } catch {
      // Best effort: the resident's next `save` carries the reset context too.
    }
  }

  private async stop(connection: ResidentConnection): Promise<void> {
    this.clearGrace(connection.id)
    if (connection.isSettled) return
    this.pool.connections.delete(connection.id)
    await this.save(connection)
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

  /** Save during replacement without interrupting the panel's live resident. */
  async checkpoint(): Promise<void> {
    for (const connection of [...this.pool.connections.values()]) await this.save(connection)
  }

  async dispose(): Promise<void> {
    await disposePool(this.states, this.pool)
  }
}
