import path from 'node:path'

/**
 * Scripted mock-model turns for the installed-upgrade acceptance (#32). Each
 * prompt drives only the public `plugin_manager` tool; the reply reads the
 * tool's real result instead of assuming the scripted call succeeded.
 */
export const UPGRADE_PROMPTS = {
  remove: 'Perform isolated package removal',
  install: 'Install isolated upgrade bundle',
}

/**
 * The manager result inside one tool_result. The pinned tool returns
 * `JSON.stringify(result)`; Messages transports wrap it in text blocks.
 */
export function managementResult(value) {
  if (typeof value === 'string') {
    try { return managementResult(JSON.parse(value)) } catch { return null }
  }
  if (!value || typeof value !== 'object') return null
  if (typeof value.application === 'string') return value
  for (const child of Object.values(value)) {
    const found = managementResult(child)
    if (found) return found
  }
  return null
}

const parts = messages => messages.flatMap(message => Array.isArray(message.content) ? message.content : [])
const isPrompt = (message, text) => message.role === 'user' && Array.isArray(message.content)
  && message.content.some(part => part.type === 'text' && part.text === text)

/** The latest upgrade prompt and the transcript after it, or null. */
function currentPrompt(messages) {
  for (let index = messages.length - 1; index >= 0; index -= 1) {
    for (const [phase, text] of Object.entries(UPGRADE_PROMPTS)) {
      if (isPrompt(messages[index], text)) return { phase, tail: messages.slice(index + 1) }
    }
  }
  return null
}

function resultOf(tail, use) {
  return managementResult(parts(tail).find(part => part.type === 'tool_result' && part.tool_use_id === use.id))
}

/**
 * Removal: list_bundles, then remove_bundle. Install: install_bundle with the
 * absolute main tarball, so PluginManager.installBundle selects the bundle and
 * runs HMR.reconcileProfile itself (an external `dsh plugin add` does not).
 * @param observed Receives `removal` and `install`, the actual tool results.
 * @param options.main Absolute path of the upgrade main tarball.
 */
export function upgradeReply(observed, { main }) {
  // The pinned parser requires an absolute local path; accept either OS spelling so fixtures run anywhere.
  if (typeof main !== 'string' || !(path.win32.isAbsolute(main) || path.posix.isAbsolute(main))) throw new Error('The upgrade main tarball must be an absolute path')
  return request => {
    if (!request.tools?.length) return null
    const prompt = currentPrompt(request.messages ?? [])
    if (!prompt) return null
    if (!request.tools.some(tool => tool.name === 'plugin_manager')) return { text: 'Missing public plugin_manager tool' }
    const uses = parts(prompt.tail).filter(part => part.type === 'tool_use' && part.name === 'plugin_manager')
    if (prompt.phase === 'remove') {
      if (!uses.length) return { tool: { name: 'plugin_manager', input: { action: 'list_bundles', limit: 100 } } }
      const remove = uses.find(part => part.input?.action === 'remove_bundle')
      if (!remove) return { tool: { name: 'plugin_manager', input: { action: 'remove_bundle', target: 'dsh-uml-viewer' } } }
      observed.removal = resultOf(prompt.tail, remove)
      return { text: observed.removal?.application === 'applied' ? 'Isolated removal applied' : 'Isolated removal did not apply' }
    }
    const install = uses.find(part => part.input?.action === 'install_bundle')
    if (!install) return { tool: { name: 'plugin_manager', input: { action: 'install_bundle', target: main } } }
    observed.install = resultOf(prompt.tail, install)
    return { text: observed.install?.application === 'applied' ? 'Isolated install applied' : 'Isolated install did not apply' }
  }
}
