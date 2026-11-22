import { StringDecoder } from 'node:string_decoder'
import type { DiscussionContext, QualityTarget, ResidentEvent, SceneFrame, SessionState } from '@dsh-uml-viewer/contracts'
import { parseResidentLine, RESIDENT_LINE_MAX_BYTES, residentAction, validateQualityTarget } from '@dsh-uml-viewer/contracts'
import { binaryArgv } from './binary.js'
import type { Config } from './binary.js'
import type { Agent, HostServices, Policy, SubprocessHandle } from './dsh.js'
import { prepareConfinedCommand, validPolicy } from './process.js'
import type { SessionStates } from './state.js'

export const READ_ONLY_REASON = 'read-only sandbox: the viewer panel runs `uml-viewer resident` in the session workspace. Switch the session to workspace-write to open it; no elevation was attempted.'

const READY_TIMEOUT_MS = 20_000
const ACTION_TIMEOUT_MS = 20_000
const STOP_TIMEOUT_MS = 3_000
export const WITHDRAWAL_GRACE_MS = 30_000

export const PANEL_QUEUE_MAX_ITEMS = 64
export const PANEL_QUEUE_MAX_BYTES = 8 * 1024 * 1024
export interface QueueSize { items: number; bytes: number }
const SYNTHETIC_EVENT_BYTES = 256

const SOURCE_CONTEXT: DiscussionContext = { kind: 'real', edn: '{:context :real}' }
const SAVED_ACTIONS = new Set(['open', 'viewport', 'camera', 'display', 'reload', 'drill-in', 'back',
  'namespace-tree', 'select', 'locate', 'context', 'declutter'])
export const STALE_CONTEXT_REASON = 'previous proposal no longer exists; showing source diagram'

export interface RefreshOutcome {
  event: ResidentEvent
  note: string | null
}

export class Refusal extends Error {}

export interface QueueLimits<T> {
  items: number
  bytes: number
  size(item: T): number
  stale(item: T): boolean
  overflow(held: { items: number; bytes: number }): T
}

export class Queue<T> {
  private readonly items: T[] = []
  private readonly waiters: ((result: IteratorResult<T>) => void)[] = []
  private bytes = 0
  private ended = false

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

