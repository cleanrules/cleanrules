// Native JavaScript data (spec/data-model.md, *Conversions by language*,
// *JavaScript and TypeScript*, and *Reading native data*): every row of the
// table, read and written back in the JSON format, and the safety rules.

import assert from 'node:assert/strict'
import { describe, test } from 'node:test'

import util from 'node:util'
import vm from 'node:vm'

import { useTypes } from '../lib/read.js'
import cr, { deno } from './support/cr.js'
import { detail, read, rules } from './support/probe.js'

const HAS_TEMPORAL = typeof Temporal === 'object'

/** A minimal Temporal polyfill value: recognised by its tag only. */
function fake(tag, fields) {
  return { [Symbol.toStringTag]: tag, calendarId: 'iso8601', ...fields }
}

describe('null and missing', () => {
  test('null is null; undefined and absent keys are missing', () => {
    assert.equal(read({ x: null }), 'null')
    assert.equal(read({ x: undefined }), 'missing_field')
    assert.equal(read({}), 'missing_field')
    assert.equal(read(null), 'null')
  })

  test('an Invalid Date is null', () => {
    assert.equal(read({ x: new Date(Number.NaN) }), 'null')
  })

  test('an undefined input is a TypeError', () => {
    assert.throws(() => rules('IF x > 1').evaluate(undefined), TypeError)
  })
})

describe('numbers', () => {
  test('booleans', () => {
    assert.equal(read({ x: true }), 'true')
    assert.equal(read({ x: false }), 'false')
  })

  test('integers within ±(2^53 - 1) are ints', () => {
    assert.equal(read({ x: 42 }), '42')
    assert.equal(read({ x: -7 }), '-7')
    assert.equal(read({ x: 0 }), '0')
    assert.equal(read({ x: Number.MAX_SAFE_INTEGER }), '9007199254740991')
    assert.equal(read({ x: Number.MIN_SAFE_INTEGER }), '-9007199254740991')
  })

  test('other finite numbers and -0 are floats; NaN is null', () => {
    assert.equal(read({ x: 2.5 }), '2.5')
    assert.equal(read({ x: -0 }), '-0.0')
    assert.equal(read({ x: 2 ** 53 }), '9007199254740992.0')
    assert.equal(read({ x: 1e21 }), '1e21')
    assert.equal(read({ x: 5e-324 }), '5e-324')
    assert.equal(read({ x: Number.NaN }), 'null')
  })

  test('float(x) is a float, NaN in it is null', () => {
    assert.equal(read({ x: cr.float(2) }), '2.0')
    assert.equal(read({ x: cr.float(Infinity) }), '{"$float":"inf"}')
    assert.equal(read({ x: cr.float(-Infinity) }), '{"$float":"-inf"}')
    assert.equal(read({ x: cr.float(Number.NaN) }), 'null')
    assert.throws(() => cr.float('2'), TypeError)
  })

  test('infinite numbers are floats', () => {
    assert.equal(read({ x: Infinity }), '{"$float":"inf"}')
    assert.equal(read({ x: -Infinity }), '{"$float":"-inf"}')
  })

  test('bigints are ints, or null with int_overflow', () => {
    assert.equal(read({ x: 5n }), '5')
    assert.equal(read({ x: 2n ** 63n - 1n }), '{"$int":"9223372036854775807"}')
    assert.equal(read({ x: -(2n ** 63n) }), '{"$int":"-9223372036854775808"}')
    assert.equal(read({ x: 2n ** 63n }), 'int_overflow')
    assert.equal(read({ x: -(2n ** 70n) }), 'int_overflow')
  })

  test('decimal(text) is a decimal, or null with decimal_overflow', () => {
    assert.equal(read({ x: cr.decimal('0.10') }), '{"$decimal":"0.10"}')
    assert.equal(read({ x: cr.decimal('-1.5e-3') }), '{"$decimal":"-0.0015"}')
    assert.equal(read({ x: cr.decimal('inf') }), '{"$decimal":"inf"}')
    assert.equal(read({ x: cr.decimal('1e40') }), 'decimal_overflow')
    assert.throws(() => cr.decimal('ten'), TypeError)
    assert.throws(() => cr.decimal('NaN'), TypeError)
    assert.throws(() => cr.decimal(0.1), TypeError)
    assert.equal(String(cr.decimal('0.10')), '0.10')
    assert.ok(cr.decimal('1') instanceof cr.Decimal)
  })
})

