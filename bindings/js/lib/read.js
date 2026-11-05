// Reading native JavaScript data (spec/data-model.md, *Reading native data*
// and *Conversions by language*, *JavaScript and TypeScript*).
//
// The reader walks an input along the paths a rule set or node reads, segment
// by segment, each native object once, and converts only what it finds at the
// end of each path. It returns the sparse value tree in a compact encoding
// (`src/wire.rs` lists the forms), plus the values that getters threw, so that
// strict mode can chain them. It runs in JavaScript because property access is
// much faster here than through N-API, and so that the wasm build (phase F.1b)
// can share it.
//
// Safety (spec/data-model.md, *Safety*): a string segment reads a `Map` key,
// or an object's own enumerable string-keyed property, or a getter defined by
// one of its classes (never by `Object.prototype`); never a symbol, a `#`
// field, a method (a function value), a prototype, `constructor` or
// `__proto__`, and never anything of a scalar. An integer segment indexes
// arrays and typed arrays only. A key that is absent or holds `undefined` is
// missing. A getter that throws gives a null with reason `read_error`.

import { MARKER } from './values.js'

/** How deep containers are converted: the format's node depth. */
const MAX_DEPTH = 256
/** How many prototypes a lookup walks (a Proxy can make the chain endless). */
const MAX_PROTOTYPES = 64
const I64_MIN = -(2n ** 63n)
const I64_MAX = 2n ** 63n - 1n
/** 0001-01-01T00:00:00Z and 9999-12-31T23:59:59.999999Z, in microseconds. */
const FIRST_MICROS = -62135596800000000n
const LAST_MICROS = 253402300799999999n
/** The largest UTC offset, in seconds: 18 hours. */
const MAX_OFFSET = 18 * 3600

// Built-in objects are told apart by brand: a built-in method or getter that
// only works on objects with the built-in's internal slot, in any realm (an
// iframe, a `vm` context). Where Node's `util.types` exists (`lib/install.js`
// injects it), its checks are brand checks that never throw; elsewhere, and
// for the checks a runtime lacks (Deno has no `isExternal`), the reader calls
// the built-in's own method and catches the `TypeError`. Objects whose
// prototypes are this realm's are checked by their prototypes first, without
// calling anything, so the brand calls run only for objects of other realms.
let types = {}

/** The checks of `util.types` that find objects of the runtime. */
const RUNTIME_CHECKS = [
  'isNativeError',
  'isSet',
  'isWeakMap',
  'isWeakSet',
  'isPromise',
  'isRegExp',
  'isAnyArrayBuffer',
  'isDataView',
  'isBoxedPrimitive',
  'isGeneratorObject',
  'isMapIterator',
  'isSetIterator',
  'isModuleNamespaceObject',
  'isExternal',
]
let runtimeChecks = []

/** The prototypes of the binding's own classes, whose objects are opaque. */
let engineProtos = new Set()

/**
 * Use `checks` (Node's `util.types`, or `null`) to recognise built-in objects,
 * and treat instances of `classes` (the engine's classes) as opaque.
 */
function useTypes(checks, classes = []) {
  types = {}
  for (const name of ['isDate', 'isMap', ...RUNTIME_CHECKS]) {
    if (typeof checks?.[name] === 'function') {
      types[name] = checks[name]
    }
  }
  runtimeChecks = RUNTIME_CHECKS.filter((name) => name in types).map((name) => types[name])
  engineProtos = new Set(classes.map((c) => c.prototype))
}

const getOwn = Object.getOwnPropertyDescriptor
const getProto = Object.getPrototypeOf
const ObjectPrototype = Object.prototype
const mapHas = Map.prototype.has
const mapGet = Map.prototype.get
const mapEntries = Map.prototype.entries
const dateTime = Date.prototype.getTime
// The getter of %TypedArray%.prototype[Symbol.toStringTag] gives the name of
// any typed array, of any realm, and `undefined` for everything else.
const typedArrayName = getOwn(getProto(Uint8Array.prototype), Symbol.toStringTag).get

const getter = (C, name) => getOwn(C.prototype, name).get

/** Whether `method` accepts `value` as its receiver: a brand check. */
function branded(method, value, ...args) {
  try {
    method.call(value, ...args)
    return true
  } catch {
    return false
  }
}

