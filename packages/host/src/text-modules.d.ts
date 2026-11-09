/** Markdown documents bundled as text (`loader: { '.md': 'text' }` in scripts/build-plugin.mjs). */
declare module '*.md' {
  const text: string
  export default text
}
