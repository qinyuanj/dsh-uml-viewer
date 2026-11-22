import { build } from 'esbuild'

await build({
  entryPoints: ['packages/host/src/index.ts'],
  outfile: 'lib/host.js',
  bundle: true,
  format: 'esm',
  platform: 'node',
  target: 'node22',
  external: ['@deepseek-ai/*'],
  loader: { '.md': 'text' },
  sourcemap: true,
})

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
