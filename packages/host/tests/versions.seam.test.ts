import { Context } from '@deepseek-ai/cordis'
import { spawn } from 'node:child_process'
import { createHash } from 'node:crypto'
import { existsSync } from 'node:fs'
import { copyFile, mkdir, mkdtemp, readdir, readFile, realpath, rm, stat, utimes, writeFile } from 'node:fs/promises'
import os from 'node:os'
import path from 'node:path'
import { fileURLToPath, pathToFileURL } from 'node:url'
import { afterEach, expect, it, vi } from 'vitest'
import type { PanelEvent, PanelOwner } from '@dsh-uml-viewer/contracts'
// The real client mount, loaded untyped: the host package has no React types.
const CLIENT_ENTRY: string = new URL('../../client/src/index.ts', import.meta.url).href
type ClientContext = { get(name: string): unknown }
import { Element, makeDocument } from '../../client/tests/dom.js'
import type { ViewerService } from '../src/viewer.js'
vi.mock('react', () => ({ createElement: vi.fn() }))
vi.mock('../../client/src/font.js', () => ({ FONT_FAMILY: 'test', loadSarasa: async () => {} }))
import type { Agent, HostServices, SpawnSpec } from '../src/dsh.js'
import * as plugin from '../src/index.js'
import { cleanupVersions } from '../src/versions.js'

// #32: the packaged binary lifecycle across plugin versions sharing one DSH
// home. Each "installed version" is a scratch main package plus its exact
// platform dependency whose payload is a fake resident (a Node script), or on
// Windows optionally a copy of the running Node executable so the version
// directory really holds an exe that is in use.

const goldenFrame = fileURLToPath(new URL('../../../crates/frame/tests/fixtures/frames/root.frame.json', import.meta.url))
const residentFixture = pathToFileURL(fileURLToPath(new URL('./fixtures/resident.mjs', import.meta.url))).href
const target = `${process.platform}-${process.arch}`
const exe = process.platform === 'win32' ? 'uml-viewer.exe' : 'uml-viewer'

const contexts: Context[] = []
const controllers: AbortController[] = []
const roots: string[] = []
afterEach(async () => {
  for (const controller of controllers.splice(0)) controller.abort()
  for (const ctx of contexts.splice(0)) await ctx.fiber.dispose()
  vi.unstubAllEnvs()
  vi.unstubAllGlobals()
  for (const root of roots.splice(0)) await rm(root, { recursive: true, force: true, maxRetries: 20, retryDelay: 100 })
})

async function scratch(label: string): Promise<string> {
  // Node resolves the resident's __filename through symlinked temp parents.
  const root = await realpath(await mkdtemp(path.join(os.tmpdir(), `uml versions ${label} `)))
  roots.push(root)
  return root
}

/** The fake resident payload: records which version and file ran, then speaks the protocol. */
function payloadScript(version: string): string {
  return `require('node:fs').appendFileSync('resident-versions.log', JSON.stringify({ version: ${JSON.stringify(version)}, binary: __filename }) + '\\n')\n`
    + `import(${JSON.stringify(residentFixture)})\n`
}

/** One installed main package `version` with its platform payload. */
async function install(version: string, options: { native?: boolean } = {}): Promise<string> {
  const root = await scratch(`package ${version}`)
  const dependency = `@dsh-uml-viewer/${target}`
  await writeFile(path.join(root, 'package.json'), JSON.stringify({ name: 'dsh-uml-viewer', version,
    optionalDependencies: { [dependency]: version } }))
  const directory = path.join(root, 'node_modules', dependency)
  await mkdir(path.join(directory, 'bin'), { recursive: true })
  await writeFile(path.join(directory, 'package.json'), JSON.stringify({ name: dependency, version }))
  const payload = path.join(directory, 'bin', exe)
  if (options.native) await copyFile(process.execPath, payload)
  else await writeFile(payload, payloadScript(version))
  return root
}

function localSubprocess(spec: SpawnSpec, wrappers: Map<string, string>) {
  const [command, ...rest] = spec.argv
  // A native payload is a copy of node.exe: run the version's wrapper script with it.
  const wrapper = wrappers.get(command!)
  const child = wrapper
    ? spawn(command!, [wrapper, ...rest], { cwd: spec.cwd, stdio: ['pipe', 'pipe', 'pipe'] })
    : spawn(process.execPath, [command!, ...rest], { cwd: spec.cwd, stdio: ['pipe', 'pipe', 'pipe'] })
  const chunks: Buffer[] = []
  child.stderr.on('data', (chunk: Buffer) => chunks.push(chunk))
  const reader = { readFrom: () => ({ text: Buffer.concat(chunks).toString(), lossy: false }) }
  return {
    stdin: child.stdin,
    stdout: child.stdout,
    stderr: child.stderr,
    collected: { stdout: reader, stderr: reader },
    done: new Promise<{ exitCode: number | null; signal: string | null }>((resolve, reject) => {
      child.on('error', reject)
      child.on('close', (exitCode, signal) => resolve({ exitCode, signal }))
    }),
    terminate: () => child.kill(),
    alive: () => child.exitCode === null && child.signalCode === null,
    waitForExit: async () => true,
  }
}

