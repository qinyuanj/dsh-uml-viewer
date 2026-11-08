/**
 * Load the packaged Sarasa face once. The face is served from the plugin's
 * own host route (the package-assets route serves only image formats), and
 * is used so CJK advances match the layout estimate (`textWidth16`).
 */
import { PANEL_FONT_PATH } from '@dsh-uml-viewer/contracts'

export const FONT_FAMILY = 'UML Viewer Sarasa'

declare global {
  // FontFace lives in lib.dom; this declaration is only for TypeScript.
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
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      const fonts = (document as unknown as { fonts?: { add(face: FontFace): void } } | undefined)?.fonts
      fonts?.add(face)
    }
  })()
  // Swallow the rejection so a missing font does not break the panel; the
  // fallback estimate keeps boxes from overflowing.
  loaded.catch(() => {})
  return loaded
}

/** Apply the family to a canvas so `measureText` uses the same face. */
export function applyFont(ctx: CanvasRenderingContext2D, size: number, bold = false, italic = false): void {
  ctx.font = `${italic ? 'italic ' : ''}${bold ? 'bold ' : ''}${size}px "${FONT_FAMILY}", monospace`
}
