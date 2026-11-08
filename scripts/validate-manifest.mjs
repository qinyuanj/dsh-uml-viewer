import { existsSync, readFileSync } from 'node:fs'
import { parseManifest } from '@dsh-std/manifest'

const pkg = JSON.parse(readFileSync('package.json', 'utf8'))
const text = readFileSync('dsh-plugin.json', 'utf8')
const manifest = parseManifest(text, { source: 'dsh-plugin.json' })
const equal = (left, right, label) => {
  if (left !== right) throw new Error(`${label}: manifest ${JSON.stringify(left)} != package.json ${JSON.stringify(right)}`)
}
equal(manifest.name, pkg.name, 'name')
equal(manifest.version, pkg.version, 'version')
equal(manifest.license, pkg.license, 'license')
const entry = manifest.facets.host.entry
if (entry !== pkg.main || !existsSync(entry)) throw new Error(`host entry must be the built ${pkg.main}`)
for (const resource of [pkg.dsh.bundle.patch, pkg.icon, 'locale/en.json', 'locale/zh.json']) {
  if (!existsSync(resource)) throw new Error(`missing package resource: ${resource}`)
}
console.log('dsh-plugin.json and package.json are consistent Community 0.15 development manifests')
