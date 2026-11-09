import { createHash, randomUUID } from 'node:crypto'
import {
  chmodSync, closeSync, copyFileSync, mkdirSync, openSync, readdirSync, readFileSync, renameSync, rmdirSync, rmSync, statSync, unlinkSync,
  utimesSync, writeFileSync,
} from 'node:fs'
import path from 'node:path'

/**
 * #32: packaged binaries live in `<home>/<version>/`, one directory per
 * plugin version, published atomically and never rewritten in place. A host
 * activation holds a lease on its version for as long as it may start
 * residents; cleanup removes only versions that are neither current nor
 * leased, and defers (never fails) when the OS still holds one in use.
 *
 * Layout under the plugin home:
 *   <version>/<exe>                 the pinned executable
 *   current.json                    the version the latest activation pinned
 *   leases/<version>/<id>.lease     one per live host activation (mtime heartbeat)
 *   .staging-<uuid>/                an in-progress copy (never used directly)
 *   .trash-<uuid>/                  a removed version whose deletion was deferred
 */

export const VERSION_PATTERN = /^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?$/
/** A lease not renewed for this long belongs to a host that is gone. */
export const LEASE_TTL_MS = 10 * 60_000
const HEARTBEAT_MS = 60_000

export interface PinnedVersion {
  /** The executable inside the version directory. */
  readonly binary: string
  /** Hold this version until the returned disposer is called (e.g. child exit). */
  retain(): () => void
  /** Release this activation's lease, then remove versions nobody uses. */
  release(): void
}

function once(fn: () => void): () => void {
  let called = false
  return () => {
    if (called) return
    called = true
    fn()
  }
}

interface Digest { size: number; sha256: string }

function digest(file: string): Digest {
  const bytes = readFileSync(file)
  return { size: bytes.length, sha256: createHash('sha256').update(bytes).digest('hex') }
}

function sameDigest(file: string, expected: Digest): boolean {
  try {
    if (statSync(file).size !== expected.size) return false
    return digest(file).sha256 === expected.sha256
  } catch {
    return false
  }
}

const errorCode = (error: unknown) => (error as NodeJS.ErrnoException | undefined)?.code

/** Windows reports a file another process is replacing or holding this way. */
const BUSY = ['EPERM', 'EACCES', 'EBUSY']

function pause(ms: number): void {
  Atomics.wait(new Int32Array(new SharedArrayBuffer(4)), 0, 0, ms)
}

/**
 * Write a small file atomically: a crash leaves the old content or the new
 * one. Windows refuses a rename onto a file that another host is renaming
 * onto at the same moment; that is retried briefly.
 */
function writeAtomic(file: string, text: string): void {
  const temporary = `${file}.${process.pid}.${randomUUID()}.tmp`
  writeFileSync(temporary, text)
  try {
    for (let attempt = 0; ; attempt += 1) {
      try {
        renameSync(temporary, file)
        return
      } catch (error) {
        if (attempt >= 50 || !BUSY.includes(errorCode(error) ?? '')) throw error
        pause(10 + Math.floor(Math.random() * 20))
      }
    }
  } finally {
    rmSync(temporary, { force: true })
  }
}

/**
 * Whether Windows still runs (or holds open) a file in `directory`. A running
 * exe can be renamed on Windows but not opened for writing, so this probe is
 * what keeps cleanup from moving the image of a live resident.
 */
function heldOnWindows(directory: string, platform: string): boolean {
  if (platform !== 'win32') return false
  let names: string[]
  try {
    names = readdirSync(directory)
  } catch {
    return false
  }
  for (const name of names) {
    let descriptor: number | undefined
    try {
      descriptor = openSync(path.join(directory, name), 'r+')
    } catch (error) {
      if (BUSY.includes(errorCode(error) ?? '')) return true
    } finally {
      if (descriptor !== undefined) closeSync(descriptor)
    }
  }
  return false
}

function removeQuietly(target: string): boolean {
  try {
    rmSync(target, { recursive: true, force: true })
    return true
  } catch {
    return false
  }
}

/**
 * Probe only the owner named by this lease; signal 0 never sends a signal.
 * No process enumeration, command-line matching, or termination. PID reuse
 * conservatively retains a directory. A suspended owner's old heartbeat
 * must never grant permission to delete its executable.
 */
function ownerMayBeAlive(file: string): boolean {
  const record = JSON.parse(readFileSync(file, 'utf8')) as { pid?: unknown }
  if (!Number.isSafeInteger(record.pid) || (record.pid as number) <= 0) return false
  try {
    process.kill(record.pid as number, 0)
    return true
  } catch (error) {
    return errorCode(error) !== 'ESRCH'
  }
}

