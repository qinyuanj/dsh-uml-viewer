import { readFileSync } from 'node:fs'
import { defineConfig } from 'vitest/config'

// prompt.ts imports docs/companion-rules.md as text (#18); esbuild's text
// loader embeds it in the bundle. Vitest needs the same treatment.
export default defineConfig({
  plugins: [
    {
      name: 'markdown-as-text',
      enforce: 'pre',
      load(id) {
        const file = id.split('?')[0]
        if (!file.endsWith('.md')) return null
        return `export default ${JSON.stringify(readFileSync(file, 'utf8'))}`
      },
    },
  ],
})
