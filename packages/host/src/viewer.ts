import type { PanelAck, PanelAction, PanelEvent, ResidentEvent } from '@dsh-uml-viewer/contracts'
import { parsePanelAction } from '@dsh-uml-viewer/contracts'
import type { Agent } from './dsh.js'
import type { PanelPrompts } from './prompt.js'
import type { PanelRequests } from './requests.js'
import { Refusal, ResidentSessions } from './resident.js'

function reasonOf(error: unknown): string {
  return error instanceof Error ? error.message : String(error)
}

/** Map one resident line to the panel event the browser receives; `null` means "nothing to show". */
export function panelEvent(event: ResidentEvent): PanelEvent | null {
  switch (event.event) {
    case 'frame':
      return { kind: 'frame', seq: typeof event.seq === 'number' ? event.seq : null, frame: event.frame }
    case 'context':
      return { kind: 'context', context: event.context }
    case 'state':
      return { kind: 'state', state: event.state }
    case 'search':
      return { kind: 'search', query: event.query, results: event.results }
    // #27: inspector/card/source are answers to one panel's action and travel
    // on that action's ack, never into every watching panel's stream.
    case 'inspector':
    case 'card':
    case 'source':
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
  ) {}

  async *watch(agent: Agent, signal: AbortSignal): AsyncIterable<PanelEvent> {
    let connection
    try {
      connection = await this.sessions.open(agent, signal)
    } catch (error) {
      yield error instanceof Refusal ? { kind: 'refused', reason: error.message } : { kind: 'failed', reason: reasonOf(error) }
      return
    }
    this.prompts.openPanel(agent)
    const queue = connection.attach()
    try {
      yield { kind: 'ready', actions: [...connection.actions] }
      if (connection.latestContext) yield { kind: 'context', context: connection.latestContext }
      if (connection.latestFrame) yield { kind: 'frame', seq: null, frame: connection.latestFrame }
      for (;;) {
        const next = await queue.next(signal)
        if (next.done) return
        const event = panelEvent(next.value)
        if (event) yield event
      }
    } finally {
      connection.detach(queue)
      this.prompts.closePanel(agent.session.id)
      await this.sessions.release(connection)
    }
  }

  async action(agent: Agent, action: PanelAction): Promise<PanelAck> {
    try {
      // #28: a quality refresh is a message to this panel's session, not a
      // resident action. Only the element id and op cross from the browser.
      if (action.action === 'quality-request') {
        // Re-parse: the carrier validates too, but the delivered message is
        // built only from fields this validator accepted.
        const request = parsePanelAction(action)
        if (request.action !== 'quality-request') throw new Error('quality-request expected')
        return await this.requests.deliver(agent, request.request, request.id)
      }
      const { event } = await this.sessions.refresh(agent, new AbortController().signal, action as unknown as { action: string })
      if (event.event === 'error') return { accepted: false, reason: event.message }
      const ack: PanelAck = { accepted: true, reason: null }
      // The overlay models ride on the ack of the action that asked for them.
      if (event.event === 'inspector') ack.inspector = event.inspector
      else if (event.event === 'card') ack.card = event.card
      else if (event.event === 'source') ack.source = event.source
      return ack
    } catch (error) {
      return { accepted: false, reason: reasonOf(error) }
    }
  }
}
