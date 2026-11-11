import fs from 'node:fs'
import path from 'node:path'

/**
 * The host tab lifecycle of the UML panel (ADR 0015, #43), driven only
 * through the host's own controls and page commands: the right sidebar's
 * collapse and fullscreen buttons, the UML tab's close button, and the
 * focused page's refresh command.
 *
 * DSH 0.2.2-rc.1 binds `page.refresh` by default only on Desktop and Windows
 * Web (Ctrl+Alt+R); Linux Web has no default and admits only Ctrl+/,
 * Ctrl+Shift+, and Ctrl+Shift+. there. The lane therefore saves a user
 * override, Ctrl+Shift+., in the page's own `dsh.keybindings.v1` preferences
 * before the page loads, the same document the shortcut reference writes.
 */
const REFRESH = { code: 'Period', modifiers: ['primary', 'shift'] }
const REFRESH_KEYS = 'Control+Shift+Period'

/** Bind the page refresh command on Linux Web before the page loads. */
export async function bindRefresh(page) {
  await page.addInitScript(binding => {
    const key = 'dsh.keybindings.v1'
    let document = { schemaVersion: 1, profiles: {} }
    try { document = JSON.parse(localStorage.getItem(key) ?? 'null') ?? document } catch { /* rewritten below */ }
    document.profiles['web:linux'] = { ...document.profiles['web:linux'], 'page.refresh': binding }
    localStorage.setItem(key, JSON.stringify(document))
  }, REFRESH)
}

/** Resident processes running in `project`, read from the OS (Linux `/proc`). */
export function residents(project) {
  if (process.platform !== 'linux') throw new Error('resident observation reads /proc; it runs on Linux only')
  const root = fs.realpathSync(project)
  const found = []
  for (const name of fs.readdirSync('/proc')) {
    if (!/^\d+$/.test(name)) continue
    try {
      const argv = fs.readFileSync(path.join('/proc', name, 'cmdline'), 'utf8').split('\0').filter(Boolean)
      if (argv.at(-1) !== 'resident' || !argv.some(arg => path.basename(arg).startsWith('uml-viewer'))) continue
      if (fs.realpathSync(path.join('/proc', name, 'cwd')) === root) found.push(Number(name))
    } catch { /* the process exited or is not ours to read */ }
  }
  return found
}

/** Record every panel action the page sends, whatever carrier the Remote uses. */
export function recordActions(page) {
  const sent = []
  const note = text => {
    for (const match of String(text ?? '').matchAll(/"action"\s*:\s*"([a-z-]+)"/g)) sent.push(match[1])
  }
  page.on('websocket', socket => socket.on('framesent', frame => note(frame.payload)))
  page.on('request', request => note(request.postData()))
  return sent
}

/** The host's tab refresh command, run on the focused UML tab (the canvas holds focus). */
export async function refreshTab(panel) {
  await panel.canvas.focus()
  await panel.page.keyboard.press(REFRESH_KEYS)
}

/** The UML tab's own close button: it runs the tab's close handler, then removes it. */
export async function closeTab(panel) {
  const tab = panel.page.locator('[data-dockkit-tab]:visible', { hasText: 'UML' }).first()
  await tab.hover()
  await tab.locator('[data-dockkit-tab-close]').click()
}

/** The right sidebar's own collapse and expand buttons. */
export async function collapseSidebar(panel) {
  await panel.page.getByRole('button', { name: 'Collapse right sidebar', exact: true }).first().click()
}
export async function expandSidebar(panel) {
  await panel.page.locator('[data-sidebar-right-expand]').first().click()
}

/** The right sidebar's fullscreen toggle. */
export async function toggleFullscreen(panel, on) {
  await panel.page.getByRole('button', { name: on ? 'Fullscreen' : 'Exit fullscreen', exact: true }).first().click()
}