/** Brand checks of the runtime objects, for objects of other realms. */
const BRANDS = [
  [getter(Set, 'size')],
  [WeakMap.prototype.has, {}],
  [WeakSet.prototype.has, {}],
  [getter(ArrayBuffer, 'byteLength')],
  [getter(DataView, 'byteLength')],
  [getter(RegExp, 'source')],
  [Number.prototype.valueOf],
  [String.prototype.valueOf],
  [Boolean.prototype.valueOf],
  [Symbol.prototype.valueOf],
  [BigInt.prototype.valueOf],
  ...(typeof SharedArrayBuffer === 'function' ? [[getter(SharedArrayBuffer, 'byteLength')]] : []),
]

/** The tags of runtime objects that have no brand method without effects. */
const RUNTIME_TAGS = new Set([
  'Generator',
  'AsyncGenerator',
  'Iterator',
  'Array Iterator',
  'Map Iterator',
  'Set Iterator',
  'String Iterator',
  'RegExp String Iterator',
  'Segmenter String Iterator',
  'Module',
  'Promise',
  'WeakRef',
  'FinalizationRegistry',
])

/** This realm's prototypes of runtime objects. */
const RUNTIME_PROTOS = new Set(
  [
    Error,
    Set,
    WeakMap,
    WeakSet,
    Promise,
    RegExp,
    ArrayBuffer,
    DataView,
    Number,
    String,
    Boolean,
    Symbol,
    BigInt,
    globalThis.SharedArrayBuffer,
    globalThis.WeakRef,
    globalThis.FinalizationRegistry,
  ]
    .filter((c) => typeof c === 'function')
    .map((c) => c.prototype),
)

/** How many prototypes a chain may have before it counts as endless. */
const MAX_CHAIN = 64

/** The prototype chain of `value`, read once (a Proxy's trap runs once). */
function chainOf(value) {
  const chain = []
  for (let proto = getProto(value); proto !== null && chain.length < MAX_CHAIN; proto = getProto(proto)) {
    chain.push(proto)
  }
  return chain
}

/** Whether `chain` is another realm's: it does not end in this realm's `Object.prototype`. */
const foreign = (chain) => chain.length > 0 && !chain.includes(ObjectPrototype)

function isDate(value, chain) {
  if (types.isDate) return types.isDate(value)
  return chain.includes(Date.prototype) || (foreign(chain) && branded(dateTime, value))
}

function isMap(value, chain) {
  if (types.isMap) return types.isMap(value)
  return chain.includes(Map.prototype) || (foreign(chain) && branded(getter(Map, 'size'), value))
}

/**
 * Objects of the runtime, and the engine's own, which are opaque and never
 * read through; `tag` is the value's `Symbol.toStringTag`, read once.
 */
function isRuntimeObject(value, tag, chain) {
  if (chain.some((proto) => RUNTIME_PROTOS.has(proto) || engineProtos.has(proto))) return true
  if (typeof tag === 'string' && RUNTIME_TAGS.has(tag)) return true
  if (typeof Error.isError === 'function' && Error.isError(value)) return true
  if (runtimeChecks.some((check) => check(value))) return true
  return foreign(chain) && BRANDS.some(([method, ...args]) => branded(method, value, ...args))
}

const MISSING = Symbol('missing')
const HOLE = Symbol('hole')

// Shapes: how a value is read.
const SCALAR = 0
const LIST = 1
const MAP = 2
const OBJECT = 3
const OPAQUE = 4

/** The tree of `paths`: `{ end, keys: Map, indices: Map }`, in first-seen order. */
function pathTree(paths) {
  const root = { end: false, keys: new Map(), indices: new Map() }
  for (const path of paths) {
    let node = root
    for (const segment of path) {
      const table = typeof segment === 'number' ? node.indices : node.keys
      let child = table.get(segment)
      if (child === undefined) {
        child = { end: false, keys: new Map(), indices: new Map() }
        table.set(segment, child)
      }
      node = child
    }
    node.end = true
  }
  return root
}

// One tree per rule set or node, for as long as it lives.
const trees = new WeakMap()

/**
 * The path tree of `owner`: a rule set or node of the napi-rs build (which has
 * `fieldPaths()`), or the array of field paths that a rule set or node of the
 * WebAssembly build makes once and passes instead.
 */
function treeOf(owner) {
  let tree = trees.get(owner)
  if (tree === undefined) {
    tree = pathTree(Array.isArray(owner) ? owner : owner.fieldPaths())
    trees.set(owner, tree)
  }
  return tree
}

/** What a thrown value says: `"TypeError: message"`, never throwing itself. */
function describe(thrown) {
  try {
    if (thrown instanceof Error) {
      return `${String(thrown.name)}: ${String(thrown.message)}`
    }
    return String(thrown)
  } catch {
    return 'a value that cannot be described'
  }
}

