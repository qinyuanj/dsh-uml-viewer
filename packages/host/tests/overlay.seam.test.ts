import { Context } from '@deepseek-ai/cordis'
import { spawn } from 'node:child_process'
import { mkdir, mkdtemp, readFile, rm, writeFile } from 'node:fs/promises'
import os from 'node:os'
import path from 'node:path'
import { fileURLToPath } from 'node:url'
import { afterEach, expect, it } from 'vitest'
import type { PanelAck, PanelEvent } from '@dsh-uml-viewer/contracts'

/**
 * Host seam (#27): the overlay actions reach the resident child verbatim and
 * their models come back on that action's ack. Another panel watching the same
 * session must not receive a second panel's card or source window.
 */
const builtHost = new URL('../../../lib/host.js', import.meta.url).href
const plugin = await import(builtHost) as typeof import('../src/index.js')
import type { Agent, HostServices, SpawnSpec, ToolDefinition } from '../src/dsh.js'

const goldenFrame = fileURLToPath(new URL('../../../crates/frame/tests/fixtures/frames/root.frame.json', import.meta.url))
const fakeBinary = [process.execPath, fileURLToPath(new URL('./fixtures/uml-viewer.mjs', import.meta.url))]

const card = {
  id: ':core.order', title: 'order', contentHeight: 342, viewHeight: 120, scroll: 192, maxScroll: 222,
  items: [{
    kind: 'name', y: 0, h: 20, text: 'order', color: { role: 'ink' }, titleFont: true, x: 16,
    clipRight: null, cells: [], bars: [], stripe: false, swatch: null, clickable: false,
    module: false, opName: null, rel: null,
  }],
}
const source = {
  // A whole, contiguous source model (rows 1..n, tokens joining `display`),
  // as the bounded contracts validator requires.
  file: 'D:/project/src/order.rs', title: 'src/order.rs:2', line: 2, lang: 'rust',
  rows: [
    { number: 1, text: '// order', display: '// order', marked: false, tokens: [{ text: '// order', kind: 'comment' }] },
    { number: 2, text: '\tpub fn place()', display: '  pub fn place()', marked: true,
      tokens: [{ text: '  ', kind: 'ink' }, { text: 'pub', kind: 'keyword' }, { text: ' fn place()', kind: 'ink' }] },
  ],
}

const roots: string[] = []
const contexts: Context[] = []
const controllers: AbortController[] = []
afterEach(async () => {
  for (const controller of controllers.splice(0)) controller.abort()
  for (const ctx of contexts.splice(0)) await ctx.fiber.dispose()
  for (const root of roots.splice(0)) await rm(root, { recursive: true, force: true })
})

function localSubprocess(spec: SpawnSpec) {
  const child = spawn(spec.argv[0]!, spec.argv.slice(1), { cwd: spec.cwd, stdio: ['pipe', 'pipe', 'pipe'] })
  child.stderr.resume()
  return {
    stdin: child.stdin,
    stdout: child.stdout,
    stderr: child.stderr,
    collected: { stdout: { readFrom: () => ({ text: '', lossy: false }) }, stderr: { readFrom: () => ({ text: '', lossy: false }) } },
    done: new Promise<{ exitCode: number | null; signal: string | null }>((resolve, reject) => {
      child.on('error', reject)
      child.on('close', (exitCode, signal) => resolve({ exitCode, signal }))
    }),
    terminate: () => child.kill(),
    waitForExit: async () => true,
  }
}

