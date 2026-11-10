import type { RequestStatusEntry } from '@dsh-uml-viewer/contracts'

/**
 * The most entries one session keeps. Finished entries are dropped oldest
 * first past it (an unfinished one only when nothing finished is left), so a
 * long session cannot grow the host's memory without bound.
 */
export const REQUEST_STATUS_MAX_ENTRIES = 64

type Listener = (requests: readonly RequestStatusEntry[]) => void

/**
 * Request status (#51, ADR 0015): each session's panel requests as its panels
 * show them, held in process memory only — DSH restarting forgets them.
 *
 * Panel streams subscribe to their own session and receive the whole list on
 * every change; a session's entries never reach another session's listener.
 * `record` adds an entry or replaces the one with the same id, which is how a
 * later state change (#52) or a dismissal (#53) reaches every open panel.
 */
export class RequestStatuses {
  private readonly lists = new Map<string, RequestStatusEntry[]>()
  private readonly listeners = new Map<string, Set<Listener>>()

  /**
   * This session's entries, oldest first.
   * @param sessionId the session.
   */
  list(sessionId: string): readonly RequestStatusEntry[] {
    return this.lists.get(sessionId) ?? []
  }

  /**
   * Add an entry, or replace the session's entry with the same id in place,
   * then tell the session's listeners.
   * @param sessionId the session the request was sent to.
   * @param entry the entry.
   */
  record(sessionId: string, entry: RequestStatusEntry): void {
    const current = this.lists.get(sessionId) ?? []
    const index = current.findIndex(item => item.id === entry.id)
    const next = index < 0 ? [...current, entry] : current.map((item, at) => at === index ? entry : item)
    while (next.length > REQUEST_STATUS_MAX_ENTRIES) {
      const finished = next.findIndex(item => item.state === 'done' || item.state === 'failed')
      next.splice(finished < 0 ? 0 : finished, 1)
    }
    this.lists.set(sessionId, next)
    for (const listener of this.listeners.get(sessionId) ?? []) listener(next)
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
}
