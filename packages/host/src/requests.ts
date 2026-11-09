import { randomUUID } from 'node:crypto'
import type { PanelAck, PanelRequest, QualityRequestOp } from '@dsh-uml-viewer/contracts'
import { renderPanelRequest } from '@dsh-uml-viewer/contracts'
import type { Agent, HostServices } from './dsh.js'
import type { ResidentSessions } from './resident.js'

function reasonOf(error: unknown): string {
  return error instanceof Error ? error.message : String(error)
}

/**
 * Right-click quality requests (#28). Each one becomes one visible user
 * message queued in the panel's own session through DSH's
 * `sessionController.prompt` (`mode: 'queue'`), never the composer.
 *
 * What the target covers is the resident's answer (`quality-target`): it
 * resolves the id against the diagram it has loaded, with the scene's own
 * grouping rules. This class never reads the IR, never re-derives proposal
 * membership and never uses a file the client named; it validates the
 * returned target and renders it.
 *
 * Requests of a session are resolved and admitted one at a time, in the order
 * the panel sent them, so DSH's queue holds them in click order.
 */
export class PanelRequests {
  private readonly tails = new Map<string, Promise<void>>()

  constructor(private readonly services: HostServices, private readonly sessions: ResidentSessions) {}

  deliver(agent: Agent, op: QualityRequestOp, id: string): Promise<PanelAck> {
    const sessionId = agent.session.id
    const previous = this.tails.get(sessionId) ?? Promise.resolve()
    const result = previous.then(() => this.admit(agent, op, id))
    const tail = result.then(() => undefined, () => undefined)
    this.tails.set(sessionId, tail)
    void tail.then(() => {
      if (this.tails.get(sessionId) === tail) this.tails.delete(sessionId)
    })
    return result
  }

  private async admit(agent: Agent, op: QualityRequestOp, id: string): Promise<PanelAck> {
    try {
      const controller = this.services.sessionController
      if (!controller) {
        return { accepted: false, reason: 'this DSH has no sessionController service, so the panel cannot queue a request message' }
      }
      const request = await this.resolve(agent, op, id)
      const text = renderPanelRequest(request)
      await controller.prompt({
        requestId: randomUUID(),
        sessionId: agent.session.id,
        mode: 'queue',
        content: [{ type: 'text', text }],
      }, new AbortController().signal)
      return { accepted: true, reason: null }
    } catch (error) {
      return { accepted: false, reason: reasonOf(error) }
    }
  }

  private async resolve(agent: Agent, op: QualityRequestOp, id: string): Promise<PanelRequest> {
    const target = await this.sessions.qualityTarget(agent, new AbortController().signal, id)
    return { op, target: { id: target.id, kind: target.kind }, files: target.files }
  }
}
