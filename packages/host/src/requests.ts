import { randomUUID } from 'node:crypto'
import type { PanelAck, PanelRequest, PanelRequestTarget, QualityRequestOp, RequestOp } from '@dsh-uml-viewer/contracts'
import {
  defaultProposalName, renderContextRequest, renderNewProposalRequest, renderOmitRequest, renderPanelRequest,
} from '@dsh-uml-viewer/contracts'
import type { Agent, HostServices } from './dsh.js'
import type { GrantKind, RequestGrants } from './gate.js'
import { RequestStatuses } from './request-status.js'
import type { ResidentSessions } from './resident.js'

function reasonOf(error: unknown): string {
  return error instanceof Error ? error.message : String(error)
}

/**
 * How long one panel request may spend being resolved and admitted before it
 * fails with a reason (#review H3). One resident action is bounded by
 * `ACTION_TIMEOUT_MS`; admission also waits for DSH, so it needs its own bound:
 * a membership lookup or a prompt that never settles must not hold this
 * session's request FIFO, and with it every later click, forever.
 */
export const ADMISSION_TIMEOUT_MS = 20_000

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
 * hide after a context switch names the new context. Each admission is bounded
 * by `ADMISSION_TIMEOUT_MS`: one that never resolves fails with a reason and
 * the next request still runs.
 *
 * Each admission outcome is the request's first request status (#51): an
 * admitted request is queued, a context switch is done once admitted (it needs
 * no companion run), and a refused one fails with the admission reason. From
 * there #52 attributes the companion's `uml_*` calls to the session's queue.
 */
export class PanelRequests {
  private readonly tails = new Map<string, Promise<void>>()

  constructor(
    private readonly services: HostServices,
    private readonly sessions: ResidentSessions,
    private readonly grants: RequestGrants,
    private readonly admissionTimeoutMs = ADMISSION_TIMEOUT_MS,
    /**
     * Where each request's status is kept for the session's panels (#51). Its
     * FIFO is also the attribution rule of #52: a display completes the head,
     * and `uml_ir({request})` names one entry exactly.
     */
    readonly statuses = new RequestStatuses(),
  ) {}

  /** A right-click quality refresh of one element. */
  deliver(agent: Agent, op: QualityRequestOp, id: string): Promise<PanelAck> {
    let named: PanelRequestTarget | null = null
    return this.enqueue(agent, op, async signal => {
      try {
        const target = await this.sessions.qualityTarget(agent, signal, id)
        named = { id: target.id, kind: target.kind }
        const request: PanelRequest = { op, target: named, files: target.files }
        return { text: renderPanelRequest(request) }
      } catch (error) {
        // A refused request still names the element the browser clicked, when
        // the shown diagram can give its kind (#52 F26); a target the resident
        // could not resolve leaves the entry naming no element.
        named ??= await this.shownTarget(agent, signal, id)
        throw error
      }
    }, () => named)
  }

  /** Hide one element of the shown diagram, in the current discussion context. */
  omit(agent: Agent, id: string): Promise<PanelAck> {
    let named: PanelRequestTarget | null = null
    return this.enqueue(agent, 'omit', async signal => {
      const { frame, context } = await this.sessions.shownView(agent, signal)
      if (!frame || frame.waiting) throw new Error('no diagram: the panel shows nothing yet; run uml_ir and uml_display first')
      const element = frame.world.find(item => (item.kind === 'class' || item.kind === 'package') && item.id === id)
      if (!element) throw new Error(`no class or namespace component ${id} in the shown diagram`)
      // Named before the policy checks: a refusal still says which element of
      // the diagram it was about (#52 F26).
      named = { id, kind: element.kind === 'package' ? 'component' : 'class' }
      if (element.kind === 'class' && element.shape === 'oval') throw new Error(`${id} is a foreign library; it is not in the policy and cannot be hidden`)
      return { kind: 'omit', text: (request: string) => renderOmitRequest({ target: named!, context, request }) }
    }, () => named)
  }

  /**
   * Choose the discussion context: the resident switches the view first (as
   * the `context` action always did), then the companion is told which
   * diagram is under discussion. A refused switch sends nothing.
   */
  context(agent: Agent, proposalId: string | null): Promise<PanelAck> {
    return this.enqueue(agent, 'context', async signal => {
      const { event } = await this.sessions.refresh(agent, signal, { action: 'context', proposalId })
      if (event.event === 'error') throw new Error(event.message)
      const { context } = await this.sessions.shownView(agent, signal)
      return { text: renderContextRequest(context) }
    })
  }

  /** Add a named proposal; without a name it is the local time to the second. */
  newProposal(agent: Agent, name: string | undefined): Promise<PanelAck> {
    return this.enqueue(agent, 'new-proposal', async () => {
      const proposal = name ?? defaultProposalName(new Date())
      return { kind: 'new-proposal', text: (request: string) => renderNewProposalRequest({ name: proposal, request }) }
    })
  }

