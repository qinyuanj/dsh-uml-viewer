import { PANEL_FONT_PATH } from '@dsh-uml-viewer/contracts'

export const FONT_FAMILY = 'UML Viewer Sarasa'

declare global {
  interface Window {
    FontFace: typeof FontFace
  }
}

let loaded: Promise<void> | null = null

export async function loadSarasa(): Promise<void> {
  if (loaded) return loaded
  const face = new FontFace(FONT_FAMILY, `url("${PANEL_FONT_PATH}")`, { style: 'normal', weight: '400' })
  loaded = (async () => {
    await face.load()
    if (typeof document !== 'undefined') {
      const fonts = (document as unknown as { fonts?: { add(face: FontFace): void } } | undefined)?.fonts
      fonts?.add(face)
    }
  })()
  loaded.catch(() => {})
  return loaded
}

export function applyFont(ctx: CanvasRenderingContext2D, size: number, bold = false, italic = false): void {
  ctx.font = `${italic ? 'italic ' : ''}${bold ? 'bold ' : ''}${size}px "${FONT_FAMILY}", monospace`
}
