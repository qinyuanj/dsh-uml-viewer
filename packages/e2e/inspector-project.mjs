import fs from 'node:fs/promises'
import path from 'node:path'

export const TARGET_LINE = 82
export const TARGET_TEXT = 'export function target(): string { return "目标 " + companion() }'

/** Real scanned modules for #27; top-level exports also match source lookup. */
export async function makeInspector(home) {
  const root = path.join(home, 'projects', 'inspector')
  await fs.mkdir(path.join(root, 'src'), { recursive: true })
  await fs.writeFile(path.join(root, 'policy.edn'), `{:hierarchical true
 :title "Inspector"
 :prefix "inspector"
 :sources [{:lang :typescript :root "src" :prefix "inspector"}]
 :levels [[companion] [catalog]]
 :order [catalog companion]
 :proposals [{:id :p-inspector :name "Inspector proposal"
              :layers [{:id :catalogue :label "Catalogue" :nses [catalog companion]}] :omit []}]
 :out "diagram.edn"}
`)
  const lines = ["import { companion } from './companion.js'"]
  for (let n = 0; n < 60; n += 1) {
    lines.push(`export function op${String(n).padStart(2, '0')}(): number { return ${n} }`)
  }
  while (lines.length < TARGET_LINE - 1) lines.push(`// 填充行 ${lines.length + 1}`)
  lines.push(TARGET_TEXT, '// 文件末尾', '')
  await fs.writeFile(path.join(root, 'src', 'catalog.ts'), lines.join('\n'))
  await fs.writeFile(path.join(root, 'src', 'companion.ts'), 'export function companion(): string { return "second file" }\n')
  return root
}
