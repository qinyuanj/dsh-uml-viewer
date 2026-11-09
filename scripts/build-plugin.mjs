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
  // docs/companion-rules.md is the prompt section's only source (#18).
  loader: { '.md': 'text' },
  sourcemap: true,
})

// The browser half. React comes from the host's platform module table, so it
// is an external `require`, never embedded (DECISIONS.md D1). The output is the
// closure-factory handoff `dsh.client` packages must use.
await build({
  entryPoints: ['packages/client/src/index.ts'],
  outfile: 'lib/client.js',
  bundle: true,
  format: 'cjs',
  platform: 'browser',
  target: 'es2022',
  minify: true,
  external: ['react', 'react/jsx-runtime'],
  define: { 'process.env.NODE_ENV': '"production"' },
  banner: {
    js: 'window.__ModuleLoader__.load({ id: "dsh-uml-viewer", factory: (require) => {'
      + 'var module = { exports: {} }; var exports = module.exports;',
  },
  footer: { js: 'return module.exports; } });' },
})
