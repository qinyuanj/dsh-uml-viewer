// Bounded gzip and ustar tar support shared by the release assembler
// (scripts/package.mjs) and the release verifier (scripts/release.mjs), so the
// bytes that will be published are parsed by the same module that wrote them.
// Node builtins only: no dependency, no external `tar`, no registry access, no
// process, no network.
//
// The reader fails closed. Every archive is capped in compressed size,
// decompressed size, entries and single-entry size, so a decompression bomb or
// an entry flood is refused before it can allocate; a malformed gzip stream,
// corrupt tar header checksum, duplicate or unsafe path, link, device or other
// non-regular entry is an error, never a skipped member.
import { gzipSync, gunzipSync } from 'node:zlib'
import { readFile, stat } from 'node:fs/promises'

const BLOCK = 512
const MIB = 1024 * 1024

// The real bundled payload is about 25 MiB (the Sarasa face), so these caps
// leave room for the native binary while bounding what a hostile or broken
// archive can make the verifier materialise.
export const archiveLimits = Object.freeze({
  compressedBytes: 128 * MIB,
  totalBytes: 256 * MIB,
  entryBytes: 128 * MIB,
  entries: 4096,
})

// Every released archive is one installable npm package below package/.
export const entryPrefix = 'package/'

// npm-compatible ustar with fixed metadata, sorted names and gzip timestamp 0.
// No npm lifecycle hooks, registry access, local dependency rewriting or host
// mtimes, so the same inputs produce the same bytes.
export function writeArchive(files) {
  const blocks = []
  for (const [name, bytes] of [...files].sort(([left], [right]) => left.localeCompare(right, 'en'))) {
    if (!Buffer.isBuffer(bytes)) throw new TypeError(`Archive member ${name} must be a Buffer`)
    const header = Buffer.alloc(BLOCK)
    const field = (offset, length, value) => header.write(value, offset, length, 'ascii')
    const octal = (offset, length, value) => field(offset, length, value.toString(8).padStart(length - 1, '0') + '\0')
    const filename = `${entryPrefix}${name}`
    if (Buffer.byteLength(filename) > 100) throw new Error(`ustar path exceeds 100 bytes: ${filename}`)
    field(0, 100, filename)
    octal(100, 8, name.startsWith('bin/') ? 0o755 : 0o644)
    octal(108, 8, 0); octal(116, 8, 0); octal(124, 12, bytes.length); octal(136, 12, 0)
    header.fill(32, 148, 156)
    field(156, 1, '0'); field(257, 6, 'ustar\0'); field(263, 2, '00')
    const checksum = header.reduce((sum, byte) => sum + byte, 0)
    field(148, 8, checksum.toString(8).padStart(6, '0') + '\0 ')
    blocks.push(header, bytes, Buffer.alloc((BLOCK - bytes.length % BLOCK) % BLOCK))
  }
  blocks.push(Buffer.alloc(2 * BLOCK))
  return gzipSync(Buffer.concat(blocks), { level: 9 })
}

const nonRegular = new Map([
  [0x31, 'a hard link'], [0x32, 'a symbolic link'], [0x33, 'a character device'],
  [0x34, 'a block device'], [0x35, 'a directory'], [0x36, 'a FIFO'],
  [0x37, 'a contiguous file'], [0x4b, 'a GNU long link record'],
  [0x4c, 'a GNU long name record'], [0x53, 'a sparse file'],
  [0x67, 'a global pax header'], [0x78, 'a pax header'],
])

function describeType(byte) {
  return nonRegular.get(byte) ?? `type ${JSON.stringify(String.fromCharCode(byte))}`
}

function blank(bytes) {
  return bytes.every(byte => byte === 0)
}

function octalField(field, label) {
  const text = field.toString('ascii').replace(/[\0 ]+$/u, '')
  if (!/^[0-7]{1,12}$/u.test(text)) throw new Error(`tar ${label} field ${JSON.stringify(text)} is not octal`)
  return Number.parseInt(text, 8)
}

function textField(field) {
  const end = field.indexOf(0)
  return field.subarray(0, end < 0 ? field.length : end).toString('utf8')
}

// A member name is relative, separator-clean and free of traversal, absolute or
// drive-letter paths; the archive layout is checked by the caller.
function safeName(name) {
  if (name === '') throw new Error('tar entry has an empty name')
  if (name.includes('\\')) throw new Error(`tar entry ${JSON.stringify(name)} contains a backslash`)
  if (name.startsWith('/') || /^[A-Za-z]:/u.test(name)) {
    throw new Error(`tar entry ${JSON.stringify(name)} is an absolute path`)
  }
  for (const segment of name.split('/')) {
    if (segment === '' || segment === '.' || segment === '..') {
      throw new Error(`tar entry ${JSON.stringify(name)} has an unsafe path segment ${JSON.stringify(segment)}`)
    }
  }
  return name
}