/** A DSH host process: its own Cordis context and sessions, sharing the DSH home. */
async function host(dshHome: string, packageRoot: string, wrappers = new Map<string, string>(),
  existing?: { ctx: Context; projects: Record<string, string>; children: ReturnType<typeof localSubprocess>[] }) {
  vi.stubEnv('UML_VIEWER_BIN', undefined)
  const projects: Record<string, string> = existing?.projects ?? {}
  const children = existing?.children ?? [] as ReturnType<typeof localSubprocess>[]
  const agents: Record<string, Agent> = {}
  const services: HostServices = {
    tools: { register: () => () => {} },
    sandboxPolicy: { resolve: ({ session }) => ({ mode: 'danger-full-access', workspaceRoot: projects[session.id]! }) },
    workingDirectory: { get: session => projects[session.id]!, ensure: async agent => projects[agent.session.id]! },
    subprocess: { spawn: spec => {
      const child = localSubprocess(spec, wrappers)
      children.push(child)
      return child
    } },
    typert: { register: () => async () => {} },
    dshHomePath: (...segments: string[]) => path.join(dshHome, ...segments),
  }
  const ctx = existing?.ctx ?? new Context()
  if (!existing) contexts.push(ctx)
  for (const [key, value] of Object.entries(services)) {
    if (!existing || !ctx.get(key)) ctx.provide(key, value)
  }
  const fiber = ctx.plugin({ name: 'uml-viewer-version-host',
    apply: (scope: Context) => plugin.applyWithPlatform(scope, {}, { platform: process.platform, arch: process.arch, root: packageRoot }) })
  await fiber
  const preparePanel = async (session: string) => {
    const project = projects[session] ?? await scratch(`project ${session}`)
    projects[session] = project
    agents[session] = { id: session, session: { id: session } }
    if (!existsSync(path.join(project, 'resident.json'))) {
      await writeFile(path.join(project, 'resident.json'), JSON.stringify({ framePath: goldenFrame }))
    }
    return { project, agent: agents[session]! }
  }
  const panel = async (session: string, owner?: PanelOwner) => {
    const { project } = await preparePanel(session)
    const controller = new AbortController()
    controllers.push(controller)
    const viewer = ctx.get('umlViewer') as unknown as {
      watch(agent: Agent, signal: AbortSignal): AsyncIterable<PanelEvent>
      action(agent: Agent, action: unknown): Promise<{ accepted: boolean; reason: string | null }>
      close(agent: Agent): Promise<{ closed: boolean }>
      watchOwned(agent: Agent, owner: PanelOwner, language: string | null, signal: AbortSignal): AsyncIterable<PanelEvent>
      closeOwned(agent: Agent, owner: PanelOwner): Promise<{ closed: boolean }>
    }
    const iterator = (owner ? viewer.watchOwned(agents[session]!, owner, null, controller.signal) : viewer.watch(agents[session]!, controller.signal))[Symbol.asyncIterator]()
    const ready = (await iterator.next()).value as PanelEvent
    expect(ready).toMatchObject({ kind: 'ready' })
    return {
      project,
      action: (value: unknown) => viewer.action(agents[session]!, value),
      /** Every resident start in this project: which version ran, from which file. */
      runs: async () => (await readFile(path.join(project, 'resident-versions.log'), 'utf8'))
        .trim().split('\n').map(line => JSON.parse(line) as { version: string; binary: string }),
      /** A carrier loss, as the gateway leaves a watch stream: abort and return. */
      withdraw: async () => {
        controller.abort()
        await iterator.return?.(undefined)
      },
      /** A user close: the explicit Remote call, then the stream's own teardown. */
      close: async () => {
        if (owner) await viewer.closeOwned(agents[session]!, owner)
        else await viewer.close(agents[session]!)
        controller.abort()
        await iterator.return?.(undefined)
      },
    }
  }
  return { ctx, projects, children, fiber, panel, preparePanel, unload: () => fiber.dispose(), dispose: () => ctx.fiber.dispose() }
}

type CloseHandler = (sessionId: string, tab: { id: string; kind: string }) => void
type TabCommands = { refresh?: () => void }

/**
 * The real client plugin applied to a client context whose right sidebar
 * records what the plugin registers: the tab-kind close handler and the
 * page commands each mounted tab binds.
 */
