import { realpath } from 'node:fs/promises'
import type { DisplayResult } from '@dsh-uml-viewer/contracts'
import type { HostServices, ToolRunContext } from './dsh.js'
import type { IrGate } from './gate.js'
import { READ_ONLY_REASON, Refusal, ResidentSessions } from './resident.js'

/** A display record: ADR 0001 fields, no child process, and the diagram it asked the panel for. */
export function displayResult(projectRoot: string, patch: Partial<DisplayResult> = {}): DisplayResult {
  return {
    version: 1, operation: 'display', projectRoot, path: null, exitCode: null, validResult: false,
    crapScope: null, configured: false, tool: 'uml-viewer', status: 'refused', reason: null, ...patch,
  }
}

/**
 * Show the diagram the latest successful `uml_ir` wrote.
 *
 * The gate is checked first (a refusal explains why). The panel is refreshed
 * through the session's resident session: a live one is reused, otherwise a
 * short-lived one is started and stopped again, so the result reports what the
 * resident actually found instead of assuming the panel is up to date.
 */
export async function runDisplay(
  services: HostServices,
  sessions: ResidentSessions,
  gate: IrGate,
  exec: ToolRunContext,
  request: { path?: string },
): Promise<DisplayResult> {
  const agent = exec.agent
  if (!agent?.session) return displayResult('', { reason: 'uml_display requires a live DSH session' })
  let root = services.workingDirectory.get(agent.session)
  const path = request.path ?? null
  // The sandbox decision comes first: in read-only mode no uml_ir can have run,
  // so "run uml_ir" would be the wrong instruction.
  const policy = services.sandboxPolicy.resolve({ session: agent.session })
  if (policy.mode === 'read-only') return displayResult(root, { status: 'refused', path, reason: READ_ONLY_REASON })
  const gated = gate.check(agent.session.id)
  if (!gated.ok) return displayResult(root, { status: 'refused', path, reason: gated.reason })
  try {
    root = await realpath(await services.workingDirectory.ensure(agent, exec.signal))
    const event = await sessions.refresh(agent, exec.signal,
      request.path === undefined ? { action: 'reload' } : { action: 'display', path: request.path })
    if (event.event === 'error') return displayResult(root, { status: 'error', path, reason: event.message })
    if (event.event !== 'frame') {
      return displayResult(root, { status: 'error', path, reason: `uml-viewer resident answered ${event.event} instead of a frame` })
    }
    if (event.frame.waiting) {
      return displayResult(root, { status: 'error', path,
        reason: 'the resident found no diagram in the project; check the policy :out path, run uml_ir again, then uml_display' })
    }
    return displayResult(root, { status: 'ok', validResult: true, path })
  } catch (error) {
    const status = error instanceof Refusal ? 'refused' : 'error'
    return displayResult(root, { status, path, reason: error instanceof Error ? error.message : String(error) })
  }
}
