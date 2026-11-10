import type { RequestStatusEntry } from '@dsh-uml-viewer/contracts'
import { expect, it } from 'vitest'
import { REQUEST_STATUS_MAX_ENTRIES, RequestStatuses } from '../src/request-status.js'

const entry = (id: string, state: RequestStatusEntry['state']): RequestStatusEntry =>
  ({ id, op: 'refresh-crap', target: null, state, reason: state === 'failed' ? 'why' : null, exitCode: null })

it('bounds a session’s list, dropping the oldest finished entries before any unfinished one', () => {
  const statuses = new RequestStatuses()
  statuses.record('s', entry('waiting', 'queued'))
  for (let index = 0; index < REQUEST_STATUS_MAX_ENTRIES; index++) {
    statuses.record('s', entry(`f${index}`, index % 2 ? 'failed' : 'done'))
  }
  const list = statuses.list('s')
  expect(list).toHaveLength(REQUEST_STATUS_MAX_ENTRIES)
  expect(list[0]!.id).toBe('waiting')
  expect(list[1]!.id).toBe('f1')
})

it('replaces an entry with the same id in place and tells only that session’s listeners', () => {
  const statuses = new RequestStatuses()
  const seen: string[][] = []
  const other: unknown[] = []
  const stop = statuses.subscribe('s', list => seen.push(list.map(item => `${item.id}:${item.state}`)))
  statuses.subscribe('t', list => other.push(list))
  statuses.record('s', entry('a', 'queued'))
  statuses.record('s', entry('b', 'queued'))
  statuses.record('s', entry('a', 'done'))
  expect(seen.at(-1)).toEqual(['a:done', 'b:queued'])
  expect(other).toEqual([])
  stop()
  statuses.record('s', entry('c', 'queued'))
  expect(seen).toHaveLength(3)
})
