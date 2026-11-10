/**
 * The five `uml_*` tools in the conversation (#54): each one registers the
 * host's keyed `tool.call.toolview` slot, so a call reads as its outcome — the
 * quality tools show validity, reason and targets, `uml_ir` shows success and
 * output, and `uml_display` shows the diagram it put on screen together with
 * 「在面板中查看」. Every word comes from `locale/*.json` and follows a DSH
 * language switch, like the panel's own chrome (#42).
 *
 * The panel stays the only viewer: 「在面板中查看」 opens this session's UML tab
 * (the host's navigation, so a closed column expands) and, when the display
 * answered a panel request that named an element, locates that element through
 * the panel's existing `locate` action. A request that named no element opens
 * the tab alone.
 */
import { createElement, useEffect, useState } from 'react'
import type { DisplayResult, IrResult, PanelRequestTarget, QualityToolResult } from '@dsh-uml-viewer/contracts'
import { panelText, type PanelText, type TextKey } from './locale.js'
import type { OverlayController } from './overlays.js'

export type ToolName = 'uml_crap' | 'uml_mutate' | 'uml_dry' | 'uml_ir' | 'uml_display'

/** The wire names the host dispatches through `tool.call.toolview`, in tool order. */
export const TOOL_NAMES: readonly ToolName[] = Object.freeze(['uml_crap', 'uml_mutate', 'uml_dry', 'uml_ir', 'uml_display'])

/** The lazy argument view the host hands a tool row. */
export interface ToolArguments {
  value(key: string): unknown
  text(key: string): string | undefined
}

/** One text block of a tool result. */
export interface ToolContent {
  readonly type?: string
  readonly text?: string
}

/** What one call stage publishes: the dispatched arguments, result content and failure flag. */
export interface ToolBlock {
  readonly args?: ToolArguments | undefined
  readonly content?: readonly ToolContent[] | undefined
  readonly isError?: boolean | undefined
}

/** The part of the host's `ToolCallOwnerProps` these rows read. */
export interface ToolViewProps {
  readonly phase?: 'preparing' | 'start' | 'result'
  readonly block?: ToolBlock | undefined
  /** The session this call belongs to (session-scoped slot), for 「在面板中查看」. */
  readonly sessionId?: string | undefined
  readonly [key: string]: unknown
}

/** One labeled value of a result, e.g. `Reason: …`. */
export interface ToolDetail {
  readonly label: string
  readonly value: string
}

/** Everything a row shows, independent of how it is drawn. */
export interface ToolRow {
  readonly state: 'ok' | 'invalid' | 'pending'
  /** The outcome word: valid/invalid, IR written, shown/not shown. */
  readonly status: string
  /** The line beside the status, e.g. the displayed diagram. */
  readonly summary: string
  readonly details: readonly ToolDetail[]
  /** 「在面板中查看」; null for every other tool and before a result arrives. */
  readonly action: { readonly label: string, run(): void } | null
}

/** A result's text is bounded in the row: the record also lives in Inspect. */
const MAX_TEXT = 2000

function clamp(value: string): string {
  return value.length > MAX_TEXT ? `${value.slice(0, MAX_TEXT)}…` : value
}

/** The result content joined as text; `null` when the call has no text result. */
export function resultText(block: ToolBlock | undefined): string | null {
  const parts = (block?.content ?? [])
    .filter(part => part?.type === 'text' && typeof part.text === 'string')
    .map(part => part.text!)
  return parts.length > 0 ? parts.join('\n') : null
}

/** The version-1 record a tool result carries, or `null` for anything else. */
function recordOf(block: ToolBlock | undefined): Record<string, unknown> | null {
  const text = resultText(block)
  if (text === null) return null
  try {
    const value: unknown = JSON.parse(text)
    return value && typeof value === 'object' && !Array.isArray(value) ? value as Record<string, unknown> : null
  } catch {
    return null
  }
}

/** The targets the call dispatched, which the quality records do not repeat. */
function targetsOf(block: ToolBlock | undefined): string[] {
  const value = block?.args?.value('targets')
  return Array.isArray(value) ? value.filter((entry): entry is string => typeof entry === 'string') : []
}

/** The element a display result named, when it answered such a request. */
function targetOf(record: unknown): PanelRequestTarget | null {
  const value = record && typeof record === 'object' ? (record as Record<string, unknown>).target : undefined
  if (!value || typeof value !== 'object' || Array.isArray(value)) return null
  const { id, kind } = value as Record<string, unknown>
  if (typeof id !== 'string') return null
  if (kind !== 'class' && kind !== 'component') return null
  return { id, kind }
}

