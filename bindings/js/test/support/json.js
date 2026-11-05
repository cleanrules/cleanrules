// A lossless JSON reader and writer for the conformance smoke tests: numbers
// keep their text and objects their member order (`JSON.parse` turns `1.0`
// into `1` and moves integer-like keys first). Comparison follows
// `conformance/README.md`, *Comparing*. The full runner is task F.3.

class Num {
  constructor(text) {
    this.text = text
  }

  get isInteger() {
    return !/[.eE]/.test(this.text)
  }
}

class Obj {
  constructor(entries) {
    this.entries = entries
  }

  get(key) {
    const entry = this.entries.find(([k]) => k === key)
    return entry === undefined ? undefined : entry[1]
  }

  has(key) {
    return this.entries.some(([k]) => k === key)
  }
}

function parse(text) {
  let i = 0
  const space = () => {
    while (i < text.length && ' \t\n\r'.includes(text[i])) i += 1
  }
  const value = () => {
    space()
    const c = text[i]
    if (c === '{') {
      i += 1
      const entries = []
      space()
      if (text[i] === '}') {
        i += 1
        return new Obj(entries)
      }
      for (;;) {
        space()
        const key = string()
        space()
        i += 1 // ':'
        entries.push([key, value()])
        space()
        if (text[i] === ',') {
          i += 1
          continue
        }
        i += 1 // '}'
        return new Obj(entries)
      }
    }
    if (c === '[') {
      i += 1
      const items = []
      space()
      if (text[i] === ']') {
        i += 1
        return items
      }
      for (;;) {
        items.push(value())
        space()
        if (text[i] === ',') {
          i += 1
          continue
        }
        i += 1 // ']'
        return items
      }
    }
    if (c === '"') return string()
    for (const [word, result] of [
      ['true', true],
      ['false', false],
      ['null', null],
    ]) {
      if (text.startsWith(word, i)) {
        i += word.length
        return result
      }
    }
    const match = /^-?\d+(\.\d+)?([eE][+-]?\d+)?/.exec(text.slice(i, i + 400))
    if (match === null) throw new SyntaxError(`bad JSON at ${i}`)
    i += match[0].length
    return new Num(match[0])
  }
  const string = () => {
    const start = i
    i += 1
    while (text[i] !== '"') i += text[i] === '\\' ? 2 : 1
    i += 1
    return JSON.parse(text.slice(start, i))
  }
  const result = value()
  return result
}

function write(tree) {
  if (tree instanceof Num) return tree.text
  if (tree instanceof Obj) return `{${tree.entries.map(([k, v]) => `${JSON.stringify(k)}:${write(v)}`).join(',')}}`
  if (Array.isArray(tree)) return `[${tree.map(write).join(',')}]`
  return JSON.stringify(tree)
}

function sameNumber(a, b) {
  if (a.isInteger !== b.isInteger) return false
  if (a.isInteger) return BigInt(a.text) === BigInt(b.text)
  return Object.is(Number(a.text), Number(b.text))
}

/** Whether `actual` matches `expected` (`conformance/README.md`, *Comparing*). */
function same(actual, expected, inReason = false) {
  if (expected instanceof Num) return actual instanceof Num && sameNumber(actual, expected)
  if (Array.isArray(expected)) {
    return Array.isArray(actual) && actual.length === expected.length && expected.every((e, k) => same(actual[k], e))
  }
  if (expected instanceof Obj) {
    if (!(actual instanceof Obj)) return false
    // Any two opaque values are equal.
    if (expected.entries.length === 1 && expected.has('$opaque')) return actual.has('$opaque')
    const strip = (o) => (inReason ? o.entries.filter(([k]) => k !== 'detail') : o.entries)
    const a = strip(actual)
    const e = strip(expected)
    return a.length === e.length && e.every(([k, v], n) => a[n][0] === k && same(a[n][1], v, k === 'reason'))
  }
  return actual === expected
}

export { Num, Obj, parse, write, same }
