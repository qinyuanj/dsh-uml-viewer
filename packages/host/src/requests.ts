import { randomUUID } from 'node:crypto'
import { readFile, stat } from 'node:fs/promises'
import path from 'node:path'
import type { PanelAck, PanelRequest, PanelRequestTarget, QualityRequestOp } from '@dsh-uml-viewer/contracts'
import { renderPanelRequest } from '@dsh-uml-viewer/contracts'
import type { Agent, HostServices } from './dsh.js'
import { EdnMap, readEdn } from './edn.js'
import type { Edn } from './edn.js'
import type { ResidentSessions } from './resident.js'

/** The IR is generated text; a diagram over this is not read for a request. */
const DIAGRAM_MAX_BYTES = 64 * 1024 * 1024

function keywordOf(value: Edn | undefined): string | null {
  return value !== null && typeof value === 'object' && 'keyword' in value ? `:${value.keyword}` : null
}

/**
 * The source files of one diagram element: the class itself, or every class
 * of the namespace component (`id` or under `id.`), each file once, in diagram
 * order. `null` when nothing in the diagram has that id.
 * @param document the parsed IR.
 * @param id the element's EDN keyword id.
 */
export function targetFiles(document: Edn, id: string, proposalId: string | null = null): { target: PanelRequestTarget; files: string[] } | null {
  if (!(document instanceof EdnMap)) throw new Error('the diagram is not an EDN map')
  const classes = document.get('classes')
  if (!Array.isArray(classes)) throw new Error('the diagram has no :classes vector')
  const members = id.startsWith(':proposal.') ? proposalMembers(document, id, proposalId, classes as readonly Edn[]) : null
  if (id.startsWith(':proposal.') && members === null) return null
  let exact = false
  let inside = members !== null
  const files: string[] = []
  for (const entry of classes as readonly Edn[]) {
    if (!(entry instanceof EdnMap)) continue
    const classId = keywordOf(entry.get('id'))
    if (classId === null) continue
    const self = classId === id
    const nested = classId.startsWith(`${id}.`)
    if (members ? !members.has(classId) : !self && !nested) continue
    exact ||= self
    inside ||= nested
    const file = entry.get('file')
    if (typeof file !== 'string' || !file) continue
    const normalized = file.replaceAll('\\', '/')
    if (!files.includes(normalized)) files.push(normalized)
  }
  if (!files.length) return null
  // A namespace node with classes under it is a component, even when a class
  // (e.g. a Rust `lib.rs`) carries the same id.
  return { target: { id, kind: inside || !exact ? 'component' : 'class' }, files }
}

function names(value: Edn | undefined): string[] {
  if (!Array.isArray(value)) return []
  return (value as readonly Edn[]).flatMap(entry => {
    if (entry instanceof EdnMap) return names(entry.get('nses'))
    if (entry !== null && typeof entry === 'object') {
      if ('keyword' in entry) return [`:${entry.keyword}`]
      if ('symbol' in entry) return [`:${entry.symbol}`]
    }
    return []
  })
}

function under(id: string, parent: string): boolean { return id === parent || id.startsWith(`${parent}.`) }

/** Resolve the grouping named by the resident's current proposal, including nested groups. */
function proposalMembers(document: EdnMap, id: string, proposalId: string | null, classes: readonly Edn[]): Set<string> | null {
  if (!proposalId) return null
  const proposals = document.get('proposals')
  if (!Array.isArray(proposals)) return null
  const proposal = (proposals as readonly Edn[]).find(entry => entry instanceof EdnMap && keywordOf(entry.get('id')) === proposalId)
  if (!(proposal instanceof EdnMap)) return null
  const layers = proposal.get('layers')
  if (!Array.isArray(layers)) return null
  let selected: string[] | null = null
  const find = (entries: readonly Edn[]): void => {
    for (const entry of entries) {
      if (!(entry instanceof EdnMap)) continue
      const group = keywordOf(entry.get('id'))
      if (group && (group === id || `:proposal.${group.slice(1)}` === id)) selected = names(entry.get('nses'))
      const children = entry.get('nses')
      if (Array.isArray(children)) find(children as readonly Edn[])
    }
  }
  find(layers as readonly Edn[])
  if (selected === null && id !== ':proposal.unassigned') return null
  const claimed = (layers as readonly Edn[]).flatMap(entry => entry instanceof EdnMap ? names(entry.get('nses')) : [])
  const omitted = [...names(document.get('omit')), ...names(proposal.get('omit'))]
  const available = classes.filter((entry): entry is EdnMap => entry instanceof EdnMap && entry.get('foreign') !== true)
    .map(entry => keywordOf(entry.get('id'))).filter((classId): classId is string => classId !== null)
    .filter(classId => !omitted.some(parent => under(classId, parent)))
  // As the scene does: a group naming an exact class takes that class, any
  // other name is the namespace subtree; `unassigned` holds the rest.
  if (selected === null) return new Set(available.filter(classId => !claimed.some(parent => under(classId, parent))))
  const picks = selected as string[]
  return new Set(available.filter(classId =>
    picks.some(parent => available.includes(parent) ? classId === parent : under(classId, parent))))
}

function reasonOf(error: unknown): string {
  return error instanceof Error ? error.message : String(error)
}

/**
 * Right-click quality requests (#28). Each one becomes one visible user
 * message queued in the panel's own session through DSH's
 * `sessionController.prompt` (`mode: 'queue'`), never the composer. Requests
 * of a session are resolved and admitted one at a time, in the order the
 * panel sent them, so DSH's queue holds them in click order.
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
    const { cwd, diagram, proposalId } = await this.sessions.shownDiagram(agent, new AbortController().signal)
    if (!diagram) throw new Error('the panel shows no diagram yet; run uml_ir and uml_display first')
    const file = path.resolve(cwd, diagram)
    const size = (await stat(file)).size
    if (size > DIAGRAM_MAX_BYTES) throw new Error(`the diagram ${diagram} is over ${DIAGRAM_MAX_BYTES} bytes`)
    const resolved = targetFiles(readEdn(await readFile(file, 'utf8')), id, proposalId)
    if (!resolved) throw new Error(`no source files in the shown diagram for ${id}; refresh a class or a namespace component`)
    return { op, ...resolved }
  }
}