function pendingRow(text: PanelText): ToolRow {
  return { state: 'pending', status: text.t('tool.running'), summary: '', details: [], action: null }
}

/** A result the row cannot read: still its raw text, never a silent blank row. */
function unreadableRow(text: PanelText, block: ToolBlock | undefined): ToolRow {
  const raw = resultText(block)
  return {
    state: 'invalid',
    status: text.t('tool.invalid'),
    summary: '',
    details: raw === null ? [] : [{ label: text.t('tool.output'), value: clamp(raw) }],
    action: null,
  }
}

function qualityRow(tool: ToolName, props: ToolViewProps, text: PanelText): ToolRow {
  if (props.phase !== 'result') return pendingRow(text)
  let record: QualityToolResult
  try {
    const value = recordOf(props.block)
    if (value === null) return unreadableRow(text, props.block)
    // The host itself produced this record; a malformed one is still shown readably.
    record = value as unknown as QualityToolResult
    if (typeof record.validResult !== 'boolean') return unreadableRow(text, props.block)
  } catch {
    return unreadableRow(text, props.block)
  }
  const details: ToolDetail[] = []
  if (record.reason !== null && record.reason !== undefined) {
    details.push({ label: text.t('tool.reason'), value: clamp(String(record.reason)) })
  }
  if (typeof record.exitCode === 'number') {
    details.push({ label: text.t('tool.exitCode'), value: String(record.exitCode) })
  }
  const targets = targetsOf(props.block)
  if (targets.length > 0) details.push({ label: text.t('tool.targets'), value: targets.join('\n') })
  else if (tool !== 'uml_mutate') details.push({ label: text.t('tool.targets'), value: text.t('tool.targets.all') })
  return {
    state: record.validResult ? 'ok' : 'invalid',
    status: text.t(record.validResult ? 'tool.valid' : 'tool.invalid'),
    summary: '',
    details,
    action: null,
  }
}

function irRow(props: ToolViewProps, text: PanelText): ToolRow {
  if (props.phase !== 'result') return pendingRow(text)
  const record = recordOf(props.block) as Partial<IrResult> | null
  if (record === null) return unreadableRow(text, props.block)
  const ok = record.status === 'ok' && record.validResult === true
  const details: ToolDetail[] = []
  if (record.reason !== null && record.reason !== undefined) {
    details.push({ label: text.t('tool.reason'), value: clamp(String(record.reason)) })
  }
  if (typeof record.exitCode === 'number') {
    details.push({ label: text.t('tool.exitCode'), value: String(record.exitCode) })
  }
  const output = [record.stdout, record.stderr]
    .filter((part): part is string => typeof part === 'string' && part.trim() !== '')
    .join('\n')
  details.push({ label: text.t('tool.output'), value: output === '' ? text.t('tool.noOutput') : clamp(output) })
  return {
    state: ok ? 'ok' : 'invalid',
    status: text.t(ok ? 'tool.ir.ok' : 'tool.ir.failed'),
    summary: '',
    details,
    action: null,
  }
}

function displayRow(props: ToolViewProps, text: PanelText, view: (target: string | null) => void): ToolRow {
  if (props.phase !== 'result') return pendingRow(text)
  const record = recordOf(props.block) as Partial<DisplayResult> | null
  if (record === null && props.block?.isError !== true) return unreadableRow(text, props.block)
  const shown = record?.status === 'ok'
  const path = typeof record?.path === 'string' ? record.path : props.block?.args?.text('path') ?? null
  // The diagram the panel shows: the path asked for, else the latest one uml_ir wrote.
  const name = path === null ? text.t('tool.display.latest') : path.split(/[\\/]/).pop() || path
  const details: ToolDetail[] = []
  if (!shown) {
    const reason = typeof record?.reason === 'string' ? record.reason : resultText(props.block)
    if (reason !== null && reason !== '') details.push({ label: text.t('tool.reason'), value: clamp(reason) })
  }
  return {
    state: shown ? 'ok' : 'invalid',
    status: text.t(shown ? 'tool.display.ok' : 'tool.display.failed'),
    summary: shown ? text.t('tool.display.diagram', { name }) : '',
    details,
    action: {
      label: text.t('tool.display.view'),
      run: () => view(targetOf(record)?.id ?? null),
    },
  }
}

