/**
 * The five `uml_*` conversation rows (#54) and 「在面板中查看」. Expected values
 * are the words the language files carry, and the element ids the panel
 * request protocol defines; nothing here reads the row's DOM or its internal
 * state.
 */
import { describe, expect, it, vi } from 'vitest'
import type { PanelText } from '../src/locale.js'
import { translate } from '../src/locale.js'
import { PanelViews, toolRow, TOOL_NAMES, TOOL_TEXT_KEYS, type ToolBlock, type ToolViewProps } from '../src/tool-views.js'
import type { OverlayController } from '../src/overlays.js'

// React is host-injected (ADR 0013, D1); these rows' models never render.
vi.mock('react', () => ({ createElement: vi.fn(), useEffect: vi.fn(), useState: () => [{}, () => {}] }))

const textFor = (language: 'en' | 'zh'): PanelText => ({
  language,
  t: (key, params) => translate(language, key, params),
  subscribe: () => () => {},
})

const en = textFor('en')
const zh = textFor('zh')

/** A result stage carrying one JSON record, as the host renders it. */
function result(record: unknown, args: Readonly<Record<string, unknown>> = {}): ToolViewProps {
  return {
    phase: 'result',
    block: {
      args: {
        value: key => args[key],
        text: key => typeof args[key] === 'string' ? args[key] as string : undefined,
      },
      content: [{ type: 'text', text: JSON.stringify(record) }],
      isError: false,
    },
    sessionId: 'session-a',
  }
}

const quality = (patch: Record<string, unknown> = {}): Record<string, unknown> => ({
  version: 1, operation: 'mutate', projectRoot: '/p', exitCode: 0, validResult: true, crapScope: null,
  configured: true, tool: 'mutator', reason: null, ...patch,
})

const display = (patch: Record<string, unknown> = {}): Record<string, unknown> => ({
  version: 1, operation: 'display', projectRoot: '/p', path: 'diagram.edn', exitCode: null, validResult: true,
  crapScope: null, configured: false, tool: 'uml-viewer', status: 'ok', reason: null, target: null, ...patch,
})

const ir = (patch: Record<string, unknown> = {}): Record<string, unknown> => ({
  version: 1, operation: 'ir', projectRoot: '/p', exitCode: 0, validResult: true, crapScope: null,
  configured: false, tool: 'uml-viewer', status: 'ok', reason: null, stdout: 'wrote diagram.edn\n',
  stderr: '', truncated: false, ...patch,
})

describe('the quality results (#54)', () => {
  it('shows validity and the targets the call dispatched', () => {
    const row = toolRow('uml_mutate', result(quality(), { targets: ['src/a.ts', 'src/b.ts'] }), en, () => {})
    expect(row.state).toBe('ok')
    expect(row.status).toBe('Valid')
    expect(row.details).toEqual([
      { label: 'Exit code', value: '0' },
      { label: 'Targets', value: 'src/a.ts\nsrc/b.ts' },
    ])
    expect(row.action).toBeNull()
  })

  it('makes an invalid result readable: the reason, the exit code and the targets', () => {
    const row = toolRow('uml_crap', result(
      { version: 1, operation: 'crap', projectRoot: '/p', exitCode: 1, validResult: false, crapScope: null,
        configured: false, tool: 'crapper', reason: 'operations must declare crap and mutate' },
      { targets: ['src/a.ts'] },
    ), en, () => {})
    expect(row.state).toBe('invalid')
    expect(row.status).toBe('Invalid')
    expect(row.details).toEqual([
      { label: 'Reason', value: 'operations must declare crap and mutate' },
      { label: 'Exit code', value: '1' },
      { label: 'Targets', value: 'src/a.ts' },
    ])
  })

  it('names the whole business scope when a call dispatched no targets', () => {
    const row = toolRow('uml_dry', result(quality({ operation: 'dry', tool: 'dryer' })), en, () => {})
    expect(row.details).toContainEqual({ label: 'Targets', value: 'the whole business scope' })
  })

  it('shows a running call as pending rather than as a blank result', () => {
    const row = toolRow('uml_crap', { phase: 'start', block: { args: { value: () => undefined, text: () => undefined } } }, en, () => {})
    expect(row.state).toBe('pending')
    expect(row.status).toBe('Running')
    expect(row.details).toEqual([])
  })

  it('never blanks an unreadable result', () => {
    const block: ToolBlock = { content: [{ type: 'text', text: 'quality crashed' }], isError: true }
    const row = toolRow('uml_crap', { phase: 'result', block, sessionId: 'session-a' }, en, () => {})
    expect(row.state).toBe('invalid')
    expect(row.details).toEqual([{ label: 'Output', value: 'quality crashed' }])
  })
})

describe('the IR result (#54)', () => {
  it('shows success and the output it wrote', () => {
    const row = toolRow('uml_ir', result(ir()), en, () => {})
    expect(row.state).toBe('ok')
    expect(row.status).toBe('IR written')
    expect(row.details).toEqual([
      { label: 'Exit code', value: '0' },
      { label: 'Output', value: 'wrote diagram.edn\n' },
    ])
  })

  it('shows a refusal with its reason', () => {
    const row = toolRow('uml_ir', result(ir({ status: 'refused', validResult: false, exitCode: 1,
      reason: 'latest quality call failed', stdout: '', stderr: 'boom\n' })), en, () => {})
    expect(row.state).toBe('invalid')
    expect(row.status).toBe('IR failed')
    expect(row.details).toEqual([
      { label: 'Reason', value: 'latest quality call failed' },
      { label: 'Exit code', value: '1' },
      { label: 'Output', value: 'boom\n' },
    ])
  })
})

