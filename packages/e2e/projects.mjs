import fs from 'node:fs/promises'
import path from 'node:path'

/**
 * Two small, distinct TypeScript projects. The scripted agent runs the real
 * binary's `ir` subcommand in each one; no synthetic SceneFrame is injected.
 */
export async function makeProjects(home) {
  const projects = {}
  for (const name of ['shop', 'ledger']) {
    const title = name === 'shop' ? 'Shop' : 'Ledger'
    const root = path.join(home, 'projects', name)
    await fs.mkdir(path.join(root, 'src'), { recursive: true })
    const policy = `{:hierarchical true
 :title "${title}"
 :prefix "${name}"
 :sources [{:lang :typescript :root "src" :prefix "${name}"}]
 :levels [[orders] [api]]
 :order [api orders]
 :proposals [{:id :p-${name} :name "${title} proposal"
              :layers [{:id :domain :label "Domain" :nses [orders]}] :omit []}]
 :out "diagram.edn"}\n`
    await fs.writeFile(path.join(root, 'policy.edn'), policy)
    await fs.writeFile(path.join(root, 'src', 'orders.ts'), `export class Order {\n  total(): number { return 42 }\n}\n`)
    await fs.writeFile(path.join(root, 'src', 'api.ts'), `import { Order } from './orders.js'\nexport class Api {\n  order = new Order()\n}\n`)
    projects[name] = root
  }
  return projects
}
