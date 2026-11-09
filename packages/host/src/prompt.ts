import type { Agent } from './dsh.js'
// The one source of the rules (#18): esbuild embeds the document as text.
import rulesDocument from '../../../docs/companion-rules.md'

/** Placement after the deployment persona and before repository-specific sections. */
const SECTION_ORDER = 500

/**
 * The companion rules, registered only in sessions whose panel stream is open
 * (SPEC: 只有面板打开的会话才注册 companion 规则提示词段落). The text is
 * `docs/companion-rules.md` verbatim, without its trailing newline.
 */
export const COMPANION_RULES = rulesDocument.trimEnd()

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
