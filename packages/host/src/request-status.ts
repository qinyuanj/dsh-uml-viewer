import type { RequestStatusEntry } from '@dsh-uml-viewer/contracts'

export const REQUEST_STATUS_MAX_ENTRIES = 64

export const TURN_ENDED_REASON = 'the companion ended this turn without completing the display'

function unfinished(entry: RequestStatusEntry): boolean {
  return entry.state === 'queued' || entry.state === 'running'
}

type Listener = (requests: readonly RequestStatusEntry[]) => void

export type RequestEnd =
  | { readonly state: 'done' }
  | { readonly state: 'failed'; readonly reason: string; readonly exitCode?: number | null }

export class RequestStatuses {
  private readonly lists = new Map<string, RequestStatusEntry[]>()
  private readonly listeners = new Map<string, Set<Listener>>()
  private readonly prompts = new Map<string, Map<string, string>>()
  private readonly claimed = new Map<string, Set<string>>()
  private readonly turns = new Map<string, number>()
  private readonly admitted = new Map<string, Map<string, number>>()

  list(sessionId: string): readonly RequestStatusEntry[] {
    return this.lists.get(sessionId) ?? []
  }

  head(sessionId: string): RequestStatusEntry | undefined {
    return this.list(sessionId).find(unfinished)
  }

  record(sessionId: string, entry: RequestStatusEntry, promptId?: string): void {
    const current = this.lists.get(sessionId) ?? []
    const index = current.findIndex(item => item.id === entry.id)
    const next = index < 0 ? [...current, entry] : current.map((item, at) => at === index ? entry : item)
    while (next.length > REQUEST_STATUS_MAX_ENTRIES) {
      const finished = next.findIndex(item => !unfinished(item))
      next.splice(finished < 0 ? 0 : finished, 1)
    }
    const kept = new Set(next.map(item => item.id))
    const prompts = this.prompts.get(sessionId)
    if (prompts) for (const [prompt, id] of prompts) if (!kept.has(id)) prompts.delete(prompt)
    if (promptId !== undefined) {
      let own = this.prompts.get(sessionId)
      if (!own) this.prompts.set(sessionId, own = new Map())
      own.set(promptId, entry.id)
    }
    let admitted = this.admitted.get(sessionId)
    if (!admitted) this.admitted.set(sessionId, admitted = new Map())
    for (const id of admitted.keys()) if (!kept.has(id)) admitted.delete(id)
    if (!admitted.has(entry.id)) admitted.set(entry.id, this.turns.get(sessionId) ?? 0)
    this.lists.set(sessionId, next)
    for (const listener of this.listeners.get(sessionId) ?? []) listener(next)
  }

  begin(sessionId: string, id?: string): RequestStatusEntry | undefined {
    const entry = this.pending(sessionId, id)
    if (!entry) return undefined
    return entry.state === 'running' ? entry : this.replace(sessionId, { ...entry, state: 'running' })
  }

  end(sessionId: string, end: RequestEnd, id?: string): RequestStatusEntry | undefined {
    const entry = this.pending(sessionId, id)
    if (!entry) return undefined
    return this.replace(sessionId, end.state === 'done'
      ? { ...entry, state: 'done' }
      : { ...entry, state: 'failed', reason: end.reason, exitCode: end.exitCode ?? null })
  }

  claim(sessionId: string, promptId: string): void {
    let claimed = this.claimed.get(sessionId)
    if (!claimed) this.claimed.set(sessionId, claimed = new Set())
    claimed.add(promptId)
  }

  turnStarted(sessionId: string): void {
    this.claimed.delete(sessionId)
    this.turns.set(sessionId, (this.turns.get(sessionId) ?? 0) + 1)
  }

  turnEnded(sessionId: string): void {
    const claimed = this.claimed.get(sessionId)
    this.claimed.delete(sessionId)
    const head = this.head(sessionId)
    const settled = new Set<string>()
    for (const promptId of claimed ?? []) {
      const id = this.prompts.get(sessionId)?.get(promptId)
      if (id === undefined || settled.has(id)) continue
      if (this.end(sessionId, { state: 'failed', reason: TURN_ENDED_REASON }, id)) settled.add(id)
    }
    if (!head || settled.has(head.id)) return
    const admitted = this.admitted.get(sessionId)?.get(head.id) ?? 0
    if (admitted >= (this.turns.get(sessionId) ?? 0)) return
    this.end(sessionId, { state: 'failed', reason: TURN_ENDED_REASON }, head.id)
  }

  subscribe(sessionId: string, listener: Listener): () => void {
    let set = this.listeners.get(sessionId)
    if (!set) this.listeners.set(sessionId, set = new Set())
    set.add(listener)
    return () => {
      set.delete(listener)
      if (set.size === 0 && this.listeners.get(sessionId) === set) this.listeners.delete(sessionId)
    }
  }

  dismiss(sessionId: string, id: string): boolean {
    const current = this.lists.get(sessionId) ?? []
    const index = current.findIndex(item => item.id === id && (item.state === 'done' || item.state === 'failed'))
    if (index < 0) return false
    const next = current.filter((_, at) => at !== index)
    this.lists.set(sessionId, next)
    const kept = new Set(next.map(item => item.id))
    const prompts = this.prompts.get(sessionId)
    if (prompts) for (const [prompt, entry] of prompts) if (!kept.has(entry)) prompts.delete(prompt)
    const admitted = this.admitted.get(sessionId)
    if (admitted) for (const entry of admitted.keys()) if (!kept.has(entry)) admitted.delete(entry)
    for (const listener of this.listeners.get(sessionId) ?? []) listener(next)
    return true
  }

  private pending(sessionId: string, id?: string): RequestStatusEntry | undefined {
    return id === undefined
      ? this.head(sessionId)
      : this.list(sessionId).find(item => item.id === id && unfinished(item))
  }

  private replace(sessionId: string, entry: RequestStatusEntry): RequestStatusEntry {
    const next = this.list(sessionId).map(item => item.id === entry.id ? entry : item)
    this.lists.set(sessionId, next)
    for (const listener of this.listeners.get(sessionId) ?? []) listener(next)
    return entry
  }
}
