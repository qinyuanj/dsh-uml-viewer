import type { Agent } from './dsh.js'
import rulesDocument from '../../../docs/companion-rules.md'

const SECTION_ORDER = 500

export const COMPANION_RULES = rulesDocument.trimEnd()

export class PanelPrompts {
  private readonly open = new Map<string, { refs: number; dispose: () => void }>()

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

  disposeAll(): void {
    for (const entry of [...this.open.values()]) entry.dispose()
    this.open.clear()
  }
}
