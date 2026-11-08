import { build } from 'esbuild'

// One published host entry. Workspace contracts are bundled; DSH owns Cordis.
await build({
  entryPoints: ['packages/host/src/index.ts'],
  outfile: 'lib/host.js',
  bundle: true,
  format: 'esm',
  platform: 'node',
  target: 'node22',
  external: ['@deepseek-ai/*'],
  sourcemap: true,
})
