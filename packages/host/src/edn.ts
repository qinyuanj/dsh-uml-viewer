/**
 * A small EDN reader for the IR `uml-viewer ir` writes: maps, vectors, lists,
 * sets, strings, keywords, symbols, numbers, characters, `nil`/`true`/`false`,
 * `;` comments, `#_` discards and tagged literals (the tag is dropped). The
 * host only reads ids and `:file` paths from it; the Rust IR stays the owner
 * of the format.
 */
export type Edn =
  | null | boolean | number | string
  | { readonly keyword: string }
  | { readonly symbol: string }
  | readonly Edn[]
  | EdnMap

export class EdnMap {
  constructor(readonly entries: readonly (readonly [Edn, Edn])[]) {}

  /** The value under keyword `:name`, if any. */
  get(name: string): Edn | undefined {
    for (const [key, value] of this.entries) {
      if (key !== null && typeof key === 'object' && 'keyword' in key && key.keyword === name) return value
    }
    return undefined
  }
}

const DELIMITERS = new Set(['(', ')', '[', ']', '{', '}', '"', ';'])
const ESCAPES: Record<string, string> = { n: '\n', t: '\t', r: '\r', b: '\b', f: '\f', '"': '"', '\\': '\\' }

class Reader {
  private index = 0
  constructor(private readonly text: string) {}

  private fail(message: string): never {
    throw new Error(`EDN: ${message} at offset ${this.index}`)
  }

  private skip(): void {
    for (;;) {
      const ch = this.text[this.index]
      if (ch === undefined) return
      if (ch === ';') {
        while (this.index < this.text.length && this.text[this.index] !== '\n') this.index += 1
      } else if (/\s/.test(ch) || ch === ',') {
        this.index += 1
      } else if (ch === '#' && this.text[this.index + 1] === '_') {
        this.index += 2
        this.read()
      } else {
        return
      }
    }
  }

  /** The next form; throws at the end of input. */
  read(): Edn {
    this.skip()
    const ch = this.text[this.index]
    if (ch === undefined) this.fail('unexpected end of input')
    switch (ch) {
      case '{': {
        this.index += 1
        const items = this.until('}')
        if (items.length % 2) this.fail('map with an odd number of forms')
        const entries: [Edn, Edn][] = []
        for (let i = 0; i < items.length; i += 2) entries.push([items[i]!, items[i + 1]!])
        return new EdnMap(entries)
      }
      case '[': case '(':
        this.index += 1
        return this.until(ch === '[' ? ']' : ')')
      case ')': case ']': case '}':
        return this.fail(`unexpected ${ch}`)
      case '"':
        return this.string()
      case '\\': {
        // A character literal (\a, \newline): only its name is kept.
        this.index += 1
        return this.token()
      }
      case '#': {
        if (this.text[this.index + 1] === '{') {
          this.index += 2
          return this.until('}')
        }
        this.index += 1
        this.token()
        return this.read()
      }
      default:
        return this.atom(this.token())
    }
  }

  private until(close: string): Edn[] {
    const items: Edn[] = []
    for (;;) {
      this.skip()
      if (this.index >= this.text.length) this.fail(`missing ${close}`)
      if (this.text[this.index] === close) {
        this.index += 1
        return items
      }
      items.push(this.read())
    }
  }

  private string(): string {
    this.index += 1
    let out = ''
    for (;;) {
      const ch = this.text[this.index]
      if (ch === undefined) this.fail('unterminated string')
      this.index += 1
      if (ch === '"') return out
      if (ch !== '\\') {
        out += ch
        continue
      }
      const next = this.text[this.index]
      if (next === undefined) this.fail('unterminated escape')
      this.index += 1
      if (next === 'u') {
        const hex = this.text.slice(this.index, this.index + 4)
        if (!/^[0-9a-fA-F]{4}$/.test(hex)) this.fail('bad \\u escape')
        this.index += 4
        out += String.fromCharCode(Number.parseInt(hex, 16))
      } else if (next in ESCAPES) {
        out += ESCAPES[next]
      } else {
        this.fail(`unknown escape \\${next}`)
      }
    }
  }

  private token(): string {
    const start = this.index
    while (this.index < this.text.length) {
      const ch = this.text[this.index]!
      if (/\s/.test(ch) || ch === ',' || DELIMITERS.has(ch)) break
      this.index += 1
    }
    if (this.index === start) this.fail('empty token')
    return this.text.slice(start, this.index)
  }

  private atom(token: string): Edn {
    if (token === 'nil') return null
    if (token === 'true') return true
    if (token === 'false') return false
    if (token.startsWith(':')) return { keyword: token.slice(1) }
    if (/^[+-]?\d/.test(token)) {
      const number = Number(token.replace(/[NM]$/, ''))
      if (Number.isNaN(number)) this.fail(`bad number ${token}`)
      return number
    }
    return { symbol: token }
  }

  end(): void {
    this.skip()
    if (this.index < this.text.length) this.fail('trailing forms')
  }
}

/**
 * Read exactly one EDN form.
 * @param text the document.
 */
export function readEdn(text: string): Edn {
  const reader = new Reader(text)
  const value = reader.read()
  reader.end()
  return value
}