  /**
   * Admit one request in this session's click order. `named` reports the
   * element the delivered message names (if any) once it is known; it becomes
   * the entry's target, which the display that answers the request reports
   * (#54). The outcome becomes the request's status entry (#51).
   */
  private enqueue(
    agent: Agent,
    op: RequestOp,
    resolve: (signal: AbortSignal) => Promise<Message>,
    named?: () => PanelRequestTarget | null,
  ): Promise<PanelAck> {
    const sessionId = agent.session.id
    const previous = this.tails.get(sessionId) ?? Promise.resolve()
    const result = previous.then(() => this.admit(agent, resolve)).then(({ ack, id, promptId }) => {
      this.statuses.record(sessionId, {
        id, op, target: named ? named() : null,
        // A context switch already happened in the resident; nothing is left to run.
        state: ack.accepted ? op === 'context' ? 'done' : 'queued' : 'failed',
        reason: ack.accepted ? null : ack.reason ?? 'the request was not admitted',
        exitCode: null,
      }, promptId)
      return ack
    })
    const tail = result.then(() => undefined, () => undefined)
    this.tails.set(sessionId, tail)
    void tail.then(() => {
      if (this.tails.get(sessionId) === tail) this.tails.delete(sessionId)
    })
    return result
  }

  /**
   * The element a refused request still names (#52 F26): what the shown
   * diagram says about the id the browser clicked. Null when the diagram has
   * no such element — then the clicked id names nothing the panel can show.
   */
  private async shownTarget(agent: Agent, signal: AbortSignal, id: string): Promise<PanelRequestTarget | null> {
    try {
      const { frame } = await this.sessions.shownView(agent, signal)
      const element = frame?.world.find(item => (item.kind === 'class' || item.kind === 'package') && item.id === id)
      if (!element) return null
      return { id, kind: element.kind === 'package' ? 'component' : 'class' }
    } catch {
      // The diagram could not be read either: the refusal's own reason stands.
      return null
    }
  }

  /**
   * Resolve and admit one request. `id` names it in the request status: the
   * grant id `uml_ir({request})` presents for a granted request (so #52 can
   * match that call exactly), otherwise the admitted message's correlation id.
   */
  private async admit(agent: Agent, resolve: (signal: AbortSignal) => Promise<Message>): Promise<{ ack: PanelAck; id: string; promptId: string }> {
    const sessionId = agent.session.id
    let grant: string | null = null
    const requestId = randomUUID()
    try {
      const controller = this.services.sessionController
      if (!controller) {
        return { id: requestId, promptId: requestId, ack: { accepted: false,
          reason: 'this DSH has no sessionController service, so the panel cannot queue a request message' } }
      }
      const ack = await this.withDeadline(async signal => {
        const message = await resolve(signal)
        let text: string
        if (typeof message.text === 'string') {
          text = message.text
        } else {
          grant = this.grants.issue(sessionId, message.kind!)
          text = message.text(grant)
        }
        await controller.prompt({
          requestId,
          sessionId,
          mode: 'queue',
          content: [{ type: 'text', text }],
        }, signal)
        return { accepted: true, reason: null }
      })
      return { ack, id: grant ?? requestId, promptId: requestId }
    } catch (error) {
      // A message that never reached the session leaves no usable grant.
      if (grant !== null) this.grants.revoke(sessionId, grant)
      return { id: grant ?? requestId, promptId: requestId, ack: { accepted: false, reason: reasonOf(error) } }
    }
  }

  /**
   * One request, one signal, one deadline. The signal reaches both waits this
   * request owns — the resident's answer and DSH's prompt — and the deadline
   * fails the request even if the step it cancelled never returns.
   */
  private withDeadline<T>(task: (signal: AbortSignal) => Promise<T>): Promise<T> {
    const controller = new AbortController()
    const reason = `the panel request was not admitted within ${Math.round(this.admissionTimeoutMs / 1000)}s; `
      + 'it was abandoned so later requests of this session can run'
    let timer: NodeJS.Timeout | undefined
    const deadline = new Promise<never>((_resolve, reject) => {
      timer = setTimeout(() => {
        controller.abort(new Error(reason))
        reject(new Error(reason))
      }, this.admissionTimeoutMs)
    })
    const work = task(controller.signal)
    // Both promises keep their handlers, so a late rejection is never unhandled.
    return Promise.race([work, deadline]).finally(() => clearTimeout(timer))
  }
}

/** One message to admit; a granted request renders its text around its grant id. */
type Message = { kind?: undefined; text: string } | { kind: GrantKind; text: (request: string) => string }