/** Serialize publication and retirement across hosts, so cleanup cannot race a spawn's pin. */
function withStoreLock<T>(home: string, run: () => T): T {
  const lock = path.join(home, '.store-lock')
  const owner = path.join(lock, 'owner.json')
  const deadline = Date.now() + 10_000
  for (;;) {
    let acquired = false
    try {
      mkdirSync(lock)
      acquired = true
    } catch (error) {
      if (errorCode(error) !== 'EEXIST') throw error
    }
    if (acquired) {
      try {
        writeFileSync(owner, JSON.stringify({ pid: process.pid }))
        return run()
      } finally {
        removeQuietly(lock)
      }
    }
    try {
      // Only inspect the lock's named owner. A reused PID is conservative.
      if (!ownerMayBeAlive(owner)) {
        removeQuietly(lock)
        continue
      }
    } catch (error) {
      // Between mkdir and writing owner.json, or a host crashed there. The
      // latter can be reclaimed after the normal lease grace period.
      try {
        if (errorCode(error) === 'ENOENT' && Date.now() - statSync(lock).mtimeMs > LEASE_TTL_MS) {
          removeQuietly(lock)
          continue
        }
      } catch {
        // Lock changed while inspecting it; retry.
      }
    }
    if (Date.now() >= deadline) throw new Error('uml-viewer version store is busy; reload the plugin')
    pause(10 + Math.floor(Math.random() * 20))
  }
}

/** Live leases on `version`, other than `own`. Stale ones are removed on the way. */
function liveLeases(home: string, version: string, own?: string, now = Date.now()): number {
  const directory = path.join(home, 'leases', version)
  let names: string[]
  try {
    names = readdirSync(directory)
  } catch {
    return 0
  }
  let live = 0
  for (const name of names) {
    const file = path.join(directory, name)
    if (file === own) continue
    try {
      if (now - statSync(file).mtimeMs < LEASE_TTL_MS || ownerMayBeAlive(file)) {
        live += 1
        continue
      }
      unlinkSync(file)
    } catch (error) {
      // Only an entry that vanished is known unused. Any other inability to
      // inspect/remove a lease is conservative: keep the version.
      if (errorCode(error) !== 'ENOENT') live += 1
    }
  }
  if (live === 0 && own === undefined) {
    try {
      // Only an empty directory goes; a lease written meanwhile keeps it.
      rmdirSync(directory)
    } catch {
      // Not empty, already gone, or a directory in use.
    }
  }
  return live
}

function acquireLease(home: string, version: string): { file: string; stop(): void } {
  const directory = path.join(home, 'leases', version)
  mkdirSync(directory, { recursive: true })
  const file = path.join(directory, `${process.pid}-${randomUUID()}.lease`)
  writeFileSync(file, `${JSON.stringify({ pid: process.pid, version, since: new Date().toISOString() })}\n`)
  const timer = setInterval(() => {
    const now = new Date()
    try {
      utimesSync(file, now, now)
    } catch {
      // A lease removed as stale (host suspended past the TTL) is written again.
      try {
        mkdirSync(directory, { recursive: true })
        writeFileSync(file, `${JSON.stringify({ pid: process.pid, version, since: now.toISOString() })}\n`)
      } catch {
        // Best effort; the next heartbeat retries.
      }
    }
  }, HEARTBEAT_MS)
  timer.unref()
  return {
    file,
    stop() {
      clearInterval(timer)
      try {
        unlinkSync(file)
      } catch {
        // Already gone.
      }
    },
  }
}

/** Copy `source` into a private staging directory and verify the copy. */
function stage(home: string, source: string, name: string, expected: Digest, platform: string): string {
  const staging = path.join(home, `.staging-${randomUUID()}`)
  mkdirSync(staging)
  try {
    const binary = path.join(staging, name)
    copyFileSync(source, binary)
    if (platform !== 'win32') chmodSync(binary, 0o755)
    if (!sameDigest(binary, expected)) throw new Error('the copied executable does not match the installed payload')
    return staging
  } catch (error) {
    removeQuietly(staging)
    throw error
  }
}

/**
 * Publish `source` as `<home>/<version>/<name>` and lease it for this
 * activation. An existing version directory is reused only when its bytes
 * match the payload; one with other bytes is replaced only when no other
 * host leases it and the OS lets it move, otherwise activation refuses.
 */
export function pinVersion(home: string, version: string, source: string, name: string,
  platform: string = process.platform): PinnedVersion {
  if (!VERSION_PATTERN.test(version)) throw new Error(`Invalid uml-viewer version directory name: ${version}`)
  mkdirSync(home, { recursive: true })
  return withStoreLock(home, () => pinLocked(home, version, source, name, platform))
}

