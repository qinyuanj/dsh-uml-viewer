import type { RequestStatusEntry } from '@dsh-uml-viewer/contracts'

/**
 * The most entries one session keeps. Finished entries are dropped oldest
 * first past it (an unfinished one only when nothing finished is left), so a
 * long session cannot grow the host's memory without bound.
 */
export const REQUEST_STATUS_MAX_ENTRIES = 64

/**
 * Why a request fails when the companion's turn ends without finishing it
 * (#52): the user has to retry or ask again, so the bar says that instead of
 * leaving the request looking unfinished forever.
 */
export const TURN_ENDED_REASON = 'the companion ended this turn without completing the display'

/** A request still waiting for the companion or being handled by it. */
function unfinished(entry: RequestStatusEntry): boolean {
  return entry.state === 'queued' || entry.state === 'running'
}

type Listener = (requests: readonly RequestStatusEntry[]) => void

/** How a request ends (#52): the display showed its diagram, or it failed. */
export type RequestEnd =
  | { readonly state: 'done' }
  | { readonly state: 'failed'; readonly reason: string; readonly exitCode?: number | null }

/**
 * Request status (#51, ADR 0015): each session's panel requests as its panels
 * show them, held in process memory only — DSH restarting forgets them.
 *
 * Panel streams subscribe to their own session and receive the whole list on
 * every change; a session's entries never reach another session's listener.
 * `record` adds an entry or replaces the one with the same id, which is how a
 * later state change (#52) or a dismissal (#53) reaches every open panel.
 *
 * #52 attributes the companion's work to the requests in FIFO order: the head
 * of the list (the oldest entry still queued or in progress) is the request it
 * is handling, so its tool results end that entry. A hide or a new proposal is
 * matched exactly instead, by the id its `uml_ir` presents.
 */
export class RequestStatuses {
  private readonly lists = new Map<string, RequestStatusEntry[]>()
  private readonly listeners = new Map<string, Set<Listener>>()
  /**
   * The prompt correlation id each entry was admitted under, per session: the
   * durable `user/message` echoes it as `source.rpcId`, which is how a turn is
   * known to have consumed this request (#52).
   */
  private readonly prompts = new Map<string, Map<string, string>>()
  /** The entries whose message entered the current turn, per session (#52). */
  private readonly claimed = new Map<string, Set<string>>()

  /**
   * This session's entries, oldest first.
   * @param sessionId the session.
   */
  list(sessionId: string): readonly RequestStatusEntry[] {
    return this.lists.get(sessionId) ?? []
  }

  /**
   * The session's head: the first entry still queued or in progress, in
   * request order — the request the companion is handling or handles first.
   * @param sessionId the session.
   * @returns the head, or undefined when every request of the session ended.
   */
  head(sessionId: string): RequestStatusEntry | undefined {
    return this.list(sessionId).find(unfinished)
  }

  /**
   * Add an entry, or replace the session's entry with the same id in place,
   * then tell the session's listeners.
   * @param sessionId the session the request was sent to.
   * @param entry the entry.
   * @param promptId the prompt correlation id it was admitted under, if any.
   */
  record(sessionId: string, entry: RequestStatusEntry, promptId?: string): void {
    const current = this.lists.get(sessionId) ?? []
    const index = current.findIndex(item => item.id === entry.id)
    const next = index < 0 ? [...current, entry] : current.map((item, at) => at === index ? entry : item)
    while (next.length > REQUEST_STATUS_MAX_ENTRIES) {
      const finished = next.findIndex(item => !unfinished(item))
      next.splice(finished < 0 ? 0 : finished, 1)
    }
    // A dropped entry's correlations must not outlive it.
    const prompts = this.prompts.get(sessionId)
    if (prompts) {
      const kept = new Set(next.map(item => item.id))
      for (const [prompt, id] of prompts) if (!kept.has(id)) prompts.delete(prompt)
    }
    if (promptId !== undefined) {
      let own = this.prompts.get(sessionId)
      if (!own) this.prompts.set(sessionId, own = new Map())
      own.set(promptId, entry.id)
    }
    this.lists.set(sessionId, next)
    for (const listener of this.listeners.get(sessionId) ?? []) listener(next)
  }