/**
 * Everything one `uml_*` row shows for the current call stage.
 * @param tool the wire tool name.
 * @param props the host's owner props for this call.
 * @param text the localized panel text.
 * @param view 「在面板中查看」: open the panel, then locate the element when one is named.
 * @returns the row model.
 */
export function toolRow(
  tool: ToolName,
  props: ToolViewProps,
  text: PanelText,
  view: (target: string | null) => void,
): ToolRow {
  if (tool === 'uml_ir') return irRow(props, text)
  if (tool === 'uml_display') return displayRow(props, text, view)
  return qualityRow(tool, props, text)
}

/** One live panel's overlay controller, and the locate a click asked for before it was live. */
export class PanelViews {
  private readonly live = new Map<string, OverlayController>()
  private readonly ready = new Set<string>()
  private readonly waiting = new Map<string, string>()

  /**
   * @param openTab opens (or reveals) the UML tab through the host's navigation.
   */
  public constructor(private readonly openTab: () => void) {}

  /** A panel mounted for this session; it is not live until its first frame. */
  public attach(sessionId: string, overlays: OverlayController): void {
    this.live.set(sessionId, overlays)
  }

  /** The mount is gone (tab closed, plugin unloaded): nothing to locate into. */
  public detach(sessionId: string, overlays: OverlayController): void {
    if (this.live.get(sessionId) !== overlays) return
    this.live.delete(sessionId)
    this.ready.delete(sessionId)
  }

  /** A live frame arrived: the panel takes actions now, so a queued locate runs. */
  public shown(sessionId: string): void {
    this.ready.add(sessionId)
    const target = this.waiting.get(sessionId)
    if (target === undefined) return
    this.waiting.delete(sessionId)
    void this.locate(sessionId, target)
  }

  /**
   * 「在面板中查看」: open this session's UML tab, then locate `target`.
   * @param sessionId the session the call belongs to; unknown opens nothing.
   * @param target the element the display answered, or null to only open the tab.
   */
  public viewInPanel(sessionId: string | undefined, target: string | null): void {
    if (sessionId === undefined) return
    try {
      this.openTab()
    } catch {
      // A host that cannot open the tab still shows the conversation; the
      // click's own outcome is the row's, not an exception in it.
    }
    if (target === null) return
    if (this.ready.has(sessionId)) void this.locate(sessionId, target)
    else this.waiting.set(sessionId, target)
  }

  private locate(sessionId: string, target: string): Promise<void> {
    const overlays = this.live.get(sessionId)
    return overlays === undefined ? Promise.resolve() : overlays.locate(target)
  }
}

/** Re-render a row after the DSH language changes. */
function useText(text: PanelText): PanelText {
  const [, setToken] = useState({})
  useEffect(() => text.subscribe(() => setToken({})), [text])
  return text
}

const STYLE = `
.dsh-uml-toolview {
  display: flex; flex-direction: column; gap: 4px; padding: 6px 8px; border-radius: var(--dsw-radius-sm, 4px);
  border: 1px solid var(--dsw-alias-border-l2); background: var(--dsw-alias-bg-layer-2);
  color: var(--dsw-alias-label-primary); font: 12px/1.5 var(--dsw-font-family, system-ui);
}
.dsh-uml-toolview-head { display: flex; flex-wrap: wrap; align-items: baseline; gap: 6px; }
.dsh-uml-toolview-name { color: var(--dsw-alias-label-secondary); font-family: var(--dsw-font-family-mono, ui-monospace, monospace); }
.dsh-uml-toolview-status { font-weight: 600; }
.dsh-uml-toolview[data-state="ok"] .dsh-uml-toolview-status { color: var(--dsw-alias-state-success-primary, var(--dsw-alias-label-primary)); }
.dsh-uml-toolview[data-state="invalid"] .dsh-uml-toolview-status { color: var(--dsw-alias-state-error-primary); }
.dsh-uml-toolview[data-state="pending"] .dsh-uml-toolview-status { color: var(--dsw-alias-label-tertiary); }
.dsh-uml-toolview-summary { color: var(--dsw-alias-label-secondary); overflow-wrap: anywhere; }
.dsh-uml-toolview-details { display: grid; grid-template-columns: max-content minmax(0, 1fr); gap: 2px 8px; margin: 0; }
.dsh-uml-toolview-details dt { color: var(--dsw-alias-label-tertiary); }
.dsh-uml-toolview-details dd {
  margin: 0; color: var(--dsw-alias-label-primary); white-space: pre-wrap; overflow-wrap: anywhere;
}
.dsh-uml-toolview-action {
  appearance: none; align-self: flex-start; border: 1px solid var(--dsw-alias-border-l2); border-radius: var(--dsw-radius-sm, 4px);
  padding: 2px 10px; background: var(--dsw-alias-button-floating-fill, transparent); color: var(--dsw-alias-label-primary);
  font: inherit; cursor: pointer;
}
.dsh-uml-toolview-action:hover { background: var(--dsw-alias-interactive-bg-hover); }
.dsh-uml-toolview-action:focus-visible {
  outline: var(--dsw-focus-ring-width, 2px) solid var(--dsw-focus-ring-color, var(--dsw-alias-state-business-primary));
  outline-offset: 1px;
}
`

