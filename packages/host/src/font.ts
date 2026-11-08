import { readFile } from 'node:fs/promises'
import path from 'node:path'
import { PANEL_FONT_PATH } from '@dsh-uml-viewer/contracts'
import type { HostServices } from './dsh.js'

export const FONT_MEDIA_TYPE = 'font/ttf'
/** The face the installer provisions: Sarasa Mono SC Regular 1.0.42. */
export const FONT_FILE = 'SarasaMonoSC-Regular.ttf'

/** The module's bundled browser half loads this family so CJK advances match the layout estimate. */
export const FONT_URL = PANEL_FONT_PATH

/**
 * Serve the packaged Sarasa face over HTTP. `dsh-client-modules` only serves
 * `.svg/.png/.webp/.jpg/.jpeg` package assets, so a font needs its own route.
 *
 * @param services the host services (a profile without a web server serves nothing).
 * @param packageRoot the package directory holding `fonts/`.
 * @returns the route disposer, or `undefined` when no web server is present.
 */
export function registerFontRoute(services: HostServices, packageRoot: string): (() => void) | undefined {
  if (!services.webServer) return undefined
  const file = path.join(packageRoot, 'fonts', FONT_FILE)
  return services.webServer.register({
    kind: 'exact',
    path: PANEL_FONT_PATH,
    async resource(): Promise<Response> {
      try {
        const bytes = await readFile(file)
        return new Response(bytes, {
          status: 200,
          headers: {
            'content-type': FONT_MEDIA_TYPE,
            'content-length': String(bytes.byteLength),
            'cache-control': 'public, max-age=31536000, immutable',
            'x-content-type-options': 'nosniff',
          },
        })
      } catch {
        return new Response('the packaged Sarasa face is missing', { status: 404 })
      }
    },
  })
}