async function clientPlugin(h: Awaited<ReturnType<typeof host>>, session: string) {
  const { agent } = await h.preparePanel(session)
  const viewer = h.ctx.get('umlViewer') as unknown as ViewerService
  const api = {
    watchOwned(_sessionId: string, owner: PanelOwner, language: string) {
      const abort = new AbortController()
      controllers.push(abort)
      const stream = viewer.watchOwned(agent, owner, language, abort.signal)
      return { [Symbol.asyncIterator]: () => stream[Symbol.asyncIterator](), end() { abort.abort() }, dispose() { abort.abort() } }
    },
    closeOwned: (_sessionId: string, owner: PanelOwner) => viewer.closeOwned(agent, owner),
    action: (_sessionId: string, action: any) => viewer.action(agent, action),
  }
  const closeHandlers = new Map<string, CloseHandler>()
  const ctx: Record<string, unknown> = {
    slots: { register: () => () => {}, inject: (_name: string, callback: () => unknown) => callback() },
    // The stylesheet effect needs a real document; this seam is the lifecycle.
    effect: () => {},
    inject: (_names: readonly string[], callback: (value: unknown) => unknown) => callback(ctx),
    sidebarRightTabs: { register: () => () => {} },
    sidebarRight: {
      registerCloseHandler(kind: string, handler: CloseHandler) {
        closeHandlers.set(kind, handler)
        return () => { closeHandlers.delete(kind) }
      },
    },
    get: (name: string) => name === 'remote' ? { $mount: async () => {} } : api,
  }
  const client = await import(CLIENT_ENTRY) as { apply(ctx: unknown): void }
  client.apply(ctx)
  return {
    ctx: ctx as unknown as ClientContext,
    /** What the host does when the user closes the UML tab or a replacement removes it. */
    closeTab: (tabId: string) => closeHandlers.get('dsh-uml-viewer')!(session, { id: tabId, kind: 'dsh-uml-viewer' }),
  }
}

/** Real mount/controller → typed API bridge → real ViewerService/root pool. */
async function mountedPanel(h: Awaited<ReturnType<typeof host>>, session: string, doc: ReturnType<typeof makeDocument>,
  tab?: { ctx: ClientContext; tabId: string }) {
  const { project, agent } = await h.preparePanel(session)
  // The real overlay controller asks for the card; with none open the fixture answers null.
  await writeFile(path.join(project, 'resident.json'), JSON.stringify({ framePath: goldenFrame }))
  const viewer = h.ctx.get('umlViewer') as unknown as ViewerService
  const api = {
    watchOwned(_sessionId: string, owner: PanelOwner, language?: string) {
      const abort = new AbortController()
      controllers.push(abort)
      const stream = viewer.watchOwned(agent, owner, language ?? null, abort.signal)
      return { [Symbol.asyncIterator]: () => stream[Symbol.asyncIterator](), end() { abort.abort() }, dispose() { abort.abort() } }
    },
    closeOwned: (_sessionId: string, owner: PanelOwner) => viewer.closeOwned(agent, owner),
    action: (_sessionId: string, action: any) => viewer.action(agent, action),
  }
  const ctx = tab?.ctx ?? { get: (name: string) => name === 'remote' ? { $mount: async () => {} } : api } as unknown as ClientContext
  vi.stubGlobal('document', doc)
  const element = new Element('host', doc)
  const { mountPanel } = await import(CLIENT_ENTRY) as { mountPanel(host: unknown, options: Record<string, unknown>): Promise<() => void> }
  const commands: TabCommands[] = []
  const dispose = await mountPanel(element, { sessionId: session, ctx, ...(tab ? {
    tabId: tab.tabId,
    bindCommands: (bound: TabCommands) => {
      commands.push(bound)
      return () => { commands.splice(commands.indexOf(bound), 1) }
    },
  } : {}) })
  const summary = () => element.descendants().find(el => el.attributes['aria-label'] === 'UML panel summary')
  await vi.waitFor(() => expect(summary()?.dataset.kind).toBe('frame'))
  return { element, dispose, project, commands, summary,
    actions: async () => (await readFile(path.join(project, 'resident-actions.log'), 'utf8')).trim().split('\n').map(line => JSON.parse(line) as { action: string }),
    runs: async () => (await readFile(path.join(project, 'resident-versions.log'), 'utf8')).trim().split('\n').map(line => JSON.parse(line) as { version: string; binary: string }) }
}

const sha256 = async (file: string) => createHash('sha256').update(await readFile(file)).digest('hex')

