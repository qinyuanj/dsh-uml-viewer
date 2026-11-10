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

// #53: the user dismisses a finished entry; an unfinished one stays, because
// it is still in the session's queue.
it('dismisses a finished entry for every listener of its session, and nothing else', () => {
  const statuses = new RequestStatuses()
  const seen: string[][] = []
  statuses.subscribe('s', list => seen.push(list.map(item => `${item.id}:${item.state}`)))
  statuses.record('s', entry('a', 'failed'))
  statuses.record('s', entry('b', 'queued'))
  statuses.record('s', entry('c', 'done'))
  statuses.record('t', entry('a', 'failed'))
  expect(statuses.dismiss('s', 'a')).toBe(true)
  expect(seen.at(-1)).toEqual(['b:queued', 'c:done'])
  expect(statuses.dismiss('s', 'c')).toBe(true)
  expect(seen.at(-1)).toEqual(['b:queued'])
  const told = seen.length
  // Unfinished, unknown, already gone or another session's: nothing changes.
  expect(statuses.dismiss('s', 'b')).toBe(false)
  expect(statuses.dismiss('s', 'zz')).toBe(false)
  expect(statuses.dismiss('s', 'a')).toBe(false)
  expect(statuses.dismiss('u', 'a')).toBe(false)
  expect(seen).toHaveLength(told)
  expect(statuses.list('s').map(item => item.id)).toEqual(['b'])
  expect(statuses.list('t').map(item => item.id)).toEqual(['a'])
})
