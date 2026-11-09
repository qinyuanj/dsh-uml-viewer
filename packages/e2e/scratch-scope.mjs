import fs from 'node:fs'
import path from 'node:path'

const BARRIER = 'dsh-uml-viewer-e2e-scratch'

/**
 * Give the scratch directory its own package scope before any run home is
 * created below it. By default the scratch lives in the checkout, whose root
 * manifest is the dsh-uml-viewer package with an `exports` map. A user's DSH
 * home never sits under such a package. Without this scope, every lookup DSH
 * continues above a profile (`<home>/profiles/package.json`), and DSH's own
 * self-reference check, resolves `dsh-uml-viewer` to the checkout's build
 * (CI14). With it, those lookups miss, the same way they miss for a user.
 * The scope has no name match and no `exports`, so it neither shadows nor
 * provides a package.
 */
export function isolateScratch(scratch) {
  fs.mkdirSync(scratch, { recursive: true })
  const manifest = path.join(scratch, 'package.json')
  if (fs.existsSync(manifest)) {
    const existing = JSON.parse(fs.readFileSync(manifest, 'utf8'))
    if (existing.name === 'dsh-uml-viewer') throw new Error(`The e2e scratch ${scratch} must not be the dsh-uml-viewer package`)
    if (existing.name === BARRIER) return
    throw new Error(`The e2e scratch ${scratch} already belongs to package ${JSON.stringify(existing.name)}`)
  }
  fs.writeFileSync(manifest, `${JSON.stringify({
    name: BARRIER,
    private: true,
    description: 'Package scope that keeps scratch DSH homes out of the dsh-uml-viewer checkout (docs/e2e-harness.md).',
  }, null, 2)}\n`)
}