describe('strings', () => {
  test('strings are strings', () => {
    assert.equal(read({ x: 'café' }), '"café"')
    assert.equal(read({ x: '' }), '""')
  })

  test('a string with a lone surrogate is opaque', () => {
    assert.equal(read({ x: 'a\ud800' }), '{"$opaque":"string"}')
  })
})

describe('dates and datetimes', () => {
  test('date(...) is a date', () => {
    assert.equal(read({ x: cr.date(2026, 11, 4) }), '{"$date":"2026-11-04"}')
    assert.equal(String(cr.date(1, 1, 1)), '0001-01-01')
    assert.throws(() => cr.date(2026, 2, 30), RangeError)
    assert.throws(() => cr.date(0, 1, 1), RangeError)
    assert.throws(() => cr.date(2026, 13, 1), RangeError)
    assert.equal(read({ x: cr.date(2024, 2, 29) }), '{"$date":"2024-02-29"}')
  })

  test('markers are recognised by their brand, so other copies of the package work', () => {
    const MARKER = Symbol.for('cleanrules.marker')
    assert.equal(cr.decimal('1')[MARKER], 'decimal')
    assert.equal(read({ x: { [MARKER]: 'decimal', text: '1.50' } }), '{"$decimal":"1.50"}')
    assert.equal(read({ x: { [MARKER]: 'float', value: 2 } }), '2.0')
    assert.equal(read({ x: { [MARKER]: 'date', year: 2026, month: 11, day: 4 } }), '{"$date":"2026-11-04"}')
    // A forged marker that is not what its kind needs is opaque.
    assert.equal(read({ x: { [MARKER]: 'decimal', text: 'ten' } }), '{"$opaque":"Decimal"}')
    assert.equal(read({ x: { [MARKER]: 'date', year: 2026, month: 2, day: 30 } }), '{"$opaque":"date"}')
    assert.equal(read({ x: { [MARKER]: 'date', year: '2026', month: 1, day: 1 } }), '{"$opaque":"LocalDate"}')
    assert.equal(read({ x: { [MARKER]: 'float', value: '2' } }), '{"$opaque":"Float"}')
    assert.equal(read({ x: { [MARKER]: 'other' } }), '{"$opaque":"Object"}')
  })

  test('datetime(...) is a naive datetime', () => {
    assert.equal(read({ x: cr.datetime(2026, 11, 4, 9, 30) }), '{"$datetime":"2026-11-04T09:30:00"}')
    assert.equal(read({ x: cr.datetime(2026, 11, 4, 9, 30, 5, 7) }), '{"$datetime":"2026-11-04T09:30:05.000007"}')
    assert.equal(String(cr.datetime(2026, 1, 2, 3, 4, 5, 6)), '2026-01-02T03:04:05.000006')
    assert.throws(() => cr.datetime(2026, 1, 1, 24), RangeError)
    assert.throws(() => cr.datetime(2026, 1, 1, 0, 0, 0, 1e6), RangeError)
  })

  test('a Date is an aware datetime in Z', () => {
    assert.equal(read({ x: new Date(0) }), '{"$datetime":"1970-01-01T00:00:00Z"}')
    assert.equal(read({ x: new Date(-1) }), '{"$datetime":"1969-12-31T23:59:59.999000Z"}')
    assert.equal(read({ x: new Date('2026-11-04T09:30:00+01:00') }), '{"$datetime":"2026-11-04T08:30:00Z"}')
  })

  test('a Date outside years 1 to 9999 is opaque', () => {
    assert.equal(read({ x: new Date('+010000-01-01T00:00:00Z') }), '{"$opaque":"Date"}')
    assert.equal(read({ x: new Date('-000001-01-01T00:00:00Z') }), '{"$opaque":"Date"}')
  })

  test('Temporal values are recognised by their tag, so polyfills work', () => {
    assert.equal(read({ x: fake('Temporal.PlainDate', { year: 2026, month: 11, day: 4 }) }), '{"$date":"2026-11-04"}')
    const dt = { year: 2026, month: 11, day: 4, hour: 9, minute: 30, second: 1 }
    const plain = fake('Temporal.PlainDateTime', { ...dt, millisecond: 2, microsecond: 3, nanosecond: 999 })
    assert.equal(read({ x: plain }), '{"$datetime":"2026-11-04T09:30:01.002003"}')
    // Nanoseconds are truncated toward the past.
    assert.equal(read({ x: fake('Temporal.Instant', { epochNanoseconds: -1n }) }), '{"$datetime":"1969-12-31T23:59:59.999999Z"}')
    const zoned = fake('Temporal.ZonedDateTime', {
      epochNanoseconds: 1704099600000000000n,
      offsetNanoseconds: 3600e9,
      timeZoneId: 'Europe/Madrid',
    })
    assert.equal(read({ x: zoned }), '{"$datetime":"2024-01-01T10:00:00+01:00[Europe/Madrid]"}')
  })

  test('a ZonedDateTime whose zone id is an offset has a fixed offset', () => {
    const zoned = fake('Temporal.ZonedDateTime', {
      epochNanoseconds: 1704083400000000000n,
      offsetNanoseconds: 5.5 * 3600e9,
      timeZoneId: '+05:30',
    })
    assert.equal(read({ x: zoned }), '{"$datetime":"2024-01-01T10:00:00+05:30"}')
  })

  test('a zone name the core does not know keeps the offset', () => {
    const zoned = fake('Temporal.ZonedDateTime', {
      epochNanoseconds: 1704099600000000000n,
      offsetNanoseconds: 3600e9,
      timeZoneId: 'europe/madrid',
    })
    assert.equal(read({ x: zoned }), '{"$datetime":"2024-01-01T10:00:00+01:00"}')
  })

  test('each getter of a Temporal value runs once', () => {
    const counts = {}
    const counted = (name, value) => ({
      get() {
        counts[name] = (counts[name] ?? 0) + 1
        return value
      },
      enumerable: true,
    })
    const date = Object.defineProperties({}, {
      [Symbol.toStringTag]: counted('tag', 'Temporal.PlainDate'),
      calendarId: counted('calendarId', 'iso8601'),
      year: counted('year', 2026),
      month: counted('month', 11),
      day: counted('day', 4),
    })
    assert.equal(read({ x: date }), '{"$date":"2026-11-04"}')
    assert.deepEqual(counts, { tag: 1, calendarId: 1, year: 1, month: 1, day: 1 })
  })

  test('Temporal values in another calendar, or with sub-second offsets, are opaque', () => {
    const hebrew = { ...fake('Temporal.PlainDate', { year: 5785, month: 1, day: 1 }), calendarId: 'hebrew' }
    assert.equal(read({ x: hebrew }), '{"$opaque":"Temporal.PlainDate"}')
    const odd = fake('Temporal.ZonedDateTime', { epochNanoseconds: 0n, offsetNanoseconds: 1, timeZoneId: '+00:00:00.000000001' })
    assert.equal(read({ x: odd }), '{"$opaque":"Temporal.ZonedDateTime"}')
    assert.equal(read({ x: fake('Temporal.Duration', {}) }), '{"$opaque":"Temporal.Duration"}')
  })

  test('the runtime Temporal, where it exists', { skip: !HAS_TEMPORAL }, () => {
    assert.equal(read({ x: Temporal.PlainDate.from('2026-11-04') }), '{"$date":"2026-11-04"}')
    assert.equal(
      read({ x: Temporal.PlainDateTime.from('2024-01-01T10:00:00.123456789') }),
      '{"$datetime":"2024-01-01T10:00:00.123456"}',
    )
    assert.equal(read({ x: Temporal.Instant.from('2024-01-01T10:00:00Z') }), '{"$datetime":"2024-01-01T10:00:00Z"}')
    assert.equal(
      read({ x: Temporal.ZonedDateTime.from('2024-07-01T10:00[Europe/Madrid]') }),
      '{"$datetime":"2024-07-01T10:00:00+02:00[Europe/Madrid]"}',
    )
    assert.equal(
      read({ x: Temporal.ZonedDateTime.from('2024-01-01T10:00+05:30[+05:30]') }),
      '{"$datetime":"2024-01-01T10:00:00+05:30"}',
    )
    const hebrew = Temporal.PlainDate.from({ year: 2024, month: 1, day: 1, calendar: 'hebrew' })
    assert.equal(read({ x: hebrew }), '{"$opaque":"Temporal.PlainDate"}')
  })

  test('nothing is read through a date', () => {
    assert.equal(read({ x: new Date(0) }, 'x.getTime'), 'missing_field')
    assert.equal(read({ x: cr.date(2026, 1, 1) }, 'x.year'), 'missing_field')
    assert.equal(read({ x: fake('Temporal.PlainDate', { year: 2026, month: 1, day: 1 }) }, 'x.year'), 'missing_field')
  })
})

