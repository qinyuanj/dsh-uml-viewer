declare module 'react' {
  /** React 18 is injected by the host, never bundled. */
  export function useCallback<T>(callback: T, deps: readonly unknown[]): T
  export function useRef<T>(initial: T): { current: T }
  export function useState<T>(initial: T): [T, (value: T) => void]
  export function useEffect(effect: () => void | (() => void), deps: readonly unknown[]): void
  export function createElement(type: unknown, props?: Record<string, unknown> | null, ...children: unknown[]): unknown
}
