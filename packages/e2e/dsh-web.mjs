import { spawn, spawnSync } from 'node:child_process'
import { randomUUID } from 'node:crypto'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

export const ROOT = path.resolve(import.meta.dirname, '../..')
export const SCRATCH = path.resolve(process.env.UML_VIEWER_E2E_SCRATCH ?? path.join(ROOT, '.scratch', 'e2e'))
export const BROWSERS = path.resolve(process.env.PLAYWRIGHT_BROWSERS_PATH ?? path.join(SCRATCH, 'browsers'))
// The installed Desktop app, discovered from the environment rather than a
// hard-coded account path; CI and other hosts pass DSH_CLI instead.
const DESKTOP = path.join(process.env.LOCALAPPDATA ?? path.join(os.homedir(), 'AppData', 'Local'),
  'Programs', 'DeepSeek Harness')

function cli() {
  if (process.env.DSH_CLI) return { file: process.env.DSH_CLI, args: [], shell: process.platform === 'win32' }
  if (process.platform === 'win32' && fs.existsSync(path.join(DESKTOP, 'resources/runtime/cli/bin/dsh.cmd'))) {
    return {
      file: path.join(DESKTOP, 'DeepSeek Harness.exe'),
      args: ['--expose-internals', path.join(DESKTOP, 'resources/app.asar/dsh/node_modules/@deepseek-ai/dsh-desktop-host/lib/cli.js')],
      shell: false,
    }
  }
  return { file: 'dsh', args: [], shell: false }
}

function run(command, args, env) {
  const result = spawnSync(command.file, [...command.args, ...args], {
    env, shell: command.shell, stdio: 'inherit',
  })
  if (result.error) throw result.error
  if (result.status !== 0) throw new Error(`${command.file} ${args.join(' ')} exited ${result.status}`)
}

function provisionHome(home) {
  if (process.platform !== 'win32') return
  const account = process.env.USERDOMAIN ? `${process.env.USERDOMAIN}\\${process.env.USERNAME}` : process.env.USERNAME
  if (!account) throw new Error('USERNAME is required to provision the isolated DSH home')
  const result = spawnSync('icacls', [home, '/grant:r', `${account}:(OI)(CI)F`], { encoding: 'utf8' })
  if (result.status !== 0) throw new Error(`could not provision ${home}: ${result.stderr}`)
}

function writeWorkspaces(home, projects) {
  const directory = path.join(home, 'storages')
  fs.mkdirSync(directory, { recursive: true })
  const now = new Date().toISOString()
  const ids = Object.fromEntries(Object.keys(projects).map(name => [name, randomUUID()]))
  const workspaces = Object.fromEntries(Object.entries(projects).map(([name, project]) => [ids[name], {
    path: project, title: name, sessionIds: [], createdAt: now, updatedAt: now,
  }]))
  fs.writeFileSync(path.join(directory, 'workspace.json'), JSON.stringify({
    unit: { name: 'workspace', version: 2 },
    global: { initialized: true, workspaceIds: Object.values(ids), archivedSessionIds: [], pinnedSessionIds: [], defaultWorkspaceId: ids.shop },
    tables: { workspaces },
  }, null, 2))
}

export function viewerBinary() {
  const configured = process.env.UML_VIEWER_BIN
  if (configured) return path.resolve(configured)
  const name = process.platform === 'win32' ? 'uml-viewer.exe' : 'uml-viewer'
  for (const profile of ['debug', 'release']) {
    const candidate = path.join(ROOT, 'target', profile, name)
    if (fs.existsSync(candidate)) return candidate
  }
  throw new Error('Build the real binary with `cargo build --locked -p uml-viewer`, or set UML_VIEWER_BIN to its absolute path.')
}

