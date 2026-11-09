import fs from 'node:fs/promises'
import path from 'node:path'

/**
 * Small, distinct TypeScript projects. The scripted agent runs the real
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
  projects.atlas = await makeAtlas(home)
  projects.nexus = await makeNexus(home)
  return projects
}

/** Six scanned imports reclassified by policy; no hand-authored IR or frame (#25). */
async function makeNexus(home) {
  const root = path.join(home, 'projects', 'nexus')
  await fs.mkdir(path.join(root, 'src'), { recursive: true })
  const kinds = ['association', 'dependency', 'inheritance', 'implements', 'aggregation', 'composition']
  await fs.writeFile(path.join(root, 'policy.edn'), `{:hierarchical true
 :title "Nexus"
 :prefix "nexus"
 :sources [{:lang :typescript :root "src" :prefix "nexus"}]
 :levels [[association dependency inheritance implements aggregation composition] [hub]]
 :edge-kinds {${kinds.map(kind => `[:hub :${kind}] :${kind}`).join('\n              ')}}
 :out "diagram.edn"}\n`)
  for (const kind of kinds) {
    await fs.writeFile(path.join(root, 'src', `${kind}.ts`),
      (kind === 'dependency' ? "import { Hub } from './hub.js'\n" : '') + 'export class Target {}\n')
  }
  await fs.writeFile(path.join(root, 'src', 'observer.ts'), 'export class Observer {}\n')
  await fs.writeFile(path.join(root, 'src', 'hub.ts'), kinds.map((kind, i) =>
    `import { Target as T${i} } from './${kind}.js'`).join('\n') + '\nexport class Hub {}\n')
  return root
}

/**
 * The panel-primitives sample (#24): a flat namespace so the root view shows
 * the classes themselves: an interface (I marker, «interface»), a CJK-named
 * module, two architecture levels, and `react` as a foreign package
 * (外来包椭圆). No metrics, so every box is unmeasured (dashed, 无数据).
 */
async function makeAtlas(home) {
  const root = path.join(home, 'projects', 'atlas')
  await fs.mkdir(path.join(root, 'src'), { recursive: true })
  await fs.writeFile(path.join(root, 'policy.edn'), `{:hierarchical true
 :title "Atlas"
 :prefix "atlas"
 :sources [{:lang :typescript :root "src" :prefix "atlas"}]
 :foreign [react]
 :levels [[priced] [gateway]]
 :order [gateway priced]
 :out "diagram.edn"}\n`)
  await fs.writeFile(path.join(root, 'src', 'priced.ts'), 'export interface Priced {\n  price(): number\n}\n')
  await fs.writeFile(path.join(root, 'src', '订单.ts'),
    "import { Priced } from './priced.js'\nexport class 订单服务 implements Priced {\n  price(): number { return 42 }\n}\n")
  await fs.writeFile(path.join(root, 'src', 'gateway.ts'), [
    "import { useState } from 'react'",
    "import { 订单服务 } from './订单.js'",
    "import { Priced } from './priced.js'",
    'export class Gateway {',
    '  service: Priced = new 订单服务()',
    '  state = useState(0)',
    '}',
    '',
  ].join('\n'))
  return root
}