async function setup(config: Record<string, unknown> = {}) {
  const home = await mkdtemp(path.join(os.tmpdir(), 'uml overlay home '))
  const root = await mkdtemp(path.join(os.tmpdir(), 'uml overlay project '))
  roots.push(home, root)
  await writeFile(path.join(root, 'resident.json'), JSON.stringify({
    framePath: goldenFrame, card, source, ...config,
  }))
  const agent = { id: 'session-a', session: { id: 'session-a' }, ctx: {} } as unknown as Agent
  const definitions = new Map<string, ToolDefinition>()
  const services: HostServices = {
    tools: { register(definition) { definitions.set(definition.name, definition); return () => { definitions.delete(definition.name) } } },
    sandboxPolicy: { resolve: () => ({ mode: 'danger-full-access', workspaceRoot: root }) },
    workingDirectory: { get: () => root, ensure: async () => root },
    subprocess: { spawn: spec => localSubprocess(spec) },
    sandbox: { confine: async argv => ({ argv: [...argv], enforcement: 'full' }) },
    typert: { register: () => async () => {} },
    dshHomePath: (...segments: string[]) => path.join(home, ...segments),
  }
  const ctx = new Context()
  contexts.push(ctx)
  for (const [key, value] of Object.entries(services)) ctx.provide(key, value)
  const fiber = ctx.plugin(plugin, { binary: fakeBinary, home: path.join(home, 'dsh-uml-viewer') })
  await fiber
  const viewer = ctx.get('umlViewer') as unknown as {
    watch(agent: Agent, signal: AbortSignal): AsyncIterable<PanelEvent>
    action(agent: Agent, action: unknown): Promise<PanelAck>
  }
  const action = (request: unknown) => viewer.action(agent, request)
  const stream = () => {
    const controller = new AbortController()
    controllers.push(controller)
    const iterator = viewer.watch(agent, controller.signal)[Symbol.asyncIterator]()
    return {
      next: async () => (await iterator.next()) as IteratorResult<PanelEvent>,
      close: async () => { controller.abort(); await iterator.return?.(undefined) },
    }
  }
  const actions = async () => {
    const text = await readFile(path.join(root, 'resident-actions.log'), 'utf8')
    return text.trim().split('\n').map(line => JSON.parse(line) as Record<string, unknown>)
  }
  return { root, action, stream, actions, fiber }
}

it('answers card and open-source on the ack of their own action', async () => {
  const { action } = await setup()
  expect(await action({ action: 'card', viewHeight: 120 })).toEqual({ accepted: true, reason: null, card })
  expect(await action({ action: 'open-source', id: ':core.order', member: 'place' }))
    .toEqual({ accepted: true, reason: null, source })
  // A module source has no target line; the resident's error is a refused ack.
  expect(await action({ action: 'open-source', id: ':core.order' })).toEqual({ accepted: true, reason: null, source })
})

it('forwards the overlay actions and their fields to the resident unchanged', async () => {
  const { action, actions } = await setup()
  await action({ action: 'open-card', at: [12, 34] })
  await action({ action: 'open-card', id: ':core.order' })
  await action({ action: 'scroll-card', delta: 4, viewHeight: 120 })
  await action({ action: 'card', width: 900, viewHeight: 700 })
  await action({ action: 'close-card' })
  const sent = await actions()
  expect(sent.filter(entry => ['open-card', 'scroll-card', 'card', 'close-card'].includes(String(entry.action))))
    .toEqual([
      expect.objectContaining({ action: 'open-card', at: [12, 34] }),
      expect.objectContaining({ action: 'open-card', id: ':core.order' }),
      expect.objectContaining({ action: 'scroll-card', delta: 4, viewHeight: 120 }),
      expect.objectContaining({ action: 'card', width: 900, viewHeight: 700 }),
      expect.objectContaining({ action: 'close-card' }),
    ])
})

it('does not broadcast one panel\'s card or source window into another panel\'s stream', async () => {
  const { action, stream } = await setup()
  const first = stream()
  expect((await first.next()).value).toMatchObject({ kind: 'ready' })
  expect((await first.next()).value).toMatchObject({ kind: 'frame' })
  const second = stream()
  expect((await second.next()).value).toMatchObject({ kind: 'ready' })
  expect((await second.next()).value).toMatchObject({ kind: 'frame' })

  const ack = await action({ action: 'open-source', id: ':core.order', member: 'place' })
  expect(ack.source).toEqual(source)
  await action({ action: 'card' })
  // A following search acts as an ordered sentinel: neither stream gets a
  // source/card payload before it. Saving state may legitimately emit state
  // events between actions (navigation ticket #23).
  await action({ action: 'search', query: 'sentinel' })
  for (const watching of [first, second]) {
    const kinds: string[] = []
    for (;;) {
      const next = await watching.next()
      if (next.done) throw new Error('watch stream ended before search sentinel')
      kinds.push(next.value.kind)
      if (next.value.kind === 'search') break
    }
    expect(kinds).not.toContain('source')
    expect(kinds).not.toContain('card')
  }
  await first.close()
  await second.close()
})

it('carries a null card when no class card is open (#47)', async () => {
  const { action } = await setup({ card: null })
  expect(await action({ action: 'card', viewHeight: 120 })).toEqual({ accepted: true, reason: null, card: null })
})

it('reports a resident refusal without inventing a model', async () => {
  const { action } = await setup({ sourceError: 'Source not found', cardError: 'no class card is open' })
  expect(await action({ action: 'open-source', id: ':core.order', member: 'absent' }))
    .toEqual({ accepted: false, reason: 'Source not found' })
  expect(await action({ action: 'card' })).toEqual({ accepted: false, reason: 'no class card is open' })
})