function headerSum(header) {
  let sum = 0
  for (let index = 0; index < BLOCK; index += 1) sum += index >= 148 && index < 156 ? 32 : header[index]
  return sum
}

function parseTar(raw, limits) {
  if (raw.length % BLOCK !== 0) throw new Error('tar data is not a whole number of 512-byte blocks')
  const entries = new Map()
  let total = 0
  let offset = 0
  while (offset + BLOCK <= raw.length && !blank(raw.subarray(offset, offset + BLOCK))) {
    const header = raw.subarray(offset, offset + BLOCK)
    const name = safeName(textField(header.subarray(0, 100)))
    const type = header[156]
    if (type !== 0x30 && type !== 0x00) throw new Error(`tar entry ${JSON.stringify(name)} is ${describeType(type)}, not a regular file`)
    if (textField(header.subarray(257, 263)) !== 'ustar' || header[262] !== 0) {
      throw new Error(`tar entry ${JSON.stringify(name)} is not ustar formatted`)
    }
    if (textField(header.subarray(263, 265)) !== '00') throw new Error(`tar entry ${JSON.stringify(name)} has an unexpected ustar version`)
    if (!blank(header.subarray(345, 500))) throw new Error(`tar entry ${JSON.stringify(name)} uses a tar path prefix`)
    const stored = octalField(header.subarray(148, 156), 'checksum')
    const computed = headerSum(header)
    if (stored !== computed) throw new Error(`tar entry ${JSON.stringify(name)} header checksum ${stored} does not match ${computed}`)
    if (entries.has(name)) throw new Error(`tar entry ${JSON.stringify(name)} appears more than once`)
    if (entries.size >= limits.entries) throw new Error(`tar data holds more than ${limits.entries} entries`)
    const size = octalField(header.subarray(124, 136), 'size')
    const mode = octalField(header.subarray(100, 108), 'mode')
    if (size > limits.entryBytes) throw new Error(`tar entry ${JSON.stringify(name)} is ${size} bytes, over the ${limits.entryBytes} byte entry limit`)
    total += size
    if (total > limits.totalBytes) throw new Error(`tar data expands to ${total} bytes, over the ${limits.totalBytes} byte decompressed limit`)
    const start = offset + BLOCK
    if (start + size > raw.length) throw new Error(`tar entry ${JSON.stringify(name)} is truncated`)
    entries.set(name, { name, size, mode, bytes: raw.subarray(start, start + size) })
    offset = start + Math.ceil(size / BLOCK) * BLOCK
  }
  if (raw.length - offset < 2 * BLOCK) throw new Error('tar data is not terminated by two zero blocks')
  if (!blank(raw.subarray(offset))) throw new Error('tar data has content after the end-of-archive marker')
  return entries
}

// Parses already-read archive bytes into a name-keyed Map of
// { name, size, mode, bytes } in archive order.
export function parseArchive(bytes, limits = archiveLimits) {
  if (!Buffer.isBuffer(bytes)) throw new TypeError('Archive data must be a Buffer')
  if (bytes.length > limits.compressedBytes) {
    throw new Error(`${bytes.length} bytes is over the ${limits.compressedBytes} byte compressed archive limit`)
  }
  if (bytes.length < 2 || bytes[0] !== 0x1f || bytes[1] !== 0x8b) {
    throw new Error('not gzip data; a release archive must be a real gzip-compressed tar, not plaintext named .tgz')
  }
  let raw
  try {
    raw = gunzipSync(bytes, { maxOutputLength: limits.totalBytes })
  } catch (error) {
    if (error?.code === 'ERR_BUFFER_TOO_LARGE') {
      throw new Error(`expands past the ${limits.totalBytes} byte decompressed archive limit`)
    }
    throw new Error(`gzip data is corrupt: ${error?.message ?? error}`)
  }
  return parseTar(raw, limits)
}

// Reads one release archive from disk, bounding the read itself before the
// whole file is materialised.
export async function readArchive(file, limits = archiveLimits) {
  const info = await stat(file).catch(() => null)
  if (!info?.isFile()) throw new Error(`${file}: release archive is not a regular file`)
  if (info.size > limits.compressedBytes) {
    throw new Error(`${file}: ${info.size} bytes is over the ${limits.compressedBytes} byte compressed archive limit`)
  }
  let bytes
  try {
    bytes = await readFile(file)
  } catch (error) {
    throw new Error(`${file}: release archive cannot be read: ${error.message}`)
  }
  try {
    return parseArchive(bytes, limits)
  } catch (error) {
    throw new Error(`${file}: ${error.message}`)
  }
}
