// A lossless JSON reader, writer and comparer for the conformance runner
// (conformance/README.md, *Comparing*), which never uses the library's own
// JSON or equality: numbers keep their text (`JSON.parse` turns `1.0` into
// `1` and reads integers beyond 2⁵³ through a float) and objects their member
// order (plain objects move integer-like keys first), as the C++ runner's
// json.hpp does.

class Num {
  /**
   * `text` is the number as written. `loose` marks an int made from a
   * JavaScript `number` given back by the binding, which cannot tell an int
   * from a float holding an integer (`spec/data-model.md`, *Values given
   * back*).
   */
  constructor(text, loose = false) {
    this.text = text
    this.loose = loose
  }

  /** Whether it is an int: no fraction and no exponent. */
  get isInteger() {
    return !/[.eE]/.test(this.text)
  }
}

class Obj {
  /** `entries` are the `[key, value]` members, in order. */
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

  /** The same object without the members `keys`. */
  without(...keys) {
    return new Obj(this.entries.filter(([k]) => !keys.includes(k)))
  }
}

const NUMBER = /-?(?:0|[1-9]\d*)(?:\.\d+)?(?:[eE][+-]?\d+)?/y
const STRING = /"(?:[^"\\\u0000-\u001f]|\\(?:["\\/bfnrt]|u[0-9a-fA-F]{4}))*"/y

/** Parse a JSON text (RFC 8259) into `Num`, `Obj`, arrays, strings, booleans and `null`. */
function parse(text) {
  let i = 0
  const fail = (what) => {
    throw new SyntaxError(`JSON: ${what} at ${i} in ${text.length > 200 ? `${text.slice(0, 200)}…` : text}`)
  }
  const space = () => {
    while (i < text.length && ' \t\n\r'.includes(text[i])) i += 1
  }
  const expect = (c) => {
    space()
    if (text[i] !== c) fail(`expected ${c}`)
    i += 1
  }
  const string = () => {
    STRING.lastIndex = i
    const match = STRING.exec(text)
    if (match === null) fail('bad string')
    i += match[0].length
    // Only the string: standard escapes, which the platform decodes exactly.
    return JSON.parse(match[0])
  }
  const value = (depth) => {
    if (depth > 10000) fail('too deep')
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
        if (text[i] !== '"') fail('expected a key')
        const key = string()
        expect(':')
        entries.push([key, value(depth + 1)])
        space()
        if (text[i] === ',') {
          i += 1
          continue
        }
        expect('}')
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
        items.push(value(depth + 1))
        space()
        if (text[i] === ',') {
          i += 1
          continue
        }
        expect(']')
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
    NUMBER.lastIndex = i
    const match = NUMBER.exec(text)
    if (match === null) fail('unexpected character')
    i += match[0].length
    return new Num(match[0])
  }
  const result = value(0)
  space()
  if (i !== text.length) fail('trailing characters')
  return result
}

/** Compact JSON, numbers as written. */
function write(tree) {
  if (tree instanceof Num) return tree.text
  if (tree instanceof Obj) return `{${tree.entries.map(([k, v]) => `${JSON.stringify(k)}:${write(v)}`).join(',')}}`
  if (Array.isArray(tree)) return `[${tree.map(write).join(',')}]`
  return JSON.stringify(tree)
}

/**
 * The tree of plain JavaScript data, such as an error's `toJSON()`: a
 * `number` must be a safe integer, the only numbers these hold.
 */
function tree(value) {
  if (typeof value === 'number') {
    if (!Number.isSafeInteger(value)) throw new RangeError(`not an exact JSON number: ${value}`)
    return new Num(String(value))
  }
  if (Array.isArray(value)) return value.map(tree)
  if (value !== null && typeof value === 'object') return new Obj(Object.keys(value).map((k) => [k, tree(value[k])]))
  return value
}

function sameNumber(a, b, given) {
  // Ints exactly, at any size.
  if (a.isInteger && b.isInteger) return BigInt(a.text) === BigInt(b.text)
  // A float holding an integer comes back to JavaScript as a `number`, which
  // reads as an int (`spec/data-model.md`, *Values given back*).
  if (given && (a.loose || b.loose)) return Object.is(Number(a.text), Number(b.text))
  if (a.isInteger !== b.isInteger) return false
  // Floats: the same double, and the same sign of zero.
  return Object.is(Number(a.text), Number(b.text))
}

/**
 * Whether `actual` matches `expected` (`conformance/README.md`, *Comparing*):
 * object members in order, ints exact at any size, floats by exact double and
 * sign of zero, any two opaque values equal, and `detail` in reasons ignored.
 *
 * `given` says that `actual` was encoded from values given back to
 * JavaScript, so that an int made from a `number` matches a float with the
 * same value (see `Num`); the order of a map's members given back is handled
 * by the caller (`jsOrder`). `recorded` accepts `null` for an opaque value, as
 * a decision read from a document gives it back.
 */
function same(actual, expected, mode = {}, inReason = false) {
  if (expected instanceof Num) return actual instanceof Num && sameNumber(actual, expected, mode.given === true)
  if (Array.isArray(expected)) {
    return Array.isArray(actual) && actual.length === expected.length && expected.every((e, k) => same(actual[k], e, mode))
  }
  if (expected instanceof Obj) {
    // Any two opaque values are equal.
    if (expected.entries.length === 1 && expected.has('$opaque')) {
      if (mode.recorded === true && actual === null) return true
      return actual instanceof Obj && actual.entries.length === 1 && actual.has('$opaque')
    }
    if (!(actual instanceof Obj)) return false
    const strip = (o) => (inReason ? o.entries.filter(([k]) => k !== 'detail') : o.entries)
    const a = strip(actual)
    const e = strip(expected)
    return a.length === e.length && e.every(([k, v], n) => a[n][0] === k && same(a[n][1], v, mode, k === 'reason'))
  }
  return actual === expected
}

/** Whether a key is an array index, which a plain object lists first, in ascending order. */
const isIndex = (key) => /^(?:0|[1-9]\d*)$/.test(key) && Number(key) < 2 ** 32 - 1

/**
 * A data value of the format with each map's members in the order a plain
 * object lists them (indices first, ascending, then the rest in order), as
 * a map comes back to JavaScript (`spec/data-model.md`, *Values given back*:
 * "equality ignores that order").
 */
function jsOrder(value) {
  if (Array.isArray(value)) return value.map(jsOrder)
  if (!(value instanceof Obj)) return value
  if (value.entries.length === 1 && value.entries[0][0].startsWith('$')) {
    const [[key, inner]] = value.entries
    return key === '$map' ? new Obj([[key, jsOrder(inner)]]) : value
  }
  const entries = value.entries.map(([k, v]) => [k, jsOrder(v)])
  const indices = entries.filter(([k]) => isIndex(k)).sort(([a], [b]) => Number(a) - Number(b))
  return new Obj([...indices, ...entries.filter(([k]) => !isIndex(k))])
}

export { Num, Obj, jsOrder, parse, same, tree, write }
