/**
 * Test doubles for the Canvas 2D surface and the host theme. They record what
 * the painter asked the browser to draw, which is the observable behavior of a
 * Canvas renderer, and resolve CSS custom properties from a token table so the
 * theme tests can flip light/dark.
 */
import { estimateW } from '../src/fit.js'

export interface DrawOp {
  op: string
  fillStyle?: string
  strokeStyle?: string
  font?: string
  globalAlpha?: number
  text?: string
  x?: number
  y?: number
  width?: number
  height?: number
  /** Every numeric argument of the call, in order (path geometry). */
  args?: number[]
  lineWidth?: number
  lineJoin?: string
  lineCap?: string
  lineDash?: number[]
  textAlign?: string
  textBaseline?: string
}

export interface RecordingContext {
  ops: DrawOp[]
  font: string
  fillStyle: string
  strokeStyle: string
  lineWidth: number
  globalAlpha: number
  textAlign: string
  textBaseline: string
  measureText(text: string): { width: number }
  [key: string]: unknown
}

const PATH_OPS = ['beginPath', 'moveTo', 'lineTo', 'arcTo', 'arc', 'ellipse', 'closePath', 'setTransform',
  'setLineDash', 'fillRect', 'fill', 'stroke', 'fillText', 'rect', 'save', 'restore']

export function recordingContext(measure?: (text: string, sizePx: number) => number): RecordingContext {
  const ops: DrawOp[] = []
  const state = {
    font: '10px sans-serif',
    fillStyle: '#000000',
    strokeStyle: '#000000',
    lineWidth: 1,
    lineJoin: 'miter',
    lineCap: 'butt',
    globalAlpha: 1,
    textAlign: 'left',
    textBaseline: 'alphabetic',
    lineDash: [] as number[],
  }
  const snapshot = (): Partial<DrawOp> => ({
    fillStyle: state.fillStyle,
    strokeStyle: state.strokeStyle,
    font: state.font,
    globalAlpha: state.globalAlpha,
    lineWidth: state.lineWidth,
    lineJoin: state.lineJoin,
    lineCap: state.lineCap,
    lineDash: [...state.lineDash],
    textAlign: state.textAlign,
    textBaseline: state.textBaseline,
  })
  const record = (op: string, extra: Partial<DrawOp> = {}): void => {
    ops.push({ op, ...snapshot(), ...extra })
  }
  const ctx = {
    ops,
    imageSmoothingEnabled: true,
    get font() { return state.font },
    set font(value: string) { state.font = value },
    get fillStyle() { return state.fillStyle },
    set fillStyle(value: string) { state.fillStyle = value },
    get strokeStyle() { return state.strokeStyle },
    set strokeStyle(value: string) { state.strokeStyle = value },
    get lineWidth() { return state.lineWidth },
    set lineWidth(value: number) { state.lineWidth = value },
    get lineJoin() { return state.lineJoin },
    set lineJoin(value: string) { state.lineJoin = value },
    get lineCap() { return state.lineCap },
    set lineCap(value: string) { state.lineCap = value },
    get globalAlpha() { return state.globalAlpha },
    set globalAlpha(value: number) { state.globalAlpha = value },
    get textAlign() { return state.textAlign },
    set textAlign(value: string) { state.textAlign = value },
    get textBaseline() { return state.textBaseline },
    set textBaseline(value: string) { state.textBaseline = value },
    measureText(text: string) {
      // Default: a monospaced face whose advance is the layout estimate at the
      // current font size, so `fit` is compared against the documented rule.
      // A test may inject the real Sarasa metrics instead.
      const match = /(\d+(?:\.\d+)?)px/.exec(state.font)
      const size = match ? Number(match[1]) : 10
      return { width: measure ? measure(text, size) : estimateW(text, size) }
    },
  } as unknown as RecordingContext
  for (const name of PATH_OPS) {
    Object.defineProperty(ctx, name, {
      value: (...args: unknown[]) => {
        if (name === 'setLineDash') state.lineDash = [...(args[0] as number[])]
        const numbers = args.filter((arg): arg is number => typeof arg === 'number')
        if (name === 'fillText') record(name, { text: String(args[0]), x: Number(args[1]), y: Number(args[2]), args: numbers })
        else if (name === 'fillRect') record(name, { x: Number(args[0]), y: Number(args[1]), width: Number(args[2]), height: Number(args[3]), args: numbers })
        else if (name === 'moveTo' || name === 'lineTo') record(name, { x: Number(args[0]), y: Number(args[1]), args: numbers })
        else record(name, { args: numbers })
      },
    })
  }
  return ctx
}


