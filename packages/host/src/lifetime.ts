import type { Context } from '@deepseek-ai/cordis'
import type { HostServices } from './dsh.js'
import type { Config } from './binary.js'
import { ResidentPool, ResidentSessions } from './resident.js'
import type { SessionStates } from './state.js'

// The root context outlives a plugin replacement; no process-global registry.
// Symbol.for also survives loading a new copy of this module after an update.
const POOLS = Symbol.for('dsh-uml-viewer.resident-pools.v1')
type Root = Context & { [POOLS]?: Map<string, ResidentPool> }

/**
 * Open panels retain their children across a plugin replacement. The new
 * activation joins the root's pool: actions for an existing session reach
 * its old resident, and a fresh session starts with the new pinned argv.
 * Last-panel close still stops its resident; root shutdown is a second
 * owner that stops every remaining child, even if no panel close arrives.
 */
export function residentLifetime(ctx: Context, home: string | undefined, services: HostServices,
  config: Config, states: SessionStates): ResidentSessions {
  const root = ctx.root as Root
  let pools = root[POOLS]
  if (!pools) {
    pools = new Map()
    Object.defineProperty(root, POOLS, { value: pools, configurable: true })
    root.effect(() => () => { delete root[POOLS] }, 'dsh-uml-viewer: resident pool registry')
  }
  const key = home ?? 'checkout'
  let pool = pools.get(key)
  const fresh = !pool
  if (!pool) {
    pool = new ResidentPool()
    pools.set(key, pool)
  }
  const sessions = new ResidentSessions(services, config, states, pool)
  if (fresh) root.effect(() => () => sessions.dispose(), 'dsh-uml-viewer: resident root lifetime')
  return sessions
}
