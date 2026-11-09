import { spawn } from 'node:child_process'
import { existsSync } from 'node:fs'
import { mkdir, mkdtemp, readFile, rm, writeFile } from 'node:fs/promises'
import os from 'node:os'
import path from 'node:path'
import { fileURLToPath } from 'node:url'
import { afterEach, expect, it } from 'vitest'
import type { Agent, HostServices, SpawnSpec } from '../src/dsh.js'
import { ResidentPool, ResidentSessions, WITHDRAWAL_GRACE_MS } from '../src/resident.js'
import { SessionStates } from '../src/state.js'

// #32, D13: a panel stream ending is a carrier loss, not a panel close. The
// resident waits in a bounded grace for the same session to re-attach, and only
// an explicit close, the grace expiring, or root shutdown stops it. These run
// against a real fake resident child with a shortened window.

const goldenFrame = fileURLToPath(new URL('../../../crates/frame/tests/fixtures/frames/root.frame.json', import.meta.url))
const fakeBinary = [process.execPath, fileURLToPath(new URL('./fixtures/uml-viewer.mjs', import.meta.url))]

const roots: string[] = []
afterEach(async () => {
  for (const root of roots.splice(0)) await rm(root, { recursive: true, force: true, maxRetries: 20, retryDelay: 100 })
})

async function waitFor(check: () => boolean, ms = 8_000): Promise<void> {
  const deadline = Date.now() + ms
  while (!check()) {
    if (Date.now() > deadline) throw new Error('condition not reached in time')
    await new Promise(resolve => setTimeout(resolve, 10))
  }
}

function localSubprocess(spec: SpawnSpec) {
  const child = spawn(spec.argv[0]!, spec.argv.slice(1), { cwd: spec.cwd, stdio: ['pipe', 'pipe', 'pipe'] })
  child.stderr.resume()
  return {
    stdin: child.stdin,
    stdout: child.stdout,
    stderr: child.stderr,
    collected: {},
    done: new Promise<{ exitCode: number | null; signal: string | null }>((resolve, reject) => {
      child.on('error', reject)
      child.on('close', (exitCode, signal) => resolve({ exitCode, signal }))
    }),
    terminate: () => child.kill(),
    alive: () => child.exitCode === null && child.signalCode === null,
  }
}

async function setup(graceMs: number) {
  const root = await mkdtemp(path.join(os.tmpdir(), 'uml withdrawal '))
  roots.push(root)
  const projects: Record<string, string> = {}
  for (const id of ['session-a', 'session-b']) {
    const project = path.join(root, id)
    await mkdir(project)
    await writeFile(path.join(project, 'resident.json'), JSON.stringify({ framePath: goldenFrame }))
    projects[id] = project
  }
  const home = path.join(root, 'home')
  const states = new SessionStates(home)
  const pool = new ResidentPool(graceMs)
  const children: ReturnType<typeof localSubprocess>[] = []
  const services = {
    sandboxPolicy: { resolve: ({ session }: { session: { id: string } }) => ({ mode: 'danger-full-access', workspaceRoot: projects[session.id]! }) },
    workingDirectory: { get: (session: { id: string }) => projects[session.id]!, ensure: async (agent: Agent) => projects[agent.session.id]! },
    subprocess: { spawn(spec: SpawnSpec) { const child = localSubprocess(spec); children.push(child); return child } },
  } as unknown as HostServices
  const sessions = new ResidentSessions(services, { binary: fakeBinary }, states, pool)
  const agent = (id: string) => ({ id, session: { id } }) as Agent
  return {
    pool, sessions, states, children, agent, projects, home,
    exitFile: (id: string) => path.join(projects[id]!, 'resident-exit.json'),
    alive: () => children.filter(child => child.alive()).length,
  }
}

it('defaults to a bounded withdrawal window of at most a minute', () => {
  expect(new ResidentPool().graceMs).toBe(WITHDRAWAL_GRACE_MS)
  expect(WITHDRAWAL_GRACE_MS).toBeGreaterThan(0)
  expect(WITHDRAWAL_GRACE_MS).toBeLessThanOrEqual(60_000)
})

it('holds a withdrawn resident for its grace and re-attaches instead of double-counting', async () => {
  const h = await setup(150)
  const signal = () => new AbortController().signal
  const opened = await h.sessions.open(h.agent('session-a'), signal())
  expect(opened.refs).toBe(1)

  await h.sessions.release(opened, 'withdrawn')
  expect(opened.refs).toBe(0)
  expect(h.pool.graces.size).toBe(1)
  expect(h.alive()).toBe(1)

  // The same session re-attaches inside the window: one holder, one timer gone.
  const again = await h.sessions.open(h.agent('session-a'), signal())
  expect(again).toBe(opened)
  expect(opened.refs).toBe(1)
  expect(h.pool.graces.size).toBe(0)
  await new Promise(resolve => setTimeout(resolve, 250))
  expect(existsSync(h.exitFile('session-a'))).toBe(false)
  expect(h.alive()).toBe(1)

  // A second withdrawal arms a fresh window; the expiry saves the view, then stops.
  await h.sessions.release(again, 'withdrawn')
  expect(h.pool.graces.size).toBe(1)
  await waitFor(() => existsSync(h.exitFile('session-a')))
  expect(JSON.parse(await readFile(h.exitFile('session-a'), 'utf8')).reason).toBe('quit')
  await waitFor(() => h.alive() === 0)
  expect(h.pool.graces.size).toBe(0)
  expect(JSON.parse(await readFile(path.join(h.home, 'sessions', 'session-a.json'), 'utf8')))
    .toMatchObject({ schema: 'dsh-uml-viewer/session-state', version: 1, camera: { x: 4, y: 5, zoom: 0.75 } })
  await h.sessions.dispose()
})

it('stops a withdrawn resident at once on an explicit close', async () => {
  const h = await setup(30_000)
  const opened = await h.sessions.open(h.agent('session-a'), new AbortController().signal)
  await h.sessions.release(opened, 'withdrawn')
  expect(h.pool.graces.size).toBe(1)
  expect(await h.sessions.close(h.agent('session-a'))).toBe(true)
  await waitFor(() => existsSync(h.exitFile('session-a')))
  expect(h.pool.graces.size).toBe(0)
  expect(JSON.parse(await readFile(h.exitFile('session-a'), 'utf8')).reason).toBe('quit')
  // Idempotent: nothing is left to close.
  expect(await h.sessions.close(h.agent('session-a'))).toBe(false)
  await h.sessions.dispose()
})

it('clears every withdrawal timer and stops every child at shutdown', async () => {
  const h = await setup(30_000)
  const signal = new AbortController().signal
  for (const id of ['session-a', 'session-b']) {
    const connection = await h.sessions.open(h.agent(id), signal)
    await h.sessions.release(connection, 'withdrawn')
  }
  expect(h.pool.graces.size).toBe(2)
  await h.sessions.dispose()
  expect(h.pool.graces.size).toBe(0)
  await waitFor(() => h.alive() === 0)
})