/** A canvas with its recording context attached; structurally a real canvas. */
export type FakeCanvas = HTMLCanvasElement & { recording: RecordingContext }

export class FakeMutationObserver {
  public static instances: FakeMutationObserver[] = []
  private readonly callback: () => void
  public constructor(callback: () => void) {
    this.callback = callback
    FakeMutationObserver.instances.push(this)
  }
  public observe(): void { /* the instance list is the observable */ }
  public trigger(): void { this.callback() }
  public disconnect(): void { /* no-op */ }
  public static trigger(): void { for (const instance of FakeMutationObserver.instances) instance.callback() }
}

export interface CanvasOptions {
  tokens?: Record<string, string>
  dark?: boolean
  dpr?: number
  width?: number
  height?: number
  /** Text advance in px at a size; defaults to the layout estimate. */
  measure?: (text: string, sizePx: number) => number
}

/** A canvas whose 2D context records operations, themed by `tokens`. */
export function fakeCanvas(options: CanvasOptions = {}): FakeCanvas {
  const recording = recordingContext(options.measure)
  const size = { width: options.width ?? 800, height: options.height ?? 600 }
  const body = { dataset: (options.dark ? { dsDarkTheme: '' } : {}) as Record<string, string> }
  const canvas: Record<string, unknown> = {
    width: 0,
    height: 0,
    style: {} as Record<string, string>,
    ownerDocument: {
      body,
      defaultView: { devicePixelRatio: options.dpr ?? 1, MutationObserver: FakeMutationObserver },
    },
    recording,
    getContext: () => recording,
    getBoundingClientRect: () => ({ x: 0, y: 0, width: size.width, height: size.height, top: 0, left: 0, right: size.width, bottom: size.height }),
  }
  installTokens(options.tokens ?? {})
  return canvas as unknown as FakeCanvas
}

/** Install a `getComputedStyle` that answers from one token table. */
export function installTokens(tokens: Record<string, string>): void {
  ;(globalThis as unknown as { getComputedStyle: (el: unknown) => unknown }).getComputedStyle = () => ({
    getPropertyValue: (token: string) => tokens[token] ?? '',
  })
}

/** The host theme tokens the tests use; values come from the DSH token list. */
export const LIGHT_TOKENS: Record<string, string> = {
  '--dsw-alias-bg-base': '#ffffff',
  '--dsw-alias-bg-layer-2': '#f7f9f8',
  '--dsw-alias-bg-layer-3': '#eef2f0',
  '--dsw-alias-bg-overlay': '#ffffff',
  '--dsw-alias-label-primary': '#1c2324',
  '--dsw-alias-label-secondary': '#4a5a54',
  '--dsw-alias-label-tertiary': '#6b7d76',
  '--dsw-alias-border-l2': '#d6ded9',
  '--dsw-alias-border-l3': '#c2cdc7',
  '--dsw-alias-state-business-primary': '#8a6d00',
  '--dsw-alias-state-business-tertiary': '#fdf3cc',
  '--dsw-alias-state-success-primary': '#2f8a5f',
  '--dsw-alias-state-warn-primary': '#a8952c',
  '--dsw-alias-state-error-primary': '#b4231c',
  '--dsw-alias-state-done-secondary': '#cfe4d8',
  '--dsw-alias-interactive-bg-hover': '#e4ece8',
  '--dsw-alias-interactive-bg-active': '#d5e2db',
}

export const DARK_TOKENS: Record<string, string> = {
  '--dsw-alias-bg-base': '#161c20',
  '--dsw-alias-bg-layer-2': '#1a2428',
  '--dsw-alias-bg-layer-3': '#121618',
  '--dsw-alias-bg-overlay': '#121618',
  '--dsw-alias-label-primary': '#ecece4',
  '--dsw-alias-label-secondary': '#c7d6cc',
  '--dsw-alias-label-tertiary': '#9db8a8',
  '--dsw-alias-border-l2': '#2a3d36',
  '--dsw-alias-border-l3': '#364c42',
  '--dsw-alias-state-business-primary': '#e8c448',
  '--dsw-alias-state-business-tertiary': '#3a3320',
  '--dsw-alias-state-success-primary': '#5fb58a',
  '--dsw-alias-state-warn-primary': '#d4c05a',
  '--dsw-alias-state-error-primary': '#ff4030',
  '--dsw-alias-state-done-secondary': '#4a604e',
  '--dsw-alias-interactive-bg-hover': '#364c42',
  '--dsw-alias-interactive-bg-active': '#425a4c',
}
