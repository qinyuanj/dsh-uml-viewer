import { readdir, stat } from 'node:fs/promises'
import path from 'node:path'

/** Only inspect policy locations; never probe the legacy installation. */
export async function defaultPolicy(root: string): Promise<string> {
  const preferred = `examples/${path.basename(root)}.policy.edn`
  if (await isFile(path.join(root, preferred))) return preferred
  let entries: string[]
  try { entries = await readdir(path.join(root, 'examples')) }
  catch (error) {
    if ((error as NodeJS.ErrnoException).code !== 'ENOENT') throw error
    entries = []
  }
  for (const name of entries.filter(name => name.endsWith('.policy.edn')).sort()) {
    if (await isFile(path.join(root, 'examples', name))) return `examples/${name}`
  }
  return 'policy.edn'
}

async function isFile(file: string): Promise<boolean> {
  try { return (await stat(file)).isFile() }
  catch (error) {
    if ((error as NodeJS.ErrnoException).code === 'ENOENT') return false
    throw error
  }
}