  /**
   * Mark a request in progress: the companion's first `uml_*` call after it is
   * admitted (#52). Only an unfinished entry moves; one already in progress
   * stays, so a later call of the same turn changes nothing.
   * @param sessionId the session.
   * @param id the entry a granted `uml_ir` names exactly, if any.
   * @returns the entry in progress, or undefined when nothing matches.
   */
  begin(sessionId: string, id?: string): RequestStatusEntry | undefined {
    const entry = this.pending(sessionId, id)
    if (!entry) return undefined
    return entry.state === 'running' ? entry : this.replace(sessionId, { ...entry, state: 'running' })
  }

  /**
   * End a request (#52): the head, or the entry a granted `uml_ir` names.
   * @param sessionId the session.
   * @param end how it ended.
   * @param id the entry to end exactly, if any; otherwise the head.
   * @returns the entry as it now reads, or undefined when nothing is unfinished.
   */
  end(sessionId: string, end: RequestEnd, id?: string): RequestStatusEntry | undefined {
    const entry = this.pending(sessionId, id)
    if (!entry) return undefined
    return this.replace(sessionId, end.state === 'done'
      ? { ...entry, state: 'done' }
      : { ...entry, state: 'failed', reason: end.reason, exitCode: end.exitCode ?? null })
  }

  /**
   * Note that the message of the request admitted under `promptId` entered the
   * current turn (#52): if that turn ends without completing it, it fails.
   * @param sessionId the session.
   * @param promptId the prompt correlation id the durable message echoes.
   */
  claim(sessionId: string, promptId: string): void {
    const entry = this.prompts.get(sessionId)?.get(promptId)
    if (entry === undefined) return
    const own = this.list(sessionId).find(item => item.id === entry && unfinished(item))
    if (!own) return
    let claimed = this.claimed.get(sessionId)
    if (!claimed) this.claimed.set(sessionId, claimed = new Set())
    claimed.add(own.id)
  }

  /**
   * A turn of this session started: what the previous one claimed is done.
   * @param sessionId the session.
   */
  turnStarted(sessionId: string): void {
    this.claimed.delete(sessionId)
  }

  /**
   * A turn of this session ended (#52). The request it consumed, or the one it
   * left in progress, did not finish: the companion ended the turn without
   * completing the display, so the request fails with that reason.
   * @param sessionId the session.
   */
  turnEnded(sessionId: string): void {
    const claimed = this.claimed.get(sessionId)
    this.claimed.delete(sessionId)
    for (const id of claimed ?? []) {
      this.end(sessionId, { state: 'failed', reason: TURN_ENDED_REASON }, id)
    }
    // A companion that ran tools without the message reaching us (an older
    // DSH, a resumed session) still left this request unfinished.
    const head = this.head(sessionId)
    if (head?.state === 'running') this.end(sessionId, { state: 'failed', reason: TURN_ENDED_REASON }, head.id)
  }

  /**
   * Listen to one session's list.
   * @param sessionId the session.
   * @param listener called with the whole list after each change.
   * @returns the unsubscribe.
   */
  subscribe(sessionId: string, listener: Listener): () => void {
    let set = this.listeners.get(sessionId)
    if (!set) this.listeners.set(sessionId, set = new Set())
    set.add(listener)
    return () => {
      set.delete(listener)
      if (set.size === 0 && this.listeners.get(sessionId) === set) this.listeners.delete(sessionId)
    }
  }

  /** The entry a transition acts on: the one `id` names exactly, else the head. */
  private pending(sessionId: string, id?: string): RequestStatusEntry | undefined {
    return id === undefined
      ? this.head(sessionId)
      : this.list(sessionId).find(item => item.id === id && unfinished(item))
  }

  /** Replace one entry in place, telling the session's listeners. */
  private replace(sessionId: string, entry: RequestStatusEntry): RequestStatusEntry {
    const next = this.list(sessionId).map(item => item.id === entry.id ? entry : item)
    this.lists.set(sessionId, next)
    for (const listener of this.listeners.get(sessionId) ?? []) listener(next)
    return entry
  }
}
