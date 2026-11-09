import { readFile, writeFile, readdir } from 'node:fs/promises'
import path from 'node:path'
import { pathToFileURL } from 'node:url'

export const root = path.resolve(import.meta.dirname, '..')
export async function releaseVersion(directory = root) {
  const cargo = await readFile(path.join(directory, 'Cargo.toml'), 'utf8')
  const section = cargo.split('[workspace.package]')[1]?.split(/\r?\n\[/)[0]
  const version = section?.match(/^version\s*=\s*"([^"]+)"/m)?.[1]
  const match = version?.match(/^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(?:-([0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*))?(?:\+([0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*))?$/)
  const invalidNumericPrerelease = match?.[4]?.split('.').some(part => /^0\d+$/.test(part))
  if (!match || invalidNumericPrerelease) {
    throw new Error('Cargo.toml workspace.package.version must be a SemVer')
  }
  return version
}
export async function checkVersions(write = false) {
  const version = await releaseVersion()
  const manifests = ['package.json', 'dsh-plugin.json']
  for (const entry of await readdir(path.join(root, 'packages'), { withFileTypes: true })) {
    if (entry.isDirectory()) manifests.push(`packages/${entry.name}/package.json`)
  }
  for (const file of manifests) {
    const absolute = path.join(root, file)
    const json = JSON.parse(await readFile(absolute, 'utf8'))
    if (write) {
      json.version = version
      await writeFile(absolute, JSON.stringify(json, null, 2) + '\n')
    } else if (json.version !== version) throw new Error(`${file}: ${json.version} != Cargo workspace ${version}; run pnpm version:sync`)
  }
  for (const entry of await readdir(path.join(root, 'crates'), { withFileTypes: true })) {
    if (!entry.isDirectory()) continue
    const file = `crates/${entry.name}/Cargo.toml`
    const cargo = await readFile(path.join(root, file), 'utf8')
    if (!/^version\.workspace\s*=\s*true$/m.test(cargo)) throw new Error(`${file} must inherit version.workspace = true`)
  }
  return version
}
if (process.argv[1] && import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href) {
  console.log(`Release version ${await checkVersions(process.argv.includes('--write'))}: Cargo/npm manifests consistent`)
}