it('actual mount disposal and a new controller retain the resident and lease, then closing the UML tab and reopening it chooses current', async () => {
  const dshHome = await scratch('actual mount home')
  const home = path.join(dshHome, 'dsh-uml-viewer')
  const doc = makeDocument()
  const old = await host(dshHome, await install('1.0.0'))
  const first = await mountedPanel(old, 'one-session', doc)
  const leases = await readdir(path.join(home, 'leases', '1.0.0'))
  expect(leases).toHaveLength(1)
  await old.unload()
  first.dispose()
  await vi.waitFor(() => expect(old.children.filter(child => child.alive())).toHaveLength(1))
  const updated = await host(dshHome, await install('1.1.0'), new Map(), old)
  const client = await clientPlugin(updated, 'one-session')
  const second = await mountedPanel(updated, 'one-session', doc, { ctx: client.ctx, tabId: 'uml-tab' })
  expect(await second.runs()).toEqual([{ version: '1.0.0', binary: path.join(home, '1.0.0', exe) }])
  expect(await readdir(path.join(home, 'leases', '1.0.0'))).toEqual(leases)
  // The user closes the UML tab: the host runs the close handler, then
  // removes the tab and its body.
  client.closeTab('uml-tab')
  second.dispose()
  await vi.waitFor(() => expect(old.children.map(child => child.alive())).toEqual([false]))
  await vi.waitFor(() => expect(existsSync(path.join(home, 'leases', '1.0.0'))).toBe(false))
  // Reopening the UML tab is a fresh mount, and a fresh choice of version.
  const reopened = await mountedPanel(updated, 'one-session', doc, { ctx: client.ctx, tabId: 'uml-tab-2' })
  await vi.waitFor(() => expect(updated.children.filter(child => child.alive())).toHaveLength(1))
  expect((await reopened.runs()).at(-1)).toEqual({ version: '1.1.0', binary: path.join(home, '1.1.0', exe) })
  reopened.dispose()
  await updated.dispose()
  expect(updated.children.map(child => child.alive())).toEqual([false, false])
})

it('a replacement that removes the UML tab runs the close handler and stops the resident', async () => {
  const dshHome = await scratch('replace tab home')
  const h = await host(dshHome, await install('1.0.0'))
  const client = await clientPlugin(h, 'one-session')
  const panel = await mountedPanel(h, 'one-session', makeDocument(), { ctx: client.ctx, tabId: 'uml-tab' })
  expect(h.children.map(child => child.alive())).toEqual([true])
  // `replaceTab` lands another page in this tab's place: the host runs the
  // close handler synchronously while the body is still mounted.
  client.closeTab('uml-tab')
  await vi.waitFor(() => expect(h.children.map(child => child.alive())).toEqual([false]))
  panel.dispose()
  expect((await panel.actions()).map(entry => entry.action)).toContain('save')
})

it('closing a UML tab whose body already unmounted still stops the withdrawn resident at once', async () => {
  const dshHome = await scratch('withdrawn tab home')
  const h = await host(dshHome, await install('1.0.0'))
  const client = await clientPlugin(h, 'one-session')
  const panel = await mountedPanel(h, 'one-session', makeDocument(), { ctx: client.ctx, tabId: 'uml-tab' })
  // Collapsing or a presentation change withdraws: the resident waits out its grace.
  panel.dispose()
  await new Promise(resolve => setTimeout(resolve, 100))
  expect(h.children.map(child => child.alive())).toEqual([true])
  client.closeTab('uml-tab')
  await vi.waitFor(() => expect(h.children.map(child => child.alive())).toEqual([false]))
})

it('closing one UML tab leaves another tab of the session alone', async () => {
  const dshHome = await scratch('other tab home')
  const h = await host(dshHome, await install('1.0.0'))
  const client = await clientPlugin(h, 'one-session')
  const doc = makeDocument()
  const kept = await mountedPanel(h, 'one-session', doc, { ctx: client.ctx, tabId: 'kept' })
  const closed = await mountedPanel(h, 'one-session', doc, { ctx: client.ctx, tabId: 'closed' })
  client.closeTab('closed')
  closed.dispose()
  await vi.waitFor(() => expect(closed.summary()).toBeUndefined())
  expect(h.children.map(child => child.alive())).toEqual([true])
  expect(kept.summary()?.dataset.kind).toBe('frame')
  client.closeTab('kept')
  await vi.waitFor(() => expect(h.children.map(child => child.alive())).toEqual([false]))
  kept.dispose()
})

it('the host tab refresh command reloads the diagram through the resident', async () => {
  const dshHome = await scratch('refresh home')
  const h = await host(dshHome, await install('1.0.0'))
  const client = await clientPlugin(h, 'one-session')
  const panel = await mountedPanel(h, 'one-session', makeDocument(), { ctx: client.ctx, tabId: 'uml-tab' })
  expect(panel.commands).toHaveLength(1)
  panel.commands[0]!.refresh!()
  await vi.waitFor(async () => expect((await panel.actions()).map(entry => entry.action)).toContain('reload'))
  panel.dispose()
  // The binding ends with the body.
  expect(panel.commands).toEqual([])
  client.closeTab('uml-tab')
})

it('keeps the leased child when an already borrowed action finishes after the last owned withdrawal', async () => {
  const dshHome = await scratch('inflight withdrawal home')
  const home = path.join(dshHome, 'dsh-uml-viewer')
  const h = await host(dshHome, await install('1.0.0'))
  const first = await h.panel('one-session', { pageId: 'page-a', holderId: 'first' })
  const leases = await readdir(path.join(home, 'leases', '1.0.0'))
  const action = first.action({ action: 'search', query: 'core' })
  await first.withdraw()
  expect(await action).toMatchObject({ accepted: true })
  expect(h.children.map(child => child.alive())).toEqual([true])
  expect(await readdir(path.join(home, 'leases', '1.0.0'))).toEqual(leases)
  const again = await h.panel('one-session', { pageId: 'page-a', holderId: 'again' })
  expect(await again.runs()).toHaveLength(1)
  expect(await readdir(path.join(home, 'leases', '1.0.0'))).toEqual(leases)
  await again.close()
  expect(h.children.map(child => child.alive())).toEqual([false])
  await h.unload()
  await h.dispose()
  const remaining = path.join(home, 'leases', '1.0.0')
  expect(existsSync(remaining) ? await readdir(remaining) : []).toEqual([])
})

