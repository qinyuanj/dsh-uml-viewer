declare module 'react' {
  /** The host injects React 18; this package only needs element creation. */
  export function createElement(type: unknown, props?: Record<string, unknown> | null, ...children: unknown[]): unknown
}
