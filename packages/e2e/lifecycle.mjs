import fs from 'node:fs'
import path from 'node:path'

/**
 * The host tab lifecycle of the UML panel (ADR 0015, #43), driven only
 * through the host's own controls and page commands: the right sidebar's
 * collapse and fullscreen buttons, and the focused page's close and refresh
 * shortcuts (Web bindings: Ctrl+Alt+W, Ctrl+Alt+R).
 */

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

/** Run a page command of the focused UML tab (the canvas holds focus). */
async function pageCommand(panel, key) {
  await panel.canvas.focus()
  await panel.page.keyboard.press(`Control+Alt+${key}`)
}

/** The host's tab refresh command. */
export const refreshTab = panel => pageCommand(panel, 'R')

/** The host's tab close command: it runs the tab's close handler, then removes it. */
export const closeTab = panel => pageCommand(panel, 'W')

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