/** Inject the tool rows' stylesheet once per document. */
export function installToolViewStyles(): () => void {
  const id = 'dsh-uml-toolview-style'
  if (document.querySelector(`style[data-plugin-css="${id}"]`) !== null) return () => {}
  const tag = document.createElement('style')
  tag.dataset.pluginCss = id
  tag.textContent = STYLE
  document.head.appendChild(tag)
  return () => { document.querySelector(`style[data-plugin-css="${id}"]`)?.remove() }
}

/** One registered view: a function component for one wire tool name. */
function makeToolView(tool: ToolName, views: PanelViews, text: PanelText): (props: ToolViewProps) => unknown {
  return function UmlToolView(props: ToolViewProps): unknown {
    const current = useText(text)
    const row = toolRow(tool, props, current, target => views.viewInPanel(props.sessionId, target))
    return createElement('div', {
      className: 'dsh-uml-toolview',
      'data-state': row.state,
      role: 'group',
      'aria-label': current.t('tool.label', { tool, status: row.status }),
    },
    createElement('div', { className: 'dsh-uml-toolview-head' },
      createElement('span', { className: 'dsh-uml-toolview-name' }, tool),
      createElement('span', { className: 'dsh-uml-toolview-status' }, row.status),
      row.summary === '' ? null : createElement('span', { className: 'dsh-uml-toolview-summary' }, row.summary)),
    row.details.length === 0 ? null : createElement('dl', { className: 'dsh-uml-toolview-details' },
      row.details.flatMap((detail, index) => [
        createElement('dt', { key: `label-${index}` }, detail.label),
        createElement('dd', { key: `value-${index}` }, detail.value),
      ])),
    row.action === null ? null : createElement('button', {
      type: 'button',
      className: 'dsh-uml-toolview-action',
      'aria-label': row.action.label,
      onClick: () => row.action!.run(),
    }, row.action.label))
  }
}

/** The slots face this module registers through (`ctx.slots`). */
interface ToolSlotsFace {
  register(descriptor: { name: string, key: string }, component: (props: ToolViewProps) => unknown): () => void
  inject(name: string, callback: () => (() => void) | undefined): unknown
}

/** The client context this module needs. */
interface ToolClientContext {
  readonly slots: ToolSlotsFace
  readonly get: (name: string) => unknown
}

/**
 * Register all five `uml_*` tool result views and their stylesheet.
 * @param ctx the client context (needs `slots`).
 * @param views the panel this session's tab lives in.
 * @returns the disposers' disposer.
 */
export function registerToolViews(ctx: ToolClientContext, views: PanelViews): () => void {
  const text = panelText(name => ctx.get(name))
  const disposers: (() => void)[] = []
  const own = (result: unknown): void => {
    if (typeof result === 'function') disposers.push(result as () => void)
  }
  const stopStyles = installToolViewStyles()
  for (const tool of TOOL_NAMES) {
    own(ctx.slots.inject('tool.call.toolview', () => ctx.slots.register(
      { name: 'tool.call.toolview', key: tool },
      props => makeToolView(tool, views, text)(props),
    )))
  }
  return () => {
    for (const dispose of disposers.reverse()) dispose()
    stopStyles()
  }
}

/** Text keys the rows read, so a locale file that drops one fails the language test. */
export const TOOL_TEXT_KEYS: readonly TextKey[] = Object.freeze([
  'tool.valid', 'tool.invalid', 'tool.running', 'tool.reason', 'tool.exitCode', 'tool.targets',
  'tool.targets.all', 'tool.output', 'tool.noOutput', 'tool.ir.ok', 'tool.ir.failed',
  'tool.display.ok', 'tool.display.failed', 'tool.display.diagram', 'tool.display.latest',
  'tool.display.view', 'tool.label',
])
