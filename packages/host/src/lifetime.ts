import type { Context } from '@deepseek-ai/cordis'
import type { Config } from './binary.js'
import type { HostServices } from './dsh.js'
import { PanelPrompts } from './prompt.js'
import { disposePool, ResidentPool, ResidentSessions } from './resident.js'
import type { SessionStates } from './state.js'

// The root context outlives a plugin replacement; no process-global registry.
// Symbol.for also survives loading a new copy of this module after an update.
// v2 includes owned holders, serialized transitions and separate panel/borrow refs.
const STATE = Symbol.for('dsh-uml-viewer.panel-state.v2')

interface Shared {
  pool: ResidentPool
  prompts: PanelPrompts
}
type Root = Context & { [STATE]?: Map<string, Shared> }

/** The per-plugin-home panel state this activation joins. */
export interface PanelLifetime {
  readonly sessions: ResidentSessions
  readonly prompts: PanelPrompts
}

/**
 * Open panels retain their children across a plugin replacement. The new
 * activation joins the root's state: actions for an existing session reach
 * its old resident, and a fresh session starts with the new pinned argv.
 * Last-panel close still stops its resident and drops its prompt section;
 * root shutdown is a second owner that stops every remaining child, even if
 * no panel close arrives.
 */
export function panelLifetime(ctx: Context, home: string | undefined, services: HostServices,
  config: Config, states: SessionStates): PanelLifetime {
  const root = ctx.root as Root
  let shared = root[STATE]
  if (!shared) {
    shared = new Map()
    Object.defineProperty(root, STATE, { value: shared, configurable: true })
    root.effect(() => () => { delete root[STATE] }, 'dsh-uml-viewer: panel state registry')
  }
  const key = home ?? 'checkout'
  let entry = shared.get(key)
  if (!entry) {
    entry = { pool: new ResidentPool(), prompts: new PanelPrompts() }
    shared.set(key, entry)
    const owned = entry
    root.effect(() => async () => {
      shared.delete(key)
      owned.prompts.disposeAll()
      await disposePool(states, owned.pool)
    }, 'dsh-uml-viewer: panel root lifetime')
  }
  return {
    sessions: new ResidentSessions(services, config, states, entry.pool),
    prompts: entry.prompts,
  }
}