describe('lists', () => {
  test('arrays and typed arrays other than bytes are lists', () => {
    assert.equal(read({ x: [1, 'a', [2.5]] }), '[1,"a",[2.5]]')
    assert.equal(read({ x: new Uint16Array([1, 2]) }), '[1,2]')
    assert.equal(read({ x: new Uint8ClampedArray([1, 2]) }), '[1,2]')
    assert.equal(read({ x: new Float64Array([0.5, 1]) }), '[0.5,1]')
    assert.equal(read({ x: new BigInt64Array([2n ** 62n]) }), '[{"$int":"4611686018427387904"}]')
  })

  test('bytes are opaque, like bytes in other languages', () => {
    assert.equal(read({ x: new Uint8Array([1, 2]) }), '{"$opaque":"Uint8Array"}')
    assert.equal(read({ x: Buffer.from('ab') }), '{"$opaque":"Buffer"}')
    assert.equal(read({ x: Buffer.from('ab') }, 'x[0]'), 'missing_field')
    assert.equal(read({ x: new ArrayBuffer(2) }), '{"$opaque":"ArrayBuffer"}')
    assert.equal(read({ x: new DataView(new ArrayBuffer(2)) }), '{"$opaque":"DataView"}')
  })

  test('an integer segment indexes a list within its length', () => {
    assert.equal(read({ x: [10, 20] }, 'x[1]'), '20')
    assert.equal(read({ x: [10, 20] }, 'x[2]'), 'missing_field')
    assert.equal(read({ x: new Int32Array([7, 8]) }, 'x[0]'), '7')
    assert.equal(read([{ a: 1 }], '[0].a'), '1')
  })

  test('integer segments never index strings, maps or objects', () => {
    assert.equal(read({ x: 'abc' }, 'x[0]'), 'missing_field')
    assert.equal(read({ x: { 0: 'a' } }, 'x[0]'), 'missing_field')
    assert.equal(read({ x: new Map([['0', 'a']]) }, 'x[0]'), 'missing_field')
  })

  test('string segments never read a list', () => {
    assert.equal(read({ x: [1, 2] }, 'x.length'), 'missing_field')
  })

  // A list cannot hold a missing element (spec/data-model.md, *Lists and maps*).
  test('holes and undefined elements are null', () => {
    // eslint-disable-next-line no-sparse-arrays
    assert.equal(read({ x: [1, , 3] }), '[1,null,3]')
    assert.equal(read({ x: [1, undefined] }, 'x[1]'), 'null')
  })
})