it('last Close then immediate Open selects the current version despite a previously borrowed action', async () => {
  const dshHome = await scratch('inflight close home')
  const old = await host(dshHome, await install('1.0.0'))
  const first = await old.panel('one-session', { pageId: 'page-a', holderId: 'first' })
  await old.unload()
  const updated = await host(dshHome, await install('1.1.0'), new Map(), old)
  const action = first.action({ action: 'search', query: 'core' })
  await first.close()
  const again = await updated.panel('one-session', { pageId: 'page-a', holderId: 'again' })
  await action
  expect((await again.runs()).map(run => run.version)).toEqual(['1.0.0', '1.1.0'])
  expect(updated.children.map(child => child.alive())).toEqual([false, true])
  await again.close()
})

it('a distinct withdrawn document is fresh, while a second live page still shares the resident', async () => {
  const dshHome = await scratch('owned document home')
  const home = path.join(dshHome, 'dsh-uml-viewer')
  const old = await host(dshHome, await install('1.0.0'))
  const first = await old.panel('one-session', { pageId: 'page-a', holderId: 'first' })
  await old.unload()
  const updated = await host(dshHome, await install('1.1.0'), new Map(), old)
  const overlapping = await updated.panel('one-session', { pageId: 'page-b', holderId: 'second' })
  expect(await overlapping.runs()).toHaveLength(1)
  expect(updated.children.filter(child => child.alive())).toHaveLength(1)
  await first.withdraw()
  await overlapping.withdraw()
  const fresh = await mountedPanel(updated, 'one-session', makeDocument())
  expect(await fresh.runs()).toEqual([
    { version: '1.0.0', binary: path.join(home, '1.0.0', exe) },
    { version: '1.1.0', binary: path.join(home, '1.1.0', exe) },
  ])
  fresh.dispose()
})

it('pins the platform payload under <DSH home>/dsh-uml-viewer/<version>/ and records it as current', async () => {
  const dshHome = await scratch('home')
  const root = await install('1.0.0')
  await host(dshHome, root)
  const home = path.join(dshHome, 'dsh-uml-viewer')
  const pinned = path.join(home, '1.0.0', exe)
  const source = path.join(root, 'node_modules', '@dsh-uml-viewer', target, 'bin', exe)
  expect(await readFile(pinned, 'utf8')).toBe(payloadScript('1.0.0'))
  expect(JSON.parse(await readFile(path.join(home, 'current.json'), 'utf8'))).toEqual({
    version: '1.0.0', size: (await stat(source)).size, sha256: await sha256(source),
  })
  // Only the published version directory, never a staging copy, remains.
  expect((await readdir(home)).filter(name => name.startsWith('.'))).toEqual([])
})

it('keeps an old panel on vN after updating, while a fresh open runs vN+1', async () => {
  const dshHome = await scratch('home')
  const home = path.join(dshHome, 'dsh-uml-viewer')
  const old = await host(dshHome, await install('1.0.0'))
  const before = await old.panel('session-old')

  const updated = await host(dshHome, await install('1.1.0'))
  const fresh = await updated.panel('session-new')

  // The running panel still answers from the same vN resident process.
  expect(await before.action({ action: 'viewport', width: 640, height: 480 })).toMatchObject({ accepted: true })
  expect(await before.runs()).toEqual([{ version: '1.0.0', binary: path.join(home, '1.0.0', exe) }])
  expect(await fresh.runs()).toEqual([{ version: '1.1.0', binary: path.join(home, '1.1.0', exe) }])
  expect(existsSync(path.join(home, '1.0.0', exe))).toBe(true)
  expect(JSON.parse(await readFile(path.join(home, 'current.json'), 'utf8'))).toMatchObject({ version: '1.1.0' })
})

