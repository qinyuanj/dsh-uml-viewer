import { Context } from '@deepseek-ai/cordis'
import { createHash } from 'node:crypto'
import { readFile } from 'node:fs/promises'
import { fileURLToPath } from 'node:url'
import { afterEach, expect, it } from 'vitest'
import { PANEL_FONT_PATH } from '@dsh-uml-viewer/contracts'

const plugin = await import(new URL('../../../lib/host.js', import.meta.url).href) as typeof import('../src/index.js')
const packagedFace = fileURLToPath(new URL('../../../fonts/SarasaMonoSC-Regular.ttf', import.meta.url))
// The installer's pin for Sarasa Mono SC Regular 1.0.42 (FONT_SHA256 of the frozen qinyuanj/uml-viewer scripts/uml_posix.py).
const FONT_SHA256 = '82e09460f6f55010296f670b85e346ddeaf91b967724840d2bf5514137f68a83'

const contexts: Context[] = []
afterEach(async () => { for (const ctx of contexts.splice(0)) await ctx.fiber.dispose() })

type Route = { kind: string; path: string; resource?: (request: Request) => Response | Promise<Response> }

it('packages the pinned Sarasa face, not a stand-in', async () => {
  const bytes = await readFile(packagedFace)
  expect(createHash('sha256').update(bytes).digest('hex')).toBe(FONT_SHA256)
})

it('serves the packaged face as font/ttf on the route the client loads, for the plugin lifetime', async () => {
  const routes: Route[] = []
  const ctx = new Context()
  contexts.push(ctx)
  ctx.provide('tools', { register: () => () => {} })
  ctx.provide('sandboxPolicy', { resolve: () => ({ mode: 'read-only', workspaceRoot: '/' }) })
  ctx.provide('workingDirectory', { get: () => '/', ensure: async () => '/' })
  ctx.provide('subprocess', { spawn: () => { throw new Error('no spawn expected') } })
  ctx.provide('webServer', { register(route: Route) { routes.push(route); return () => { routes.splice(routes.indexOf(route), 1) } } })
  // Route activation requires a pinned executable even though this fixture never spawns it.
  const fiber = ctx.plugin(plugin, { binary: process.execPath })
  await fiber
  expect(routes.map(route => [route.kind, route.path])).toEqual([['exact', PANEL_FONT_PATH]])
  const response = await routes[0]!.resource!(new Request(`http://127.0.0.1${PANEL_FONT_PATH}`))
  expect(response.status).toBe(200)
  expect(response.headers.get('content-type')).toBe('font/ttf')
  expect(response.headers.get('x-content-type-options')).toBe('nosniff')
  const body = Buffer.from(await response.arrayBuffer())
  expect(body.equals(await readFile(packagedFace))).toBe(true)
  await fiber.dispose()
  expect(routes).toEqual([])
})