describe('maps', () => {
  test('a Map is a map in insertion order', () => {
    assert.equal(read({ x: new Map([['b', 1], ['10', 2], ['a', 3]]) }), '{"b":1,"10":2,"a":3}')
    assert.equal(read({ x: new Map([['k', 1]]) }, 'x.k'), '1')
    assert.equal(read({ x: new Map([['k', undefined]]) }, 'x.k'), 'missing_field')
    assert.equal(read({ x: new Map() }, 'x.size'), 'missing_field')
  })

  test('a key that starts with $ is an ordinary key', () => {
    assert.equal(read({ x: { $ref: 1 } }), '{"$map":{"$ref":1}}')
  })

  test('entries with keys that are not strings are left out', () => {
    assert.equal(read({ x: new Map([[1, 'a'], ['b', 2]]) }), '{"b":2}')
  })

  test('a function in a Map is opaque', () => {
    assert.equal(read({ x: new Map([['f', () => 1]]) }, 'x.f'), '{"$opaque":"Function"}')
  })
})

describe('objects', () => {
  test('plain objects are maps of their own enumerable string keys', () => {
    assert.equal(read({ x: { b: 1, a: { c: true } } }), '{"b":1,"a":{"c":true}}')
    assert.equal(read({ x: { a: { c: true } } }, 'x.a.c'), 'true')
    const bare = Object.create(null)
    bare.k = 'v'
    assert.equal(read({ x: bare }), '{"k":"v"}')
  })

  test('JavaScript puts integer-like keys first', () => {
    assert.equal(read({ x: { b: 1, 10: 2, a: 3 } }), '{"10":2,"b":1,"a":3}')
  })

  test('class instances: own fields and class getters, never methods', () => {
    class Base {
      constructor() {
        this.id = 1
      }

      get kind() {
        return 'base'
      }

      method() {
        return 'no'
      }
    }
    class Customer extends Base {
      #secret = 'hidden'

      constructor() {
        super()
        this.name = 'Ana'
        this.callback = () => 'no'
      }

      get years() {
        return 6
      }

      peek() {
        return this.#secret
      }
    }
    const customer = new Customer()
    assert.equal(read({ x: customer }), '{"id":1,"name":"Ana","years":6,"kind":"base"}')
    assert.equal(read({ x: customer }, 'x.years'), '6')
    assert.equal(read({ x: customer }, 'x.kind'), '"base"')
    assert.equal(read({ x: customer }, 'x.method'), 'missing_field')
    assert.equal(read({ x: customer }, 'x.peek'), 'missing_field')
    assert.equal(read({ x: customer }, 'x.callback'), 'missing_field')
    assert.equal(read({ x: customer }, 'x.secret'), 'missing_field')
  })

  test('constructor and __proto__ are always missing', () => {
    assert.equal(read({ x: {} }, 'x.constructor'), 'missing_field')
    assert.equal(read({ x: {} }, 'x.`__proto__`'), 'missing_field')
    const own = JSON.parse('{"constructor": 1, "__proto__": 2, "k": 3}')
    assert.equal(read({ x: own }, 'x.constructor'), 'missing_field')
    assert.equal(read({ x: own }, 'x.`__proto__`'), 'missing_field')
    assert.equal(read({ x: own }), '{"k":3}')
  })

  test('non-enumerable properties and symbols are not data', () => {
    const object = { shown: 1, [Symbol('s')]: 2 }
    Object.defineProperty(object, 'hidden', { value: 3, enumerable: false })
    assert.equal(read({ x: object }), '{"shown":1}')
    assert.equal(read({ x: object }, 'x.hidden'), 'missing_field')
  })

  test('getters of Object.prototype are never read', () => {
    Object.defineProperty(Object.prototype, 'leak', { get: () => 'process', configurable: true })
    try {
      assert.equal(read({ x: {} }, 'x.leak'), 'missing_field')
      assert.equal(read({ x: {} }), '{}')
    } finally {
      delete Object.prototype.leak
    }
  })

  test('prototype data that is not a getter is not read', () => {
    const proto = { inherited: 1 }
    assert.equal(read({ x: Object.create(proto) }, 'x.inherited'), 'missing_field')
  })

  test('nothing is read through a scalar', () => {
    assert.equal(read({ x: 'abc' }, 'x.length'), 'missing_field')
    assert.equal(read({ x: 5 }, 'x.toFixed'), 'missing_field')
    assert.equal(read({ x: cr.decimal('1') }, 'x.text'), 'missing_field')
  })

  test('runtime objects are opaque and never read through', () => {
    assert.equal(read({ x: new Set([1]) }), '{"$opaque":"Set"}')
    assert.equal(read({ x: new Set([1]) }, 'x.size'), 'missing_field')
    assert.equal(read({ x: /a/ }), '{"$opaque":"RegExp"}')
    assert.equal(read({ x: new Error('e') }), '{"$opaque":"Error"}')
    assert.equal(read({ x: new Error('e') }, 'x.message'), 'missing_field')
    assert.equal(read({ x: Promise.resolve(1) }), '{"$opaque":"Promise"}')
    assert.equal(read({ x: new ArrayBuffer(2) }), '{"$opaque":"ArrayBuffer"}')
    assert.equal(read({ x: new WeakMap() }), '{"$opaque":"WeakMap"}')
    assert.equal(read({ x: Symbol('s') }), '{"$opaque":"symbol"}')
    // Iterators, generators and boxed primitives, by class or tag in every runtime.
    for (const value of [[1].values(), 'ab'[Symbol.iterator](), new Map().keys(), (function* g() {})(), Object(1), Object('s')]) {
      assert.match(read({ x: value }), /^\{"\$opaque":/, String(value))
    }
  })

  test("the engine's own objects are opaque", () => {
    const ruleset = rules('RULE a\nIF TRUE')
    assert.equal(read({ x: ruleset }), '{"$opaque":"RuleSet"}')
    assert.equal(read({ x: ruleset }, 'x.length'), 'missing_field')
    assert.equal(read({ x: ruleset.evaluate({}) }), '{"$opaque":"Decision"}')
    assert.equal(read({ x: cr.Document.read('x > 1', { format: 'text' }) }), '{"$opaque":"Document"}')
  })

  test('objects of another realm read as those of this one', { skip: deno }, () => {
    const realm = vm.createContext({})
    const make = (code) => vm.runInContext(code, realm)
    assert.equal(read({ x: make('({ a: [1, 2.5] })') }), '{"a":[1,2.5]}')
    assert.equal(read({ x: make('new Map([["k", 1]])') }), '{"k":1}')
    assert.equal(read({ x: make('new Date(0)') }), '{"$datetime":"1970-01-01T00:00:00Z"}')
    assert.equal(read({ x: make('new Uint8Array([1])') }), '{"$opaque":"Uint8Array"}')
    assert.equal(read({ x: make('new Int16Array([1, 2])') }), '[1,2]')
    for (const code of ['new Set([1])', 'new Error("e")', '/a/', 'Object(1)', 'new WeakMap()', 'Promise.resolve()', '[1].values()', 'new ArrayBuffer(1)', 'new DataView(new ArrayBuffer(1))']) {
      assert.match(read({ x: make(code) }), /^\{"\$opaque":/, code)
    }
  })

  test('without util.types, built-ins are told apart the same way', { skip: deno }, () => {
    useTypes(null, [cr.Document, cr.RuleSet, cr.Decision, cr.Node])
    try {
      const realm = vm.createContext({})
      const values = [
        new Set([1]),
        new Error('e'),
        Object(1),
        [1].values(),
        new Map([['k', 1]]),
        new Date(0),
        vm.runInContext('new Set([1])', realm),
        // Without util.types, an Error from another realm is known only
        // through Error.isError, which Node 22 lacks (see the README).
        ...(typeof Error.isError === 'function' ? [vm.runInContext('new Error("e")', realm)] : []),
        vm.runInContext('Object("s")', realm),
        vm.runInContext('new WeakSet()', realm),
        vm.runInContext('new Map([["k", 1]])', realm),
        vm.runInContext('new Date(0)', realm),
        rules('RULE a\nIF TRUE'),
      ]
      const without = values.map((x) => read({ x }))
      useTypes(util.types, [cr.Document, cr.RuleSet, cr.Decision, cr.Node])
      assert.deepEqual(values.map((x) => read({ x })), without)
    } finally {
      useTypes(util.types, [cr.Document, cr.RuleSet, cr.Decision, cr.Node])
    }
  })

  test('a container that holds itself is opaque where it repeats', () => {
    const loop = { a: 1 }
    loop.self = loop
    assert.equal(read({ x: loop }), '{"a":1,"self":{"$opaque":"Object"}}')
  })

  test('containers nested deeper than 256 levels are opaque there', () => {
    let deep = 1
    for (let i = 0; i < 300; i += 1) deep = [deep]
    const written = read({ x: deep })
    assert.match(written, /\{"\$opaque":"Array"\}/)
    assert.equal((written.match(/\[/g) ?? []).length, 256)
  })
})

describe('reading', () => {
  test('a getter that throws gives a null with read_error', () => {
    const data = {
      get x() {
        throw new RangeError('no access')
      },
    }
    assert.equal(read(data), 'read_error')
    assert.equal(detail(data), 'RangeError: no access')
    assert.equal(read(data, 'x.y'), 'read_error')
    const thrownValue = {
      get x() {
        throw 'plain'
      },
    }
    assert.equal(detail(thrownValue), 'plain')
  })

  test('a Proxy whose trap throws gives read_error', () => {
    const proxy = new Proxy(
      {},
      {
        getOwnPropertyDescriptor() {
          throw new Error('trap')
        },
      },
    )
    assert.equal(read({ x: proxy }, 'x.y'), 'read_error')
    const keyless = new Proxy(
      {},
      {
        ownKeys() {
          throw new Error('trap')
        },
      },
    )
    assert.equal(read({ x: keyless }), 'read_error')
  })

  test('only the paths the rule set reads are read, each object once', () => {
    let reads = 0
    const data = {
      get customer() {
        reads += 1
        // A different object each time: both paths see the first one.
        return { age: 30 + reads, name: 'Ana' }
      },
      get untouched() {
        throw new Error('never read')
      },
    }
    const ruleset = rules('RULE r\nIF customer.age > 18\nTHEN who = customer, age = customer.age')
    const decision = ruleset.evaluate(data)
    assert.equal(reads, 1)
    assert.equal(decision.outputJson(), '{"who":{"age":31,"name":"Ana"},"age":31}')
  })

  test('the same object is the same opaque value', () => {
    const ruleset = rules('HIT POLICY any\n\nRULE a\nIF TRUE\nTHEN v = x\n\nRULE b\nIF TRUE\nTHEN v = y')
    const shared = new Set()
    const decision = ruleset.evaluate({ x: shared, y: shared })
    assert.ok(decision.firedAt(0) && decision.firedAt(1))
    assert.throws(() => ruleset.evaluate({ x: new Set(), y: new Set() }), cr.HitPolicyError)
  })

  test('strict mode: a read error is an EvaluationError caused by what was thrown', () => {
    const thrown = new RangeError('no access')
    const data = {
      customer: {
        get age() {
          throw thrown
        },
      },
    }
    const ruleset = rules('STRICT\n\nRULE adult\nIF customer.age >= 18')
    assert.throws(
      () => ruleset.evaluate(data),
      (error) =>
        error instanceof cr.EvaluationError &&
        !(error instanceof cr.MissingFieldError) &&
        error.code === 'evaluation_error' &&
        error.cause === thrown &&
        error.context.rule === 'adult' &&
        error.context.path.join('.') === 'customer.age',
    )
  })

  test('strict mode: a missing field is a MissingFieldError without cause', () => {
    const ruleset = rules('STRICT\n\nRULE adult\nIF customer.age >= 18')
    assert.throws(
      () => ruleset.evaluate({ customer: {} }),
      (error) => error instanceof cr.MissingFieldError && error.cause === undefined,
    )
  })
})