it('survives unloading vN and loading vN+1 in the same Cordis root until the old panel closes', async () => {
  const dshHome = await scratch('reload home')
  const home = path.join(dshHome, 'dsh-uml-viewer')
  const old = await host(dshHome, await install('1.0.0'))
  const before = await old.panel('old-session')
  await old.unload()
  expect(existsSync(path.join(before.project, 'resident-exit.json'))).toBe(false)
  const updated = await host(dshHome, await install('1.1.0'), new Map(), old)
  const fresh = await updated.panel('fresh-session')
  const currentViewer = updated.ctx.get('umlViewer') as { action(agent: Agent, action: unknown): Promise<unknown> }
  expect(await currentViewer.action({ id: 'old-session', session: { id: 'old-session' } },
    { action: 'viewport', width: 640, height: 480 })).toMatchObject({ accepted: true })
  expect(await before.runs()).toEqual([{ version: '1.0.0', binary: path.join(home, '1.0.0', exe) }])
  expect(await fresh.runs()).toEqual([{ version: '1.1.0', binary: path.join(home, '1.1.0', exe) }])
  expect(existsSync(path.join(home, '1.0.0', exe))).toBe(true)
  await before.close()
  expect(existsSync(path.join(home, '1.0.0'))).toBe(false)
  expect(existsSync(path.join(home, '1.1.0', exe))).toBe(true)
  // Root shutdown remains a second owner even if a panel never closes.
  expect(updated.children.map(child => child.alive())).toEqual([false, true])
  await updated.dispose()
  expect(updated.children.map(child => child.alive())).toEqual([false, false])
  // A process stopped at shutdown frees its version too, with no lease left.
  expect(existsSync(path.join(home, '1.0.0'))).toBe(false)
  expect(existsSync(path.join(home, 'leases', '1.0.0'))).toBe(false)
})

it('binds a second live panel of the same session to the same vN resident', async () => {
  const dshHome = await scratch('reopen home')
  const home = path.join(dshHome, 'dsh-uml-viewer')
  const old = await host(dshHome, await install('1.0.0'))
  const before = await old.panel('one-session')
  await old.unload()
  const updated = await host(dshHome, await install('1.1.0'), new Map(), old)
  // Two live panels of one session are one resident: the second binds vN. This
  // is the sidebar/main reparent case, not a carrier loss (D13).
  const attached = await updated.panel('one-session')
  expect((await attached.runs()).at(-1)).toEqual({ version: '1.0.0', binary: path.join(home, '1.0.0', exe) })
  await attached.close()
  await before.close()
  // The last panel of that session closed: the next open is a fresh choice.
  const reopened = await updated.panel('one-session')
  expect(await reopened.runs()).toEqual([
    { version: '1.0.0', binary: path.join(home, '1.0.0', exe) },
    { version: '1.1.0', binary: path.join(home, '1.1.0', exe) },
  ])
  expect(updated.children.filter(child => child.alive())).toHaveLength(1)
  await reopened.close()
  expect(existsSync(path.join(home, '1.1.0', exe))).toBe(true)
})

it('keeps vN when a withdrawn stream is followed by the same session’s remount (D13)', async () => {
  const dshHome = await scratch('withdraw home')
  const home = path.join(dshHome, 'dsh-uml-viewer')
  const old = await host(dshHome, await install('1.0.0'))
  const before = await old.panel('one-session')
  await old.unload()
  // What the shipped gateway does when the browser half is unloaded: the method
  // signal aborts and the business iterator is returned. That is a carrier
  // loss, not a user close, so it must not switch the session's version.
  await before.withdraw()
  expect(old.children.map(child => child.alive())).toEqual([true])

  const updated = await host(dshHome, await install('1.1.0'), new Map(), old)
  const remount = await updated.panel('one-session')
  // The remount re-attaches to the waiting vN resident: one child, one version.
  expect(await remount.runs()).toEqual([{ version: '1.0.0', binary: path.join(home, '1.0.0', exe) }])
  expect(updated.children.filter(child => child.alive())).toHaveLength(1)
  expect(await remount.action({ action: 'viewport', width: 640, height: 480 })).toMatchObject({ accepted: true })
  expect(await before.runs()).toEqual([{ version: '1.0.0', binary: path.join(home, '1.0.0', exe) }])

  // A user close is the definite signal: it stops the resident at once.
  await remount.close()
  expect(old.children.map(child => child.alive())).toEqual([false])
  cleanupVersions(home)
  expect(existsSync(path.join(home, '1.0.0'))).toBe(false)
  expect(existsSync(path.join(home, '1.1.0', exe))).toBe(true)
})

it('stops the resident on an explicit close after a withdrawal, saving its view', async () => {
  const dshHome = await scratch('close home')
  const home = path.join(dshHome, 'dsh-uml-viewer')
  const old = await host(dshHome, await install('1.0.0'))
  const before = await old.panel('old-session')
  const project = before.project
  await old.unload()
  await host(dshHome, await install('1.1.0'), new Map(), old)
  await before.withdraw()
  expect(old.children.map(child => child.alive())).toEqual([true])
  // A grace already waiting is not the same as a closed panel: close wins.
  await before.close()
  expect(old.children.map(child => child.alive())).toEqual([false])
  const log = (await readFile(path.join(project, 'resident-actions.log'), 'utf8')).trim().split('\n')
    .map(line => JSON.parse(line) as { action: string })
  expect(log.map(entry => entry.action)).toContain('save')
  expect(existsSync(path.join(home, '1.0.0'))).toBe(false)
})