function pinLocked(home: string, version: string, source: string, name: string, platform: string): PinnedVersion {
  // Lease before looking: cleanup re-checks leases after it moved a directory
  // away, so either it sees this lease and restores, or this sees it gone.
  const lease = acquireLease(home, version)
  try {
    const expected = digest(source)
    const directory = path.join(home, version)
    const binary = path.join(directory, name)
    if (!sameDigest(binary, expected)) publish(home, version, directory, binary, source, name, expected, lease.file, platform)
    writeAtomic(path.join(home, 'current.json'), `${JSON.stringify({ version, ...expected })}\n`)
    cleanupUnlocked(home, version)
    let holders = 1
    const drop = (): void => {
      holders -= 1
      if (holders === 0) {
        lease.stop()
        cleanupVersions(home)
      }
    }
    return {
      binary,
      retain() {
        if (holders === 0) throw new Error('This uml-viewer activation has ended; open the panel again')
        holders += 1
        return once(drop)
      },
      release: once(drop),
    }
  } catch (error) {
    lease.stop()
    throw error
  }
}

function publish(home: string, version: string, directory: string, binary: string, source: string, name: string,
  expected: Digest, ownLease: string, platform: string): void {
  const staging = stage(home, source, name, expected, platform)
  try {
    for (let attempt = 0; attempt < 3; attempt += 1) {
      try {
        renameSync(staging, directory)
        return
      } catch (error) {
        if (!['EEXIST', 'ENOTEMPTY', 'EPERM', 'EACCES', 'EBUSY'].includes(errorCode(error) ?? '')) throw error
      }
      // Another activation published first: its complete copy is as good as ours.
      if (sameDigest(binary, expected)) return
      if (!statExists(directory)) continue
      // Same version, other bytes. Never write into it; move it away only when
      // no other host leases it and the OS agrees nothing runs from it.
      if (liveLeases(home, version, ownLease) > 0 || heldOnWindows(directory, platform)) {
        throw new Error(`uml-viewer ${version} is already pinned with different bytes and is in use by another DSH host; close its viewer panels or restart DSH, then reload the plugin`)
      }
      const trash = path.join(home, `.trash-${randomUUID()}`)
      try {
        renameSync(directory, trash)
      } catch (error) {
        throw new Error(`uml-viewer ${version} is already pinned with different bytes and cannot be replaced while in use (${errorCode(error) ?? String(error)}); close its viewer panels or restart DSH, then reload the plugin`)
      }
      removeQuietly(trash)
    }
    throw new Error(`Cannot publish uml-viewer ${version} under ${home}: the version directory kept changing`)
  } finally {
    removeQuietly(staging)
  }
}

function statExists(target: string): boolean {
  try {
    statSync(target)
    return true
  } catch {
    return false
  }
}

function currentVersion(home: string): string | null {
  try {
    const record = JSON.parse(readFileSync(path.join(home, 'current.json'), 'utf8')) as { version?: unknown }
    return typeof record.version === 'string' ? record.version : null
  } catch {
    return null
  }
}

/**
 * Remove every version directory that is neither the current version, nor
 * `keep`, nor leased by a live host. Never throws: a directory the OS still
 * holds (a running exe on Windows) stays until a later cleanup.
 */
export function cleanupVersions(home: string, keep?: string): void {
  try {
    withStoreLock(home, () => cleanupUnlocked(home, keep))
  } catch {
    // A busy store is retried by the next activation or lease release.
  }
}

function cleanupUnlocked(home: string, keep?: string): void {
  let names: string[]
  try {
    names = readdirSync(home)
  } catch {
    return
  }
  const current = currentVersion(home)
  const now = Date.now()
  for (const name of names) {
    const entry = path.join(home, name)
    if (name.startsWith('.trash-')) {
      removeQuietly(entry)
      continue
    }
    if (name.startsWith('.staging-')) {
      // An activation that crashed mid-copy; a live one finishes in seconds.
      try {
        if (now - statSync(entry).mtimeMs > LEASE_TTL_MS) removeQuietly(entry)
      } catch {
        // Gone meanwhile.
      }
      continue
    }
    if (!VERSION_PATTERN.test(name) || name === current || name === keep) continue
    try {
      if (!statSync(entry).isDirectory()) continue
    } catch {
      continue
    }
    if (liveLeases(home, name, undefined, now) > 0 || heldOnWindows(entry, process.platform)) continue
    retire(home, entry, () => liveLeases(home, name) > 0 || heldOnWindows(entry, process.platform))
  }
}

/** Move `entry` aside, give it back if it became used meanwhile, else delete it. */
function retire(home: string, entry: string, inUse: () => boolean): void {
  const trash = path.join(home, `.trash-${randomUUID()}`)
  try {
    renameSync(entry, trash)
  } catch {
    // Windows: an exe running from it (or an open handle) blocks the move. Deferred.
    return
  }
  if (inUse()) {
    try {
      renameSync(trash, entry)
      return
    } catch {
      // An activation already republished a verified copy there.
    }
  }
  removeQuietly(trash)
}
