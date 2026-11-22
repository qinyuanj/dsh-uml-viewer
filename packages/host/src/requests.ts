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

export const ADMISSION_TIMEOUT_MS = 20_000

export class PanelRequests {
  private readonly tails = new Map<string, Promise<void>>()

  constructor(
    private readonly services: HostServices,
    private readonly sessions: ResidentSessions,
    private readonly grants: RequestGrants,
    private readonly admissionTimeoutMs = ADMISSION_TIMEOUT_MS,
    readonly statuses = new RequestStatuses(),
  ) {}

  deliver(agent: Agent, op: QualityRequestOp, id: string): Promise<PanelAck> {
    let named: PanelRequestTarget | null = null
    return this.enqueue(agent, op, async signal => {
      try {
        const target = await this.sessions.qualityTarget(agent, signal, id)
        named = { id: target.id, kind: target.kind }
        const request: PanelRequest = { op, target: named, files: target.files }
        return { text: renderPanelRequest(request) }
      } catch (error) {
        named ??= await this.shownTarget(agent, signal, id)
        throw error
      }
    }, () => named)
  }

  omit(agent: Agent, id: string): Promise<PanelAck> {
    let named: PanelRequestTarget | null = null
    return this.enqueue(agent, 'omit', async signal => {
      const { frame, context } = await this.sessions.shownView(agent, signal)
      if (!frame || frame.waiting) throw new Error('no diagram: the panel shows nothing yet; run uml_ir and uml_display first')
      const element = frame.world.find(item => (item.kind === 'class' || item.kind === 'package') && item.id === id)
      if (!element) throw new Error(`no class or namespace component ${id} in the shown diagram`)
      named = { id, kind: element.kind === 'package' ? 'component' : 'class' }
      if (element.kind === 'class' && element.shape === 'oval') throw new Error(`${id} is a foreign library; it is not in the policy and cannot be hidden`)
      return { kind: 'omit', text: (request: string) => renderOmitRequest({ target: named!, context, request }) }
    }, () => named)
  }

  context(agent: Agent, proposalId: string | null): Promise<PanelAck> {
    return this.enqueue(agent, 'context', async signal => {
      const { event } = await this.sessions.refresh(agent, signal, { action: 'context', proposalId })
      if (event.event === 'error') throw new Error(event.message)
      const { context } = await this.sessions.shownView(agent, signal)
      return { text: renderContextRequest(context) }
    })
  }

  newProposal(agent: Agent, name: string | undefined): Promise<PanelAck> {
    return this.enqueue(agent, 'new-proposal', async () => {
      const proposal = name ?? defaultProposalName(new Date())
      return { kind: 'new-proposal', text: (request: string) => renderNewProposalRequest({ name: proposal, request }) }
    })
  }

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

  private async shownTarget(agent: Agent, signal: AbortSignal, id: string): Promise<PanelRequestTarget | null> {
    try {
      const { frame } = await this.sessions.shownView(agent, signal)
      const element = frame?.world.find(item => (item.kind === 'class' || item.kind === 'package') && item.id === id)
      if (!element) return null
      return { id, kind: element.kind === 'package' ? 'component' : 'class' }
    } catch {
      return null
    }
  }

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
      if (grant !== null) this.grants.revoke(sessionId, grant)
      return { id: grant ?? requestId, promptId: requestId, ack: { accepted: false, reason: reasonOf(error) } }
    }
  }

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
    return Promise.race([work, deadline]).finally(() => clearTimeout(timer))
  }
}

type Message = { kind?: undefined; text: string } | { kind: GrantKind; text: (request: string) => string }