  private trim(): void {
    const limits = this.limits
    if (!limits) return
    while (this.items.length > limits.items || this.bytes > limits.bytes) {
      const latest = this.items.findLastIndex(item => limits.stale(item))
      const index = this.items.findIndex((item, index) => index < latest && limits.stale(item))
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

  next(signal?: AbortSignal): Promise<IteratorResult<T>> {
    if (this.items.length) {
      const item = this.items.shift()!
      this.bytes -= this.limits?.size(item) ?? 0
      return Promise.resolve({ value: item, done: false })
    }
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

export class ResidentConnection {
  readonly id: string
  readonly cwd: string
  readonly child: SubprocessHandle
  readonly confined: boolean
  actions: readonly string[] = []
  latestFrame: SceneFrame | null = null
  latestState: SessionState | null = null
  latestContext: DiscussionContext | null = null
  refs = 0
  panelRefs = 0
  closeRequested = false
  readonly holders = new Map<string, { pageId: string; active: boolean; closed: boolean }>()
  pageId: string | null = null
  withdrawn = false
  seq = 0
  private readonly subscribers = new Set<Queue<ResidentEvent>>()
  private readonly pending = new Map<number, {
    resolve: (event: ResidentEvent) => void
    reject: (error: Error) => void
    quietError: boolean
    quiet: boolean
  }>()
  private readonly lineBytes = new WeakMap<ResidentEvent, number>()
  private readonly decoder = new StringDecoder('utf8')
  private buffer = ''
  private bufferBytes = 0
  private broken: string | null = null
  private settled = false
  readonly exited: Promise<void>
  readonly maxLineBytes: number
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

  ingest(chunk: string | Uint8Array): void {
    if (this.broken !== null) return
    const text = typeof chunk === 'string' ? chunk : this.decoder.write(Buffer.from(chunk))
    let start = 0
    for (;;) {
      const end = text.indexOf('\n', start)
      const piece = text.slice(start, end < 0 ? text.length : end)
      const pieceBytes = Buffer.byteLength(piece, 'utf8')
      if (end < 0) {
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

  private overflow(bytes: number): void {
    this.breakStream(`uml-viewer resident wrote a JSONL record over ${this.maxLineBytes} bytes `
      + `(at least ${bytes}); the resident was stopped`)
    this.child.terminate()
  }

  private breakStream(reason: string): void {
    if (this.broken !== null) return
    this.broken = reason
    this.buffer = ''
    this.bufferBytes = 0
    this.fail(reason)
    this.end()
  }

  private rejectPending(reason: string): void {
    const waiting = [...this.pending.values()]
    this.pending.clear()
    for (const pending of waiting) pending.reject(new Error(reason))
  }

  private dispatch(line: string, bytes: number): void {
    let event: ResidentEvent | null
    try {
      event = parseResidentLine(line)
    } catch (error) {
      this.fail(error instanceof Error ? error.message : String(error))
      return
    }
    if (event === null) return
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
    if (!quiet) for (const queue of this.subscribers) queue.push(event)
  }

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

export class ResidentPool {
  readonly connections = new Map<string, ResidentConnection>()
  readonly opening = new Map<string, Promise<ResidentConnection>>()
  readonly transitions = new Map<string, Promise<unknown>>()
  disposed = false
  readonly graces = new Map<string, NodeJS.Timeout>()
  readonly graceMs: number

  constructor(graceMs = WITHDRAWAL_GRACE_MS) {
    this.graceMs = graceMs
  }
}

export async function saveView(states: SessionStates, connection: ResidentConnection): Promise<void> {
  try {
    if (connection.latestFrame?.waiting && await states.read(connection.id)) return
    const saved = await connection.request({ action: 'save' }, ACTION_TIMEOUT_MS, { quiet: true })
    if (saved.event === 'state') await states.write(connection.id, saved.state)
  } catch {
  }
}

export async function disposePool(states: SessionStates, pool: ResidentPool): Promise<void> {
  for (const timer of pool.graces.values()) clearTimeout(timer)
  pool.graces.clear()
  pool.disposed = true
  await Promise.allSettled([...pool.transitions.values()])
  pool.transitions.clear()
  await Promise.allSettled([...pool.opening.values()])
  for (const connection of [...pool.connections.values()]) {
    pool.connections.delete(connection.id)
    connection.holders.clear()
    connection.refs = 0
    connection.panelRefs = 0
    await saveView(states, connection)
    connection.end()
    connection.child.terminate()
    await Promise.race([connection.exited, new Promise(resolve => setTimeout(resolve, STOP_TIMEOUT_MS))])
  }
}

export type ReleaseReason =
  | 'withdrawn'
  | 'closed'
  | 'borrowed'

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

  async open(agent: Agent, signal: AbortSignal, panel = true, language: string | null = null): Promise<ResidentConnection> {
    this.writablePolicy(agent)
    if (this.pool.disposed) throw new Refusal('Viewer root is shut down')
    const id = agent.session.id
    const existing = this.pool.connections.get(id)
    if (existing && !existing.isSettled) {
      if (panel) {
        this.clearGrace(id)
        existing.withdrawn = false
        existing.closeRequested = false
      }
      if (panel) existing.panelRefs += 1
      existing.refs += 1
      return existing
    }
    const inflight = this.pool.opening.get(id)
    if (inflight) {
      const joined = await inflight
      this.writablePolicy(agent)
      if (panel) {
        this.clearGrace(id)
        joined.withdrawn = false
        joined.closeRequested = false
        joined.panelRefs += 1
      }
      joined.refs += 1
      return joined
    }
    const started = this.startFresh(agent, signal, panel, language)
    this.pool.opening.set(id, started)
    try {
      return await started
    } finally {
      this.pool.opening.delete(id)
    }
  }

  private async transition<T>(id: string, run: () => Promise<T>): Promise<T> {
    const previous = this.pool.transitions.get(id)
    const pending = (async () => { await previous?.catch(() => {}); return run() })()
    this.pool.transitions.set(id, pending)
    try { return await pending } finally {
      if (this.pool.transitions.get(id) === pending) this.pool.transitions.delete(id)
    }
  }

  async openOwned(agent: Agent, signal: AbortSignal, owner: { pageId: string; holderId: string },
    language: string | null = null): Promise<ResidentConnection> {
    return this.transition(agent.session.id, async () => {
      this.writablePolicy(agent)
      const old = this.pool.connections.get(agent.session.id)
      if (old && old.refs === 0 && old.pageId !== null && old.pageId !== owner.pageId) await this.stop(old)
      const connection = await this.open(agent, signal, true, language)
      for (const [id, holder] of connection.holders) if (!holder.active) connection.holders.delete(id)
      if (connection.holders.has(owner.holderId)) {
        await this.release(connection, 'withdrawn')
        throw new Refusal('Viewer holder is already attached')
      }
      connection.pageId = owner.pageId
      connection.holders.set(owner.holderId, { pageId: owner.pageId, active: true, closed: false })
      return connection
    })
  }

  async releaseOwned(connection: ResidentConnection, owner: { pageId: string; holderId: string }): Promise<void> {
    const holder = connection.holders.get(owner.holderId)
    if (!holder || !holder.active || holder.pageId !== owner.pageId) return
    holder.active = false
    if (holder.closed) connection.holders.delete(owner.holderId)
    await this.release(connection, holder.closed ? 'closed' : 'withdrawn')
  }

  async closeOwned(agent: Agent, owner: { pageId: string; holderId: string }): Promise<boolean> {
    return this.transition(agent.session.id, async () => {
      const connection = this.pool.connections.get(agent.session.id)
      const holder = connection?.holders.get(owner.holderId)
      if (!connection || !holder || holder.pageId !== owner.pageId || holder.closed) return false
      holder.closed = true
      connection.holders.delete(owner.holderId)
      if (holder.active) await this.release(connection, 'closed')
      else if (connection.panelRefs === 0) {
        connection.withdrawn = false
        this.clearGrace(connection.id)
        await this.stop(connection)
      }
      return true
    })
  }

  private async startFresh(agent: Agent, signal: AbortSignal, panel: boolean,
    language: string | null): Promise<ResidentConnection> {
    const connection = await this.start(agent, signal)
    connection.refs = 1
    connection.panelRefs = panel ? 1 : 0
    this.pool.connections.set(connection.id, connection)
    try {
      const state = await this.states.read(connection.id)
      connection.latestContext = state?.context ?? null
      const opened = await connection.request({ action: 'open', waiting: true,
        ...(language !== null ? { language } : {}), ...(state ? { state } : {}) }, READY_TIMEOUT_MS)
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
    const prepared = await prepareConfinedCommand(
      this.services, agent, signal,
      () => [...binaryArgv(this.config), 'resident'],
      (refusal, current) => {
        if (refusal === 'read-only') return READ_ONLY_REASON
        if (refusal === 'invalid-policy') return 'Invalid authoritative sandbox policy; no resident was started'
        return `Sandbox policy changed during preparation (${current?.mode}); open the panel again`
      },
    )
    if (prepared.command === undefined) throw new Refusal(prepared.reason)
    const cwd = prepared.root
    const child = this.services.subprocess.spawn({
      argv: prepared.command.argv,
      cwd,
      stdio: { stdin: 'pipe', stdout: 'pipe', stderr: { maxBytes: 64 * 1024 } },
      graceMs: 2000,
    })
    const connection = new ResidentConnection(agent.session.id, cwd, child, Boolean(prepared.command.confined))
    try {
      if (!child.stdout) throw new Error('DSH subprocess provider gave no resident stdout stream')
      const ready = connection.attach()
      child.stdout.on('data', chunk => connection.ingest(typeof chunk === 'string' ? chunk : chunk as Uint8Array))
      child.stdout.on('error', error => connection.fail(`resident stdout failed: ${String(error)}`))
      void child.done.then(outcome => {
        if (!connection.isSettled) {
          connection.fail(`uml-viewer resident exited (${outcome.exitCode === null ? `signal ${outcome.signal}` : `exit ${outcome.exitCode}`})`)
        }
        connection.end()
        this.clearGrace(connection.id)
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

  async release(connection: ResidentConnection, reason: ReleaseReason = 'closed'): Promise<void> {
    if (connection.refs <= 0) return
    connection.refs -= 1
    if (reason !== 'borrowed' && connection.panelRefs > 0) {
      connection.panelRefs -= 1
      if (connection.panelRefs === 0) {
        connection.withdrawn = reason === 'withdrawn' && !connection.closeRequested
        if (connection.withdrawn && !connection.isSettled && !this.pool.disposed) this.armGrace(connection)
        else this.clearGrace(connection.id)
      }
    }
    if (connection.isSettled || this.pool.disposed || this.pool.connections.get(connection.id) !== connection) return
    if (reason === 'closed' && connection.panelRefs === 0) {
      await this.stop(connection)
      return
    }
    if (connection.refs > 0) return
    if (connection.withdrawn && !connection.closeRequested && this.pool.graces.has(connection.id)) return
    await this.stop(connection)
  }

  async close(agent: Agent): Promise<boolean> {
    const id = agent.session.id
    const connection = this.pool.connections.get(id)
    if (!connection || connection.pageId !== null) return false
    this.clearGrace(id)
    connection.closeRequested = true
    if (connection.refs <= 0) await this.stop(connection)
    return true
  }

  private armGrace(connection: ResidentConnection): void {
    this.clearGrace(connection.id)
    connection.withdrawn = true
    const timer = setTimeout(() => {
      this.pool.graces.delete(connection.id)
      if (connection.refs > 0 || connection.isSettled) return
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

  async refresh(agent: Agent, signal: AbortSignal, action: { action: string } & Record<string, unknown>,
    restoreContext = false): Promise<RefreshOutcome> {
    const connection = await this.open(agent, signal, false)
    try {
      let event = await connection.request(action, ACTION_TIMEOUT_MS)
      let note: string | null = null
      const context = connection.latestContext
      if (restoreContext && context?.kind === 'proposal' && context.proposalId && event.event === 'frame') {
        const switched = await connection.request({ action: 'context', proposalId: context.proposalId },
          ACTION_TIMEOUT_MS, { quietError: true })
        if (switched.event === 'error') {
          note = STALE_CONTEXT_REASON
          await this.clearStaleContext(connection)
          try {
            await connection.request({ action: 'context', proposalId: null }, ACTION_TIMEOUT_MS, { quietError: true })
          } catch {
          }
          return { event, note }
        }
        event = await connection.request({ action: 'reload' }, ACTION_TIMEOUT_MS)
      }
      return { event, note }
    } finally {
      if (SAVED_ACTIONS.has(action.action)) await this.save(connection)
      await this.release(connection, 'borrowed')
    }
  }

  async qualityTarget(agent: Agent, signal: AbortSignal, id: string): Promise<QualityTarget> {
    const connection = await this.open(agent, signal, false)
    try {
      if (connection.latestFrame?.waiting) {
        throw new Error('no diagram: the panel shows nothing yet; run uml_ir and uml_display first')
      }
      const answer = await connection.request({ action: 'quality-target', id }, ACTION_TIMEOUT_MS, { quiet: true })
      if (answer.event === 'error') throw new Error(answer.message)
      if (answer.event !== 'quality-target') {
        throw new Error(`uml-viewer resident answered quality-target with ${answer.event}`)
      }
      return validateQualityTarget(answer.target, 'quality target', id)
    } finally {
      await this.release(connection, 'borrowed')
    }
  }

  private save(connection: ResidentConnection): Promise<void> {
    return saveView(this.states, connection)
  }

  async shownView(agent: Agent, signal: AbortSignal): Promise<{ frame: SceneFrame | null; context: DiscussionContext }> {
    const connection = await this.open(agent, signal, false)
    try {
      return { frame: connection.latestFrame, context: connection.latestContext ?? SOURCE_CONTEXT }
    } finally {
      await this.release(connection, 'borrowed')
    }
  }

  private async clearStaleContext(connection: ResidentConnection): Promise<void> {
    connection.latestContext = SOURCE_CONTEXT
    try {
      const state = await this.states.read(connection.id)
      if (state) {
        await this.states.write(connection.id, { ...state, context: SOURCE_CONTEXT, proposalId: null, openLayer: null })
      }
    } catch {
    }
  }

  private async stop(connection: ResidentConnection): Promise<void> {
    this.clearGrace(connection.id)
    if (connection.isSettled) return
    if (this.pool.connections.get(connection.id) === connection) this.pool.connections.delete(connection.id)
    connection.holders.clear()
    await this.save(connection)
    try {
      await connection.request({ action: 'quit' }, STOP_TIMEOUT_MS)
    } catch {
    }
    connection.end()
    const exited = await Promise.race([connection.exited.then(() => true),
      new Promise<boolean>(resolve => setTimeout(() => resolve(false), STOP_TIMEOUT_MS))])
    if (!exited) {
      connection.child.terminate()
      await Promise.race([connection.exited, new Promise(resolve => setTimeout(resolve, STOP_TIMEOUT_MS))])
    }
  }

  async checkpoint(): Promise<void> {
    for (const connection of [...this.pool.connections.values()]) await this.save(connection)
  }

  async dispose(): Promise<void> {
    await disposePool(this.states, this.pool)
  }
}