/** The name of a value's class, for opaque values; runs no getter. */
function typeName(value) {
  if (value === null || (typeof value !== 'object' && typeof value !== 'function')) {
    return typeof value
  }
  try {
    const proto = getProto(value)
    const constructor = proto === null ? undefined : getOwn(proto, 'constructor')
    if (constructor !== undefined && typeof constructor.value === 'function' && constructor.value.name) {
      return constructor.value.name
    }
  } catch {
    // A Proxy whose traps throw: no name.
  }
  return typeof value === 'function' ? 'Function' : 'Object'
}

/** `floor(a / b)` for bigints (division truncates toward zero). */
function floorDiv(a, b) {
  const q = a / b
  return a % b !== 0n && a < 0n !== b < 0n ? q - 1n : q
}

/**
 * The shape of an object (not null): `[kind]`, `[LIST, length]`, or for a
 * scalar `[SCALAR, Temporal tag or undefined, marker kind or undefined]`.
 * Computed once per value, so that no getter runs twice.
 */
function shapeOf(value) {
  const marker = value[MARKER]
  if (typeof marker === 'string') {
    return [SCALAR, undefined, marker]
  }
  const chain = chainOf(value)
  if (isDate(value, chain)) {
    return [SCALAR]
  }
  const typed = typedArrayName.call(value)
  // Bytes (`Uint8Array`, and `Buffer`, its subclass) are opaque, as in every
  // language; other typed arrays are lists of numbers.
  if (typed === 'Uint8Array') {
    return [OPAQUE]
  }
  if (Array.isArray(value) || typed !== undefined) {
    return [LIST, value.length]
  }
  if (isMap(value, chain)) {
    return [MAP]
  }
  // Read once: a getter of the tag runs at most once.
  const tag = value[Symbol.toStringTag]
  if (isRuntimeObject(value, tag, chain)) {
    return [OPAQUE]
  }
  // Temporal values, and polyfills, by their tag.
  if (typeof tag === 'string' && tag.startsWith('Temporal.')) {
    return [SCALAR, tag]
  }
  return [OBJECT]
}

/** Read `key` of an object: `MISSING`, or `{ value }`. Throws what a getter throws. */
function objectLookup(object, key) {
  if (key === 'constructor' || key === '__proto__') {
    return MISSING
  }
  let found
  const own = getOwn(object, key)
  if (own !== undefined) {
    if (!own.enumerable) {
      return MISSING
    }
    if ('value' in own) {
      found = own.value
    } else if (own.get !== undefined) {
      found = own.get.call(object)
    } else {
      return MISSING
    }
  } else {
    // A getter defined by one of the object's classes; a method or any other
    // prototype data is not data.
    let proto = getProto(object)
    for (let walked = 0; ; walked += 1) {
      if (proto === null || proto === ObjectPrototype || walked >= MAX_PROTOTYPES) {
        return MISSING
      }
      const inherited = getOwn(proto, key)
      if (inherited !== undefined) {
        if (inherited.get === undefined) {
          return MISSING
        }
        found = inherited.get.call(object)
        break
      }
      proto = getProto(proto)
    }
  }
  // A method is not data, and `undefined` is missing.
  if (found === undefined || typeof found === 'function') {
    return MISSING
  }
  return { value: found }
}

/** Read `key` of a `Map` (string keys only): `MISSING`, or `{ value }`. */
function mapLookup(map, key) {
  if (!mapHas.call(map, key)) {
    return MISSING
  }
  const found = mapGet.call(map, key)
  return found === undefined ? MISSING : { value: found }
}

/** Element `index` of a list: `HOLE` for a hole or `undefined`. */
function element(list, index) {
  if (!(index in list)) {
    return HOLE
  }
  const found = list[index]
  return found === undefined ? HOLE : found
}

/** The keys of an object when it is converted whole, in order. */
function objectKeys(object) {
  const keys = Object.keys(object).filter((k) => k !== 'constructor' && k !== '__proto__' && k.isWellFormed())
  const seen = new Set(keys)
  let proto = getProto(object)
  for (let walked = 0; proto !== null && proto !== ObjectPrototype && walked < MAX_PROTOTYPES; walked += 1) {
    for (const name of Object.getOwnPropertyNames(proto)) {
      if (seen.has(name) || name === 'constructor' || name === '__proto__' || !name.isWellFormed()) {
        continue
      }
      const descriptor = getOwn(proto, name)
      if (descriptor !== undefined && descriptor.get !== undefined) {
        seen.add(name)
        keys.push(name)
      }
    }
    proto = getProto(proto)
  }
  return keys
}

