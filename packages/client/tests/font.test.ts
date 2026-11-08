import { describe, expect, it } from 'vitest'
import { PANEL_FONT_PATH } from '@dsh-uml-viewer/contracts'
import { FONT_FAMILY, loadSarasa } from '../src/font.js'

interface LoadedFace {
  family: string
  source: string
  load(): Promise<unknown>
}

describe('packaged Sarasa face', () => {
  it('loads the face the host serves and registers it for the panel canvas', async () => {
    const faces: LoadedFace[] = []
    const registered: LoadedFace[] = []
    ;(globalThis as unknown as { FontFace: unknown }).FontFace = class {
      public readonly family: string
      public readonly source: string
      public constructor(family: string, source: string) {
        this.family = family
        this.source = source
        faces.push(this as unknown as LoadedFace)
      }
      public async load(): Promise<unknown> {
        this.loaded = true
        return this
      }
      public loaded = false
    }
    const document = { fonts: { add: (face: LoadedFace) => registered.push(face) } }
    ;(globalThis as unknown as { document: unknown }).document = document

    await loadSarasa()
    expect(faces).toHaveLength(1)
    expect(faces[0]!.family).toBe(FONT_FAMILY)
    expect(faces[0]!.source).toBe(`url("${PANEL_FONT_PATH}")`)
    expect(registered).toEqual(faces)
    // The route is the plugin's own font route, not the image-only asset route.
    expect(PANEL_FONT_PATH).toBe('/dsh-uml-viewer/font/SarasaMonoSC-Regular.ttf')

    // Loading twice reuses the same face.
    await loadSarasa()
    expect(faces).toHaveLength(1)
  })
})
