import { randomUUID } from 'node:crypto'
import type { PanelAck, PanelRequest, PanelRequestTarget, QualityRequestOp } from '@dsh-uml-viewer/contracts'
import {
  defaultProposalName, renderContextRequest, renderNewProposalRequest, renderOmitRequest, renderPanelRequest,
} from '@dsh-uml-viewer/contracts'
import type { Agent, HostServices } from './dsh.js'
import type { GrantKind, RequestGrants } from './gate.js'
import type { ResidentSessions } from './resident.js'

function reasonOf(error: unknown): string {
  return error instanceof Error ? error.message : String(error)
}

/**
 * Panel requests: each one becomes one visible user message queued in the
 * panel's own session through DSH's `sessionController.prompt`
 * (`mode: 'queue'`), never the composer, so the user's draft is untouched.
 *
 * - Quality refreshes (#28). What the target covers is the resident's answer
 *   (`quality-target`): this class never reads the IR, never re-derives
 *   proposal membership and never uses a file the client named.
 * - Hiding an element, choosing the discussion context and adding a proposal
 *   (#29). They need no quality refresh; the companion edits the policy in
 *   place. The ones that regenerate the IR carry a single-use grant id that
 *   `uml_ir({request})` presents.
 *
 * Every request of a session is resolved and admitted one at a time, in the
 * order the panel sent it, so DSH's queue holds them in click order and a
 * hide after a context switch names the new context.
 */
export class PanelRequests {
  private readonly tails = new Map<string, Promise<void>>()

  constructor(
    private readonly services: HostServices,
    private readonly sessions: ResidentSessions,
    private readonly grants: RequestGrants,
  ) {}

  /** A right-click quality refresh of one element. */
  deliver(agent: Agent, op: QualityRequestOp, id: string): Promise<PanelAck> {
    return this.enqueue(agent, async () => {
      const target = await this.sessions.qualityTarget(agent, new AbortController().signal, id)
      const request: PanelRequest = { op, target: { id: target.id, kind: target.kind }, files: target.files }
      return { text: renderPanelRequest(request) }
    })
  }

  /** Hide one element of the shown diagram, in the current discussion context. */
  omit(agent: Agent, id: string): Promise<PanelAck> {
    return this.enqueue(agent, async () => {
      const { frame, context } = await this.sessions.shownView(agent, new AbortController().signal)
      if (!frame || frame.waiting) throw new Error('no diagram: the panel shows nothing yet; run uml_ir and uml_display first')
      const element = frame.world.find(item => (item.kind === 'class' || item.kind === 'package') && item.id === id)
      if (!element) throw new Error(`no class or namespace component ${id} in the shown diagram`)
      if (element.kind === 'class' && element.shape === 'oval') throw new Error(`${id} is a foreign library; it is not in the policy and cannot be hidden`)
      const target: PanelRequestTarget = { id, kind: element.kind === 'package' ? 'component' : 'class' }
      return { kind: 'omit', text: (request: string) => renderOmitRequest({ target, context, request }) }
    })
  }

  /**
   * Choose the discussion context: the resident switches the view first (as
   * the `context` action always did), then the companion is told which
   * diagram is under discussion. A refused switch sends nothing.
   */
  context(agent: Agent, proposalId: string | null): Promise<PanelAck> {
    return this.enqueue(agent, async () => {
      const { event } = await this.sessions.refresh(agent, new AbortController().signal, { action: 'context', proposalId })
      if (event.event === 'error') throw new Error(event.message)
      const { context } = await this.sessions.shownView(agent, new AbortController().signal)
      return { text: renderContextRequest(context) }
    })
  }

  /** Add a named proposal; without a name it is the local time to the second. */
  newProposal(agent: Agent, name: string | undefined): Promise<PanelAck> {
    return this.enqueue(agent, async () => {
      const proposal = name ?? defaultProposalName(new Date())
      return { kind: 'new-proposal', text: (request: string) => renderNewProposalRequest({ name: proposal, request }) }
    })
  }

  private enqueue(agent: Agent, resolve: () => Promise<Message>): Promise<PanelAck> {
    const sessionId = agent.session.id
    const previous = this.tails.get(sessionId) ?? Promise.resolve()
    const result = previous.then(() => this.admit(agent, resolve))
    const tail = result.then(() => undefined, () => undefined)
    this.tails.set(sessionId, tail)
    void tail.then(() => {
      if (this.tails.get(sessionId) === tail) this.tails.delete(sessionId)
    })
    return result
  }

  private async admit(agent: Agent, resolve: () => Promise<Message>): Promise<PanelAck> {
    const sessionId = agent.session.id
    let grant: string | null = null
    try {
      const controller = this.services.sessionController
      if (!controller) {
        return { accepted: false, reason: 'this DSH has no sessionController service, so the panel cannot queue a request message' }
      }
      const message = await resolve()
      let text: string
      if (typeof message.text === 'string') {
        text = message.text
      } else {
        grant = this.grants.issue(sessionId, message.kind!)
        text = message.text(grant)
      }
      await controller.prompt({
        requestId: randomUUID(),
        sessionId,
        mode: 'queue',
        content: [{ type: 'text', text }],
      }, new AbortController().signal)
      return { accepted: true, reason: null }
    } catch (error) {
      // A message that never reached the session leaves no usable grant.
      if (grant !== null) this.grants.revoke(sessionId, grant)
      return { accepted: false, reason: reasonOf(error) }
    }
  }
}

/** One message to admit; a granted request renders its text around its grant id. */
type Message = { kind?: undefined; text: string } | { kind: GrantKind; text: (request: string) => string }