/** A float's shortest round-trip text, with `-0`. */
function floatText(x) {
  return Object.is(x, -0) ? '-0' : String(x)
}

class Reader {
  constructor() {
    /** Values thrown while reading, by index. */
    this.errors = []
    /** The token of each opaque value, in the order they are met. */
    this.tokens = new Map()
    /** The containers being converted, for cycles. */
    this.open = new Set()
  }

  raised(thrown) {
    this.errors.push(thrown)
    return ['E', describe(thrown), this.errors.length - 1]
  }

  opaque(value, name) {
    let token = this.tokens.get(value)
    if (token === undefined) {
      token = this.tokens.size
      this.tokens.set(value, token)
    }
    return ['X', name, token]
  }

  /** Walk `value` along the path tree `node`. */
  walk(value, node) {
    if (node.end || value === null || typeof value !== 'object') {
      return this.convert(value)
    }
    let shape
    try {
      shape = shapeOf(value)
    } catch (thrown) {
      return this.raised(thrown)
    }
    switch (shape[0]) {
      case MAP:
      case OBJECT: {
        const lookup = shape[0] === MAP ? mapLookup : objectLookup
        const entries = []
        for (const [key, sub] of node.keys) {
          let found
          try {
            found = lookup(value, key)
          } catch (thrown) {
            entries.push([key, this.raised(thrown)])
            continue
          }
          if (found !== MISSING) {
            entries.push([key, this.walk(found.value, sub)])
          }
        }
        return ['O', entries]
      }
      case LIST: {
        const length = shape[1]
        const entries = []
        for (const [index, sub] of node.indices) {
          if (index >= length) {
            continue
          }
          let found
          try {
            found = element(value, index)
          } catch (thrown) {
            entries.push([index, this.raised(thrown)])
            continue
          }
          entries.push([index, found === HOLE ? ['M'] : this.walk(found, sub)])
        }
        return ['L', length, entries]
      }
      default:
        return this.convert(value, shape)
    }
  }

  /** The whole value, where a path ends; `known` is its shape, if computed. */
  convert(value, known) {
    switch (typeof value) {
      case 'boolean':
        return value
      case 'string':
        // A lone surrogate has no Unicode form.
        return value.isWellFormed() ? value : this.opaque(value, 'string')
      case 'number':
        if (Number.isNaN(value)) {
          return null
        }
        if (Number.isSafeInteger(value) && !Object.is(value, -0)) {
          return ['i', String(value)]
        }
        // Every other number, -0 and infinities included.
        return ['f', floatText(value)]
      case 'bigint':
        return value >= I64_MIN && value <= I64_MAX ? ['i', value.toString()] : ['IO']
      case 'undefined':
        return ['M']
      case 'symbol':
      case 'function':
        return this.opaque(value, typeName(value))
      default:
        break
    }
    if (value === null) {
      return null
    }
    let shape = known
    if (shape === undefined) {
      try {
        shape = shapeOf(value)
      } catch (thrown) {
        return this.raised(thrown)
      }
    }
    if (shape[0] === SCALAR) {
      try {
        return this.scalar(value, shape[1], shape[2])
      } catch (thrown) {
        return this.raised(thrown)
      }
    }
    if (shape[0] === OPAQUE || this.open.has(value) || this.open.size >= MAX_DEPTH) {
      return this.opaque(value, typeName(value))
    }
    this.open.add(value)
    try {
      return this.container(value, shape)
    } finally {
      this.open.delete(value)
    }
  }

  container(value, shape) {
    if (shape[0] === LIST) {
      const items = []
      for (let index = 0; index < shape[1]; index += 1) {
        let found
        try {
          found = element(value, index)
        } catch (thrown) {
          items.push(this.raised(thrown))
          continue
        }
        items.push(found === HOLE ? ['M'] : this.convert(found))
      }
      return ['l', items]
    }
    const entries = []
    if (shape[0] === MAP) {
      for (const [key, item] of mapEntries.call(value)) {
        // An entry whose key is not a string (or not Unicode) is left out
        // (spec/data-model.md, *Lists and maps*).
        if (typeof key === 'string' && key.isWellFormed() && item !== undefined) {
          entries.push([key, this.convert(item)])
        }
      }
      return ['O', entries]
    }
    let keys
    try {
      keys = objectKeys(value)
    } catch (thrown) {
      return this.raised(thrown)
    }
    for (const key of keys) {
      let found
      try {
        found = objectLookup(value, key)
      } catch (thrown) {
        entries.push([key, this.raised(thrown)])
        continue
      }
      if (found !== MISSING) {
        entries.push([key, this.convert(found.value)])
      }
    }
    return ['O', entries]
  }

