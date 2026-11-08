import type { Agent } from './dsh.js'

/** Placement after the deployment persona and before repository-specific sections. */
const SECTION_ORDER = 500

/**
 * The companion rules, registered only in sessions whose panel stream is open
 * (SPEC: 只有面板打开的会话才注册 companion 规则提示词段落).
 */
export const COMPANION_RULES = `## UML viewer panel (dsh-uml-viewer)

The user has this session's UML viewer panel open. It is bound to this session and shows the
project in the session working directory; the panel actions arrive as ordinary user messages.

- Quality calls are serialized: send one \`uml_crap\` / \`uml_mutate\` / \`uml_dry\` request at a
  time and wait for its result before starting the next one.
- \`uml_ir\` runs only after the latest quality call in this session returned \`validResult\`
  true, or when the request needs no quality refresh (such as \`:omit\` or a proposal request).
- \`uml_display\` runs only after the latest \`uml_ir\` in this session succeeded. If it is
  refused, fix the cause and run \`uml_ir\` again instead of retrying the display.
- \`:refresh-mutate\` is differential on the target files; \`:refresh-mutate-all\` passes
  \`--mutate-all\` for those same files. Pass every target file in one quality call.
- \`:omit\` adds the id to the current proposal's \`:omit\`, or to the policy \`:omit\` on the real
  diagram, then regenerates the IR; display only after the IR succeeds.
- Proposals are named groupings that are not in the source. Preserve them when you rewrite the
  policy, do not invent them, and answer **P** (the panel's namespace-tree action) by returning
  to the namespace tree.
- Edit the policy file in place: change only the keys you mean to, and keep its comments,
  ordering and line layout.
- The discussion context is the diagram under discussion. Stay on it until a later \`:context\`
  request arrives.`

/**
 * One prompt section per session with an open panel stream, ref-counted so a
 * second panel of the same session shares it and the last close removes it.
 */
export class PanelPrompts {
  private readonly open = new Map<string, { refs: number; dispose: () => void }>()

  /** Register the section in the agent's own scope. */
  openPanel(agent: Agent): void {
    const existing = this.open.get(agent.session.id)
    if (existing) {
      existing.refs += 1
      return
    }
    const systemPrompt = agent.ctx?.systemPrompt
    if (!systemPrompt) return
    const dispose = systemPrompt.section({
      name: 'dsh-uml-viewer:companion-rules',
      order: SECTION_ORDER,
      text: COMPANION_RULES,
      interpolate: false,
    })
    this.open.set(agent.session.id, { refs: 1, dispose })
  }

  closePanel(sessionId: string): void {
    const existing = this.open.get(sessionId)
    if (!existing) return
    existing.refs -= 1
    if (existing.refs > 0) return
    this.open.delete(sessionId)
    existing.dispose()
  }

  forget(sessionId: string): void {
    const existing = this.open.get(sessionId)
    if (!existing) return
    this.open.delete(sessionId)
    existing.dispose()
  }

  /** Second owner: plugin unload removes every section it registered. */
  disposeAll(): void {
    for (const entry of [...this.open.values()]) entry.dispose()
    this.open.clear()
  }
}