/** Install the checkout and boot a token-authenticated host on an OS-assigned port. */
export async function startDshWeb({ home, projects }) {
  fs.mkdirSync(home, { recursive: true })
  provisionHome(home)
  writeWorkspaces(home, projects)
  const env = {
    ...process.env,
    DSH_HOME: home,
    DSH_E2E_MOCK_KEY: 'isolated-mock-key',
    ELECTRON_RUN_AS_NODE: '1',
    PLAYWRIGHT_BROWSERS_PATH: BROWSERS,
    npm_config_store_dir: path.join(SCRATCH, 'pnpm-store'),
    npm_config_cache: path.join(SCRATCH, 'npm-cache'),
    npm_config_state_dir: path.join(SCRATCH, 'pnpm-state'),
  }
  const command = cli()
  run(command, ['plugin', '--profile', 'web', 'add', ROOT], env)
  return { command, env }
}

export async function bootDshWeb({ command, env, home, mockUrl }) {
  const patch = path.join(home, 'lane.patch.yml')
  fs.writeFileSync(patch, [
    '- id: dsh-uml-viewer',
    '  config:',
    `    binary: ${JSON.stringify(viewerBinary())}`,
    `    home: ${JSON.stringify(path.join(home, 'plugin-state'))}`,
    '- id: llm-deepseek',
    '  config:',
    `    baseURL: ${mockUrl}`,
    '    apiKeyEnv: DSH_E2E_MOCK_KEY',
    '',
  ].join('\n'))
  const lifeline = path.join(import.meta.dirname, 'lifeline.cjs').replaceAll('\\', '/')
  const childEnv = {
    ...env,
    UML_VIEWER_E2E_LIFELINE: '1',
    NODE_OPTIONS: [env.NODE_OPTIONS, `--require "${lifeline}"`].filter(Boolean).join(' '),
  }
  const child = spawn(command.file, [...command.args, '--profile', 'web', '--patch', patch, '--port', '0', '--no-open'], {
    env: childEnv, shell: command.shell, stdio: ['pipe', 'pipe', 'pipe'],
  })
  let output = ''
  let stopRequested = false
  const stop = () => {
    stopRequested = true
    if (child.exitCode !== null || child.signalCode !== null) return
    child.stdin.end()
    if (process.platform === 'win32') spawnSync('taskkill', ['/pid', String(child.pid), '/T', '/F'], { stdio: 'ignore' })
    else child.kill('SIGTERM')
  }
  const exited = new Promise((resolve, reject) => {
    child.once('error', reject)
    child.once('exit', code => stopRequested ? resolve(code) : reject(new Error(`isolated DSH exited ${code}:\n${output}`)))
  })
  exited.catch(() => {})
  const url = await new Promise((resolve, reject) => {
    const timer = setTimeout(() => {
      stop()
      reject(new Error(`DSH printed no URL within 120s:\n${output}`))
    }, 120_000)
    const watch = chunk => {
      output += chunk.toString()
      const match = output.match(/dsh web: (http\S+)/)
      if (match) {
        clearTimeout(timer)
        resolve(match[1])
      }
    }
    child.stdout.on('data', watch)
    child.stderr.on('data', watch)
    child.once('exit', code => {
      clearTimeout(timer)
      reject(new Error(`DSH exited ${code} before its URL:\n${output}`))
    })
  })
  const parsed = new URL(url)
  if (!['127.0.0.1', 'localhost', '[::1]'].includes(parsed.hostname) || parsed.port === '14227') {
    stop()
    throw new Error(`refusing non-isolated or live GUI URL: ${url}`)
  }
  return { url, home, stop, exited, output: () => output }
}

export async function dismissOverlays(page) {
  const locator = page.locator('[role="dialog"]:not([data-shortcut-modal="sidebar"]):visible, [role="presentation"]:visible')
  let quietSince = null
  const deadline = Date.now() + 12_000
  for (let attempt = 0; Date.now() < deadline && attempt < 40; attempt++) {
    const overlay = locator.last()
    if (await overlay.count() === 0) {
      quietSince ??= Date.now()
      if (Date.now() - quietSince >= 1000) return
      await page.waitForTimeout(150)
      continue
    }
    quietSince = null
    const buttons = overlay.locator('button:visible:enabled')
    if (await buttons.count() === 0) {
      await page.waitForTimeout(200)
      continue
    }
    await buttons.first().click({ timeout: 1000 }).catch(() => {})
    await page.waitForTimeout(400)
  }
  throw new Error(`first-run overlays did not close: ${await locator.allInnerTexts()}`)
}

export { fileURLToPath }
