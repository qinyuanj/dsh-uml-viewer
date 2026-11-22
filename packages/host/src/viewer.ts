import type { PanelAck, PanelAction, PanelCloseAck, PanelEvent, ResidentEvent } from '@dsh-uml-viewer/contracts'
import { parsePanelAction, parsePanelLanguage, parsePanelOwner, type PanelOwner } from '@dsh-uml-viewer/contracts'
import type { Agent } from './dsh.js'
import type { PanelPrompts } from './prompt.js'
import { RequestStatuses } from './request-status.js'
import type { PanelRequests } from './requests.js'
import { Refusal, ResidentSessions } from './resident.js'

function reasonOf(error: unknown): string {
  return error instanceof Error ? error.message : String(error)
}

export function panelEvent(event: ResidentEvent): PanelEvent | null {
  switch (event.event) {
    case 'frame':
      return { kind: 'frame', seq: typeof event.seq === 'number' ? event.seq : null, frame: event.frame }
    case 'context':
      return { kind: 'context', context: event.context }
    case 'state':
      return { kind: 'state', state: event.state, breadcrumbs: event.breadcrumbs, status: event.status,
        proposals: event.proposals }
    case 'search':
      return { kind: 'search', query: event.query, results: event.results }
    case 'card':
    case 'source':
    case 'quality-target':
      return null
    case 'error':
      return { kind: 'failed', reason: event.message }
    default:
      return null
  }
}

/**
 * The panel's typed Remote. `watch` binds a panel to one session's resident;
 * `action` forwards one validated panel action (viewport, camera, drill-in,
 * select, search, save) to that resident.
 */
export class ViewerService {
  /** The gateway discovers this public binding when the Remote is mounted. */
  readonly typertRemote = { service: this, serviceKey: 'umlViewer', namespace: 'umlViewer' }

  constructor(
    private readonly sessions: ResidentSessions,
    private readonly prompts: PanelPrompts,
    private readonly requests: PanelRequests,
    private readonly statuses: RequestStatuses = new RequestStatuses(),
  ) {}

  /**
   * The owned panel stream. The gateway passes the descriptor's parameters in
   * order and the cancellation signal last; `language` is the DSH language the
   * panel passes (#44), `null` from a panel that predates it.
   */
  watchOwned(agent: Agent, owner: PanelOwner, language: string | null | undefined, signal: AbortSignal): AsyncIterable<PanelEvent> {
    return this.watch(agent, signal, parsePanelOwner(owner), language)
  }

  async closeOwned(agent: Agent, owner: PanelOwner): Promise<PanelCloseAck> {
    return { closed: await this.sessions.closeOwned(agent, parsePanelOwner(owner)) }
  }

  /**
   * Open this session's panel stream. `language` is the DSH language the panel
   * passes (#44): DSH has one language for every panel, so the latest one wins
   * and no view state records it.
   */
  async *watch(agent: Agent, signal: AbortSignal, owner?: PanelOwner, language?: string | null): AsyncIterable<PanelEvent> {
    let connection
    try {
      const locale = parsePanelLanguage(language)
      connection = owner
        ? await this.sessions.openOwned(agent, signal, owner, locale)
        : await this.sessions.open(agent, signal, true, locale)
    } catch (error) {
      yield error instanceof Refusal ? { kind: 'refused', reason: error.message } : { kind: 'failed', reason: reasonOf(error) }
      return
    }
    this.prompts.openPanel(agent)
    const queue = connection.attach()
    const sessionId = agent.session.id
    let statusChanged = true
    let wake: (() => void) | null = null
    const unsubscribe = this.statuses.subscribe(sessionId, () => {
      statusChanged = true
      wake?.()
    })
    try {
      yield { kind: 'ready', actions: [...connection.actions] }
      if (connection.latestContext) yield { kind: 'context', context: connection.latestContext }
      if (connection.latestFrame) yield { kind: 'frame', seq: null, frame: connection.latestFrame }
      let reading: Promise<IteratorResult<ResidentEvent>> | null = null
      for (;;) {
        if (statusChanged) {
          statusChanged = false
          yield { kind: 'request-status', requests: [...this.statuses.list(sessionId)] }
          continue
        }
        reading ??= queue.next(signal)
        const woken = new Promise<null>(resolve => { wake = () => resolve(null) })
        const next = await Promise.race([reading, woken])
        wake = null
        if (next === null) continue
        reading = null
        if (next.done) return
        const event = panelEvent(next.value)
        if (event) yield event
      }
    } finally {
      unsubscribe()
      connection.detach(queue)
      this.prompts.closePanel(agent.session.id)
      // A stream ending is a carrier loss, not a panel close: the gateway aborts
      if (owner) await this.sessions.releaseOwned(connection, owner)
      else await this.sessions.release(connection, connection.closeRequested ? 'closed' : 'withdrawn')
    }
  }

  /**
   * The panel's explicit close, and the only definite "this panel is gone"
   * signal: it stops the session's resident at once (when no other panel holds
   * it) instead of waiting out the withdrawal grace.
   */
  async close(agent: Agent): Promise<PanelCloseAck> {
    return { closed: await this.sessions.close(agent) }
  }

  /**
   * One panel action. The Remote descriptor declares no cancellation parameter
   * for this invocation (only `watch` has one), so `signal` is optional: a
   * gateway that supplies one has it reach the resident, and the action's own
   * timeouts bound it otherwise.
   */
  async action(agent: Agent, action: PanelAction, signal?: AbortSignal): Promise<PanelAck> {
    try {
      if (action.action === 'quality-request') {
        const request = parsePanelAction(action)
        if (request.action !== 'quality-request') throw new Error('quality-request expected')
        return await this.requests.deliver(agent, request.request, request.id)
      }
      if (action.action === 'omit' || action.action === 'new-proposal' || action.action === 'context') {
        const request = parsePanelAction(action)
        if (request.action === 'omit') return await this.requests.omit(agent, request.id)
        if (request.action === 'new-proposal') return await this.requests.newProposal(agent, request.name)
        if (request.action === 'context') return await this.requests.context(agent, request.proposalId)
        throw new Error('view request expected')
      }
      if (action.action === 'dismiss-request') {
        const request = parsePanelAction(action)
        if (request.action !== 'dismiss-request') throw new Error('dismiss-request expected')
        return this.statuses.dismiss(agent.session.id, request.id)
          ? { accepted: true, reason: null }
          : { accepted: false, reason: `no finished request ${request.id} in this session` }
      }
      const { event } = await this.sessions.refresh(agent, signal ?? new AbortController().signal, action as unknown as { action: string })
      if (event.event === 'error') return { accepted: false, reason: event.message }
      const ack: PanelAck = { accepted: true, reason: null }
      if (event.event === 'card') ack.card = event.card
      else if (event.event === 'source') ack.source = event.source
      return ack
    } catch (error) {
      return { accepted: false, reason: reasonOf(error) }
    }
  }
}