it('stops every resident at root shutdown while a withdrawal grace is pending', async () => {
  const dshHome = await scratch('shutdown home')
  const home = path.join(dshHome, 'dsh-uml-viewer')
  const old = await host(dshHome, await install('1.0.0'))
  const before = await old.panel('old-session')
  await old.unload()
  const updated = await host(dshHome, await install('1.1.0'), new Map(), old)
  await before.withdraw()
  expect(updated.children.map(child => child.alive())).toEqual([true])
  // Root shutdown is the second owner: the pending window must not keep a child.
  await updated.dispose()
  expect(updated.children.map(child => child.alive())).toEqual([false])
  expect(existsSync(path.join(home, '1.0.0'))).toBe(false)
  expect(existsSync(path.join(home, 'leases', '1.0.0'))).toBe(false)
})

it('removes the old version directory once the host still using it has stopped', async () => {
  const dshHome = await scratch('home')
  const home = path.join(dshHome, 'dsh-uml-viewer')
  const old = await host(dshHome, await install('1.0.0'))
  const before = await old.panel('session-old')
  const updated = await host(dshHome, await install('1.1.0'))
  expect(existsSync(path.join(home, '1.0.0'))).toBe(true)

  await before.close()
  await old.dispose()
  expect(existsSync(path.join(home, '1.0.0'))).toBe(false)
  expect(existsSync(path.join(home, 'leases', '1.0.0'))).toBe(false)
  // The current version stays for the next fresh start, even with no host.
  await updated.dispose()
  expect(await readFile(path.join(home, '1.1.0', exe), 'utf8')).toBe(payloadScript('1.1.0'))
})

it('cleans up only version directories that are neither current nor leased by a live host', async () => {
  const dshHome = await scratch('home')
  const home = path.join(dshHome, 'dsh-uml-viewer')
  const hour = new Date(Date.now() - 60 * 60_000)
  for (const version of ['0.9.0', '0.9.1', '0.9.2', '0.9.3']) {
    await mkdir(path.join(home, version), { recursive: true })
    await writeFile(path.join(home, version, exe), `old ${version}`)
  }
  // 0.9.1: a live host (fresh heartbeat). 0.9.2: a host that died an hour ago.
  await mkdir(path.join(home, 'leases', '0.9.1'), { recursive: true })
  await writeFile(path.join(home, 'leases', '0.9.1', 'live-host.lease'), '{}')
  await mkdir(path.join(home, 'leases', '0.9.2'), { recursive: true })
  await writeFile(path.join(home, 'leases', '0.9.2', 'dead-host.lease'), '{}')
  await utimes(path.join(home, 'leases', '0.9.2', 'dead-host.lease'), hour, hour)
  // 0.9.3 was current before this activation; what is not a version stays.
  await writeFile(path.join(home, 'current.json'), JSON.stringify({ version: '0.9.3' }))
  await mkdir(path.join(home, 'sessions'))
  await writeFile(path.join(home, 'sessions', 'session-a.json'), '{}')
  await mkdir(path.join(home, 'not-a-version'))
  // An old staging copy from a crashed activation, a fresh one from a live one.
  await mkdir(path.join(home, '.staging-crashed'))
  await utimes(path.join(home, '.staging-crashed'), hour, hour)
  await mkdir(path.join(home, '.staging-in-progress'))

  await host(dshHome, await install('1.0.0'))
  expect((await readdir(home)).sort()).toEqual(['.staging-in-progress', '0.9.1', '1.0.0', 'activations.jsonl',
    'current.json', 'leases', 'not-a-version', 'sessions'])
  expect(await readdir(path.join(home, 'leases'))).toEqual(expect.arrayContaining(['0.9.1', '1.0.0']))
  expect(await readdir(path.join(home, 'leases'))).toHaveLength(2)
  expect(await readFile(path.join(home, '0.9.1', exe), 'utf8')).toBe('old 0.9.1')
})

it('publishes one intact copy when several hosts load the same version at once', async () => {
  const dshHome = await scratch('home')
  const home = path.join(dshHome, 'dsh-uml-viewer')
  const source = path.join(await scratch('payload'), exe)
  // Large enough that unsynchronized writers would interleave visibly.
  const bytes = Buffer.alloc(8 << 20)
  for (let i = 0; i < bytes.length; i += 4) bytes.writeUInt32LE((i * 2654435761) >>> 0, i)
  await writeFile(source, bytes)
  const expected = createHash('sha256').update(bytes).digest('hex')
  // Each loader is a separate Node process running the real pinVersion.
  const bundle = path.join(await scratch('bundle'), 'versions.mjs')
  const esbuild = await import('esbuild')
  await esbuild.build({ entryPoints: [fileURLToPath(new URL('../src/versions.ts', import.meta.url))], bundle: true,
    platform: 'node', format: 'esm', outfile: bundle, logLevel: 'silent' })
  const script = `const { pinVersion } = await import(${JSON.stringify(pathToFileURL(bundle).href)})
const pinned = pinVersion(process.argv[1], '1.0.0', process.argv[2], ${JSON.stringify(exe)})
process.stdout.write(pinned.binary)
process.exit(0)`
  const loaders = Array.from({ length: 6 }, () => new Promise<{ code: number | null; out: string; err: string }>(resolve => {
    const child = spawn(process.execPath, ['--input-type=module', '-e', script, home, source], { stdio: ['ignore', 'pipe', 'pipe'] })
    let out = ''
    let err = ''
    child.stdout.on('data', chunk => { out += chunk })
    child.stderr.on('data', chunk => { err += chunk })
    child.on('close', code => resolve({ code, out, err }))
  }))
  const results = await Promise.all(loaders)
  expect(results.map(result => [result.code, result.out, result.err]))
    .toEqual(Array.from({ length: 6 }, () => [0, path.join(home, '1.0.0', exe), '']))
  expect(await sha256(path.join(home, '1.0.0', exe))).toBe(expected)
  expect(await readdir(path.join(home, '1.0.0'))).toEqual([exe])
  expect((await readdir(home)).filter(name => name.startsWith('.'))).toEqual([])
  expect(JSON.parse(await readFile(path.join(home, 'current.json'), 'utf8'))).toEqual({ version: '1.0.0', size: bytes.length, sha256: expected })
}, 60_000)