  /**
   * A scalar object: a marker (by its `marker` kind), a `Date`, or a Temporal
   * value (by `tag`). Each getter is read once.
   */
  scalar(value, tag, marker) {
    if (marker !== undefined) {
      return this.marker(value, marker)
    }
    if (tag === undefined) {
      // A `Date`: an instant in UTC, or null when it is Invalid.
      const millis = dateTime.call(value)
      return Number.isNaN(millis) ? null : this.instant(value, BigInt(millis) * 1000n, 0, null)
    }
    switch (tag) {
      case 'Temporal.PlainDate': {
        const { calendarId, year, month, day } = value
        if (calendarId !== 'iso8601' || !(year >= 1 && year <= 9999)) {
          return this.opaque(value, tag)
        }
        return this.civil(value, tag, ['D', year, month, day])
      }
      case 'Temporal.PlainDateTime': {
        const { calendarId, year, month, day, hour, minute, second, millisecond, microsecond } = value
        if (calendarId !== 'iso8601' || !(year >= 1 && year <= 9999)) {
          return this.opaque(value, tag)
        }
        // Nanoseconds are truncated.
        return this.civil(value, tag, ['N', year, month, day, hour, minute, second, millisecond * 1000 + microsecond])
      }
      case 'Temporal.Instant':
        return this.instant(value, floorDiv(value.epochNanoseconds, 1000n), 0, null)
      case 'Temporal.ZonedDateTime': {
        const { calendarId, offsetNanoseconds: offset, timeZoneId: id, epochNanoseconds } = value
        if (calendarId !== 'iso8601' || typeof offset !== 'number') {
          return this.opaque(value, tag)
        }
        if (offset % 1e9 !== 0 || Math.abs(offset / 1e9) > MAX_OFFSET) {
          return this.opaque(value, tag)
        }
        // A zone id that is an offset, such as "+05:30", is a fixed offset.
        const zone = typeof id === 'string' && !/^[+-]/.test(id) ? id : null
        return this.instant(value, floorDiv(epochNanoseconds, 1000n), offset / 1e9, zone)
      }
      default:
        // Temporal.Duration, PlainTime, PlainYearMonth, PlainMonthDay...
        return this.opaque(value, tag)
    }
  }

  /** A marker value; one whose fields are not what its kind needs is opaque. */
  marker(value, kind) {
    switch (kind) {
      case 'decimal': {
        const { text } = value
        return typeof text === 'string' ? ['d', text] : this.opaque(value, 'Decimal')
      }
      case 'float': {
        const number = value.value
        if (typeof number !== 'number') {
          return this.opaque(value, 'Float')
        }
        return Number.isNaN(number) ? null : ['f', floatText(number)]
      }
      case 'date': {
        const { year, month, day } = value
        return this.civil(value, 'LocalDate', ['D', year, month, day])
      }
      case 'naive_datetime': {
        const { year, month, day, hour, minute, second, microsecond } = value
        return this.civil(value, 'NaiveDateTime', ['N', year, month, day, hour, minute, second, microsecond])
      }
      default:
        return this.opaque(value, typeName(value))
    }
  }

  /** A date or naive datetime form, if its fields are whole numbers. */
  civil(value, name, form) {
    return form.slice(1).every(Number.isSafeInteger) ? form : this.opaque(value, name)
  }

  instant(value, micros, offset, zone) {
    if (typeof micros !== 'bigint' || micros < FIRST_MICROS || micros > LAST_MICROS) {
      return this.opaque(value, typeName(value))
    }
    return ['A', micros.toString(), offset, zone]
  }
}

/**
 * Read `data` along the paths of `owner` (a `RuleSet` or a `Node`):
 * `[encoding, thrown]`.
 */
function read(data, owner) {
  if (data === undefined) {
    throw new TypeError('the input is undefined; pass the data to evaluate (null for none)')
  }
  const reader = new Reader()
  const encoded = reader.walk(data, treeOf(owner))
  return [JSON.stringify(encoded), reader.errors]
}

export { describe, read, useTypes }