describe('the display result and 「在面板中查看」 (#54)', () => {
  it('shows the diagram name and opens the panel on the element the request named', () => {
    const targets: (string | null)[] = []
    const props = result(display({ target: { id: ':core', kind: 'component' } }))
    const row = toolRow('uml_display', props, en, target => targets.push(target))
    expect(row.state).toBe('ok')
    expect(row.status).toBe('Shown')
    expect(row.summary).toBe('Diagram: diagram.edn')
    expect(row.action?.label).toBe('View in panel')
    row.action!.run()
    expect(targets).toEqual([':core'])
  })

  it('only opens the panel when the display answered no element request', () => {
    const targets: (string | null)[] = []
    const row = toolRow('uml_display', result(display()), en, target => targets.push(target))
    expect(row.summary).toBe('Diagram: diagram.edn')
    row.action!.run()
    expect(targets).toEqual([null])
  })

  it('names the latest diagram when no path was asked for', () => {
    const row = toolRow('uml_display', result(display({ path: null })), en, () => {})
    expect(row.summary).toBe('Diagram: the latest diagram')
  })

  it('shows a refused display with its reason and no action before a result', () => {
    const failed = toolRow('uml_display', result(display({ status: 'refused', validResult: false, reason: 'run uml_ir first' })), en, () => {})
    expect(failed.state).toBe('invalid')
    expect(failed.status).toBe('Not shown')
    expect(failed.details).toEqual([{ label: 'Reason', value: 'run uml_ir first' }])
    const pending = toolRow('uml_display', { phase: 'start' }, en, () => {})
    expect(pending.state).toBe('pending')
    expect(pending.action).toBeNull()
  })
})

describe('the tool rows follow the DSH language (#54)', () => {
  it('shows the same rows in Chinese', () => {
    const valid = toolRow('uml_mutate', result(quality(), { targets: ['src/a.ts'] }), zh, () => {})
    expect(valid.status).toBe('有效')
    expect(valid.details).toEqual([
      { label: '退出码', value: '0' },
      { label: '目标', value: 'src/a.ts' },
    ])
    const invalid = toolRow('uml_dry', result(quality({ validResult: false, reason: 'duplicate' })), zh, () => {})
    expect(invalid.status).toBe('无效')
    expect(invalid.details[0]).toEqual({ label: '原因', value: 'duplicate' })
    const shown = toolRow('uml_display', result(display({ target: { id: ':core', kind: 'class' } })), zh, () => {})
    expect(shown.status).toBe('已展示')
    expect(shown.summary).toBe('图：diagram.edn')
    expect(shown.action?.label).toBe('在面板中查看')
  })

  it('has a language entry for every word the rows show', () => {
    for (const key of TOOL_TEXT_KEYS) {
      expect(translate('en', key, { tool: 'uml_ir', status: 'x', name: 'y' }), key).not.toBe(key)
      expect(translate('zh', key, { tool: 'uml_ir', status: 'x', name: 'y' }), key).not.toBe(key)
    }
  })
})

/** A panel controller that only records the locates it was asked to send. */
function fakeOverlays(): { locate(id: string): Promise<void>, sent: string[] } {
  const sent: string[] = []
  return { sent, locate(id) { sent.push(id); return Promise.resolve() } }
}

const asOverlays = (value: { locate(id: string): Promise<void> }): OverlayController => value as unknown as OverlayController

describe('「在面板中查看」 opens the panel and locates once it can take actions', () => {
  it('opens the tab and locates immediately when the panel is live', () => {
    let opened = 0
    const views = new PanelViews(() => { opened += 1 })
    const overlays = fakeOverlays()
    views.attach('session-a', asOverlays(overlays))
    views.shown('session-a')
    views.viewInPanel('session-a', ':core')
    expect(opened).toBe(1)
    expect(overlays.sent).toEqual([':core'])
  })

  it('waits for the panel’s first frame when the tab is being opened', async () => {
    let opened = 0
    const views = new PanelViews(() => { opened += 1 })
    views.viewInPanel('session-a', ':core')
    expect(opened).toBe(1)
    const overlays = fakeOverlays()
    views.attach('session-a', asOverlays(overlays))
    expect(overlays.sent).toEqual([])
    views.shown('session-a')
    await Promise.resolve()
    expect(overlays.sent).toEqual([':core'])
    // One request, one locate: a later frame does not repeat it.
    views.shown('session-a')
    await Promise.resolve()
    expect(overlays.sent).toEqual([':core'])
  })

  it('only opens the tab when the display named no element', () => {
    let opened = 0
    const views = new PanelViews(() => { opened += 1 })
    const overlays = fakeOverlays()
    views.attach('session-a', asOverlays(overlays))
    views.shown('session-a')
    views.viewInPanel('session-a', null)
    expect(opened).toBe(1)
    expect(overlays.sent).toEqual([])
  })

  it('locates into the newest panel of that session and nothing after it detached', () => {
    const views = new PanelViews(() => {})
    const first = fakeOverlays()
    const second = fakeOverlays()
    views.attach('session-a', asOverlays(first))
    views.shown('session-a')
    views.attach('session-a', asOverlays(second))
    views.detach('session-a', asOverlays(second))
    views.viewInPanel('session-a', ':api')
    expect(first.sent).toEqual([])
    expect(second.sent).toEqual([])
  })

  it('leaves the five wire tool names together', () => {
    expect(TOOL_NAMES).toEqual(['uml_crap', 'uml_mutate', 'uml_dry', 'uml_ir', 'uml_display'])
  })
})