it.runIf(process.platform === 'win32')('updates while an exe in the old version directory is running, and removes it after exit', async () => {
  const dshHome = await scratch('home')
  const home = path.join(dshHome, 'dsh-uml-viewer')
  // The vN payload is a real Windows executable (a copy of node.exe); the
  // resident it runs keeps that exe image mapped, so Windows denies moving or
  // deleting its directory while the panel is open.
  const wrapperDirectory = await scratch('wrapper')
  const wrapper = path.join(wrapperDirectory, 'resident-1.0.0.cjs')
  await writeFile(wrapper, `require('node:fs').appendFileSync('resident-versions.log', JSON.stringify({ version: '1.0.0', binary: process.execPath }) + '\\n')\nimport(${JSON.stringify(residentFixture)})\n`)
  const nativeExe = path.join(home, '1.0.0', exe)
  const old = await host(dshHome, await install('1.0.0', { native: true }), new Map([[nativeExe, wrapper]]))
  const before = await old.panel('session-old')
  expect(await before.runs()).toEqual([{ version: '1.0.0', binary: nativeExe }])
  // Simulate a lost owner's lease with its resident still running: only the
  // OS now knows the exe is in use. This exercises cleanup's Windows guard,
  // independently of the ordinary live-lease path.
  const leases = path.join(home, 'leases', '1.0.0')
  const longGone = new Date(Date.now() - 60 * 60_000)
  for (const lease of await readdir(leases)) {
    const file = path.join(leases, lease)
    // A dead host: stale heartbeat and no process with that PID.
    await writeFile(file, JSON.stringify({ pid: 2 ** 22 + 17, version: '1.0.0' }))
    await utimes(file, longGone, longGone)
  }

  const updated = await host(dshHome, await install('1.1.0'))
  const fresh = await updated.panel('session-new')
  expect(await fresh.runs()).toEqual([{ version: '1.1.0', binary: path.join(home, '1.1.0', exe) }])
  // Deferred, not failed: the running exe is untouched and still serving.
  expect(existsSync(nativeExe)).toBe(true)
  expect(await before.action({ action: 'viewport', width: 320, height: 200 })).toMatchObject({ accepted: true })
  expect((await readdir(home)).filter(name => name.startsWith('.'))).toEqual([])

  await before.close()
  await old.dispose()
  await fresh.close()
  await updated.dispose()
  expect(existsSync(path.join(home, '1.0.0'))).toBe(false)
  expect(existsSync(path.join(home, '1.1.0', exe))).toBe(true)
}, 60_000)

it.runIf(process.platform === 'win32')('retries removal of a version whose exe was still mapped inside trash', async () => {
  const dshHome = await scratch('home')
  const home = path.join(dshHome, 'dsh-uml-viewer')
  const nativeExe = path.join(home, '1.0.0', exe)
  const native = await install('1.0.0', { native: true })
  const source = path.join(native, 'node_modules', '@dsh-uml-viewer', target, 'bin', exe)
  await mkdir(path.dirname(nativeExe), { recursive: true })
  await copyFile(source, nativeExe)
  // A process maps this image but nothing leases the version.
  const running = spawn(nativeExe, ['-e', 'setTimeout(() => {}, 60000)'], { stdio: 'ignore' })
  try {
    await new Promise(resolve => setTimeout(resolve, 300))
    cleanupVersions(home)
    // Neither the live directory nor a deferred trash copy may lose the image.
    const remaining = await readdir(home)
    const holds = remaining.filter(name => name.startsWith('.trash-') || name === '1.0.0')
    expect(holds).toHaveLength(1)
    expect(await readdir(holds[0] === '1.0.0' ? path.join(home, '1.0.0') : path.join(home, holds[0]!))).toEqual([exe])
  } finally {
    running.kill()
    await new Promise(resolve => running.once('exit', resolve))
  }
  cleanupVersions(home)
  expect((await readdir(home)).filter(name => name === '1.0.0' || name.startsWith('.trash-'))).toEqual([])
}, 60_000)
