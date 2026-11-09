import { readFileSync } from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'
import vm from 'node:vm'
import { describe, expect, it } from 'vitest'

/**
 * The checks that can only be made on the built artifact: the browser bundle
 * is `lib/client.js`, produced by `pnpm build`. They assert the client facet
 * the host serves, and that React and the UI primitives never enter the
 * payload (DECISIONS.md D1).
 */
const root = path.resolve(fileURLToPath(new URL('../../..', import.meta.url)))
const bundlePath = path.join(root, 'lib', 'client.js')
const pkg = JSON.parse(readFileSync(path.join(root, 'package.json'), 'utf8')) as {
  name: string
  exports: Record<string, string>
  dsh?: { client?: { platform?: string, immediately?: boolean, inject?: string[], external?: string[] } }
  files: string[]
}
const bundle = readFileSync(bundlePath, 'utf8')

interface Factory {
  factory(require: (name: string) => unknown): Record<string, unknown>
  id: string
}

function registered(): Factory {
  let captured: Factory | null = null
  const sandbox = {
    window: { __ModuleLoader__: { load: (value: Factory) => { captured = value } } },
    console,
    document: fakeDocument(),
  }
  vm.createContext(sandbox)
  vm.runInContext(bundle, sandbox)
  if (!captured) throw new Error('lib/client.js did not register with window.__ModuleLoader__.load')
  return captured
}

/** The smallest DOM that `apply` touches: the injected stylesheet only. */
function fakeDocument(): Record<string, unknown> {
  const head = { appendChild: () => undefined }
  return {
    head,
    querySelector: () => null,
    createElement: () => ({ dataset: {}, textContent: '', className: '' }),
  }
}

interface SlotDescriptor { name: string; key?: string; id?: string }

function clientContext(): {
  ctx: Record<string, unknown>
  slots: { name: string; descriptor: SlotDescriptor; component: (props: Record<string, unknown>) => unknown }[]
  tabs: Record<string, unknown>[]
  services: string[]
} {
  const slots: { name: string; descriptor: SlotDescriptor; component: (props: Record<string, unknown>) => unknown }[] = []
  const tabs: Record<string, unknown>[] = []
  const services: string[] = []
  const ctx: Record<string, unknown> = {
    slots: {
      register(descriptor: SlotDescriptor, component: (props: Record<string, unknown>) => unknown) {
        slots.push({ name: descriptor.name, descriptor, component })
        return () => undefined
      },
      inject(name: string, callback: () => unknown) {
        services.push(name)
        return callback()
      },
    },
    sidebarRightTabs: {
      register(tab: Record<string, unknown>) {
        tabs.push(tab)
        return () => undefined
      },
    },
    effect(callback: () => unknown) { callback() },
    inject(names: readonly string[], callback: (value: unknown) => unknown) {
      services.push(...names)
      return callback(ctx)
    },
    get: () => undefined,
  }
  return { ctx, slots, tabs, services }
}

describe('built client bundle', () => {
  it('declares the client facet the host reads: dsh.client, a ./client export, and the shipped files', () => {
    expect(pkg.dsh?.client?.platform).toBe('web')
    expect(pkg.dsh?.client?.immediately).toBe(true)
    // Activation order only: the Remote gateway and the right sidebar must be
    // mounted before the tab registers.
    expect(pkg.dsh?.client?.inject).toEqual(['@deepseek-ai/dsh-api-gateway', '@deepseek-ai/dsh-client-ui-sidebar-right'])
    // No non-baseline module requests: everything but React is bundled.
    expect(pkg.dsh?.client?.external ?? []).toEqual([])
    expect(pkg.exports['./client']).toBe('./lib/client.js')
    expect(pkg.files).toContain('lib')
    expect(pkg.files).toContain('fonts')
    // The bundle is a closure-factory registration for the package name.
    expect(bundle.startsWith('window.__ModuleLoader__.load({ id: "dsh-uml-viewer", factory: (require) => {')).toBe(true)
    expect(bundle).not.toContain('var(--dsw-alias-state-error-primary, #')
    const factory = registered()
    expect(factory.id).toBe('dsh-uml-viewer')
  })

  it('carries no React, react-dom or dsh-client-ui-primitives payload', () => {
    expect(bundle).not.toContain('dsh-client-ui-primitives')
    expect(bundle).not.toContain('react-dom')
    expect(bundle).not.toContain('__SECRET_INTERNALS_DO_NOT_USE_OR_YOU_WILL_BE_FIRED')
    expect(bundle).not.toContain('react.production.min')
    // The only module request is the host-injected React.
    const requests = [...bundle.matchAll(/require\(\s*(["'])([^"']+)\1\s*\)/g)].map(match => match[2])
    expect([...new Set(requests)]).toEqual(['react'])
  })

  it('mounts as a Cordis client plugin and registers a session-bound panel, not a dangling entry', () => {
    const factory = registered()
    const module = factory.factory(name => {
      expect(name).toBe('react')
      return {
        createElement: (type: unknown, props: Record<string, unknown> | null, ...children: unknown[]) => ({ type, props, children }),
        useRef: () => ({ current: null }), useState: (value: unknown) => [value, () => {}],
        useCallback: (callback: unknown) => callback,
      }
    })
    expect(module.inject).toEqual(['slots'])
    expect(module.name).toBe('dsh-uml-viewer')
    expect(typeof module.apply).toBe('function')

    const harness = clientContext()
    ;(module.apply as (ctx: unknown) => void)(harness.ctx)
    expect(harness.tabs).toEqual([
      expect.objectContaining({ id: 'dsh-uml-viewer', kind: 'dsh-uml-viewer' }),
    ])
    const names = harness.slots.map(entry => entry.name)
    expect(names).toEqual(['sidebar.right.pane.tab', 'sidebar.right.pane.tab.title', 'main'])
    for (const entry of harness.slots) expect(entry.descriptor.key).toBe('dsh-uml-viewer')
    // Every registration went through a slot/service injection that exists.
    expect(harness.services).toEqual(['sidebarRightTabs', 'sidebar.right.pane.tab', 'sidebar.right.pane.tab.title', 'main'])

    // The pane component is a real component: React is asked for the panel element,
    // and the element it builds carries the panel class the stylesheet targets.
    const pane = harness.slots[0]!.component({ sessionId: 'session-a' })
    const element = pane as { type: string; props: Record<string, unknown> }
    expect(element.type).toBe('div')
    expect(element.props.className).toBe('dsh-uml-viewer-panel')
    const canvasHost = (pane as { children: { props: Record<string, unknown> }[] }).children[0]!
    expect(typeof canvasHost.props.ref).toBe('function')
  })
})
