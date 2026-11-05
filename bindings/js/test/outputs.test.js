// Values given back (spec/data-model.md, *Values given back*), and the
// structure of explanations and summaries against the core's JSON (task F.2a).

import assert from 'node:assert/strict'
import { describe, test } from 'node:test'

import { root } from './support/cr.js'

const {
  Cause,
  CauseGroup,
  Counterfactual,
  Decimal,
  Explanation,
  F,
  FieldUsage,
  LocalDate,
  NaiveDateTime,
  Note,
  Reason,
  Rule,
  RuleExplanation,
  RuleNotFoundError,
  RuleSet,
  RuleSummary,
  Summary,
  ZonedDateTime,
  date,
  datetime,
  decimal,
  float,
} = root

/** The output `x` of evaluating `{ x: value }` (the value read, and given back). */
const ECHO = new RuleSet([new Rule('echo', F.go, { x: F.x })])
const echo = (value) => ECHO.evaluate({ go: true, x: value }).output.x

/** An aware datetime in a zone, through a recorded decision (no Temporal needed). */
function zoned(text) {
  const recorded = JSON.stringify(ECHO.evaluate({ go: true, x: new Date(0) }).toJSON())
  return ECHO.readDecision(recorded.replaceAll('1970-01-01T00:00:00Z', text)).output.x
}

describe('values given back', () => {
  test('ints: a number within ±(2^53 − 1), a bigint beyond', () => {
    assert.equal(echo(2 ** 53 - 1), 2 ** 53 - 1)
    assert.equal(echo(-(2 ** 53 - 1)), -(2 ** 53 - 1))
    assert.equal(echo(2n ** 53n - 1n), 2 ** 53 - 1)
    assert.equal(echo(2n ** 53n), 2n ** 53n)
    assert.equal(echo(-(2n ** 53n)), -(2n ** 53n))
    assert.equal(echo(2n ** 63n - 1n), 2n ** 63n - 1n)
    assert.equal(echo(0), 0)
  })

  test('floats: numbers, -0 and infinities included', () => {
    assert.ok(Object.is(echo(-0), -0))
    assert.equal(echo(Infinity), Infinity)
    assert.equal(echo(-Infinity), -Infinity)
    assert.equal(echo(0.1), 0.1)
    assert.equal(echo(5e-324), 5e-324)
    assert.equal(echo(1.7976931348623157e308), 1.7976931348623157e308)
    // A float that holds an integer is a number, which reads back as an int.
    assert.equal(echo(float(2)), 2)
    const product = new RuleSet([new Rule('r', F.go, { y: F.x.mul(2) })]).evaluate({ go: true, x: 1.5 }).output.y
    assert.equal(product, 3)
  })

  test('decimals: Decimal with the canonical text, infinities included', () => {
    const value = echo(decimal('007.50'))
    assert.ok(value instanceof Decimal)
    assert.equal(value.text, '7.50')
    assert.equal(echo(decimal('1.5e-3')).text, '0.0015')
    assert.equal(echo(decimal('-0.00')).text, '0.00')
    assert.equal(echo(decimal('inf')).text, 'inf')
    assert.equal(echo(decimal('-inf')).text, '-inf')
  })

  test('dates and naive datetimes: the markers', () => {
    const day = echo(date(2026, 11, 5))
    assert.ok(day instanceof LocalDate)
    assert.deepEqual({ ...day }, { year: 2026, month: 11, day: 5 })
    const moment = echo(datetime(2026, 11, 5, 9, 30, 15, 123456))
    assert.ok(moment instanceof NaiveDateTime)
    assert.equal(String(moment), '2026-11-05T09:30:15.123456')
  })

  test('aware datetimes: ZonedDateTime in UTC, in a zone and at a fixed offset', () => {
    const utc = echo(new Date(Date.UTC(2026, 10, 5, 9, 30, 0, 250)))
    assert.ok(utc instanceof ZonedDateTime)
    assert.ok(Object.isFrozen(utc))
    assert.deepEqual(
      [utc.year, utc.month, utc.day, utc.hour, utc.minute, utc.second, utc.microsecond, utc.offsetSeconds, utc.timeZone],
      [2026, 11, 5, 9, 30, 0, 250000, 0, null],
    )
    assert.equal(utc.epochMicroseconds, BigInt(Date.UTC(2026, 10, 5, 9, 30, 0, 250)) * 1000n)
    assert.equal(String(utc), '2026-11-05T09:30:00.250000Z')
    assert.equal(utc.toDate().getTime(), Date.UTC(2026, 10, 5, 9, 30, 0, 250))
    assert.equal(JSON.stringify({ at: utc }), '{"at":"2026-11-05T09:30:00.250000Z"}')

    const madrid = zoned('2026-10-31T09:30:00.000001+01:00[Europe/Madrid]')
    assert.deepEqual(
      [madrid.year, madrid.month, madrid.day, madrid.hour, madrid.minute, madrid.microsecond, madrid.offsetSeconds, madrid.timeZone],
      [2026, 10, 31, 9, 30, 1, 3600, 'Europe/Madrid'],
    )
    assert.equal(String(madrid), '2026-10-31T09:30:00.000001+01:00[Europe/Madrid]')
    assert.equal(madrid.toDate().toISOString(), '2026-10-31T08:30:00.000Z')

    const fixed = zoned('2026-10-31T09:30:00-03:30')
    assert.deepEqual([fixed.hour, fixed.offsetSeconds, fixed.timeZone], [9, -12600, null])
    assert.equal(String(fixed), '2026-10-31T09:30:00-03:30')

    // Before 1970, toDate rounds toward the past.
    assert.equal(zoned('1969-12-31T23:59:59.999999Z').toDate().getTime(), -1)
    assert.throws(() => new ZonedDateTime(), TypeError)
  })

  test('ZonedDateTime reads back as the same value, zone included', () => {
    for (const value of [zoned('2026-10-31T09:30:00+01:00[Europe/Madrid]'), zoned('2026-10-31T09:30:00+05:30'), echo(new Date(0))]) {
      const again = echo(value)
      assert.equal(String(again), String(value))
      assert.equal(again.timeZone, value.timeZone)
      const same = new RuleSet([new Rule('r', F.a.eq(F.b))]).evaluate({ a: value, b: value.toDate() })
      assert.deepEqual(same.fired, ['r'])
    }
    // A forged one (fields of the wrong types) is opaque, never a datetime.
    const forged = Object.create(ZonedDateTime.prototype, { epochMicroseconds: { value: 0n }, offsetSeconds: { value: 'x' }, timeZone: { value: null } })
    assert.equal(echo(forged), forged)
  })

  test('toTemporal: the same value, or a clear TypeError without Temporal', () => {
    const value = zoned('2026-10-31T09:30:00+01:00[Europe/Madrid]')
    const fixed = zoned('2026-10-31T09:30:00+05:30')
    const seconds = zoned('2026-10-31T09:30:00+05:30:15')
    assert.equal(seconds.offsetSeconds, 19815)
    if (typeof globalThis.Temporal === 'object') {
      assert.throws(() => seconds.toTemporal(), (error) => error instanceof RangeError && /\+05:30:15 has seconds.*toDate\(\)/.test(error.message))
      assert.equal(String(value.toTemporal()), '2026-10-31T09:30:00+01:00[Europe/Madrid]')
      assert.equal(String(fixed.toTemporal()), '2026-10-31T09:30:00+05:30[+05:30]')
      const from = echo(globalThis.Temporal.ZonedDateTime.from('2026-10-31T09:30:00+05:30[+05:30]'))
      assert.equal(String(from), '2026-10-31T09:30:00+05:30')
    } else {
      assert.throws(() => value.toTemporal(), (error) => error instanceof TypeError && /no Temporal/.test(error.message))
    }
  })

  test('null, whatever its reason; booleans and strings', () => {
    assert.equal(echo(null), null)
    assert.equal(echo(NaN), null)
    assert.equal(echo(2n ** 70n), null) // int_overflow
    assert.equal(echo(decimal('1e40')), null) // decimal_overflow
    const throwing = {
      go: true,
      get x() {
        throw new Error('no')
      },
    }
    assert.equal(ECHO.evaluate(throwing).output.x, null) // read_error
    assert.equal(ECHO.evaluate({ go: true }).output.x, null) // missing
    assert.equal(echo(true), true)
    assert.equal(echo('héllo'), 'héllo')
  })

  test('lists are arrays; maps are plain objects, __proto__ an own key', () => {
    assert.deepEqual(echo([1, [2.5, 'a'], null, undefined]), [1, [2.5, 'a'], null, null])
    const map = echo(new Map([['b', 1], ['__proto__', { c: 2n ** 60n }], ['a', [decimal('1.0')]]]))
    assert.equal(Object.getPrototypeOf(map), Object.prototype)
    assert.deepEqual(Object.keys(map), ['b', '__proto__', 'a'])
    assert.ok(Object.hasOwn(map, '__proto__'))
    assert.deepEqual(map.__proto__, { c: 2n ** 60n }) // eslint-disable-line no-proto
    assert.equal(map.a[0].text, '1.0')
    // Integer-like keys come first, as JavaScript orders them.
    assert.deepEqual(Object.keys(echo(new Map([['x', 1], ['2', 2]]))), ['2', 'x'])
  })

  test('opaque values give back the very object read', () => {
    const set = new Set([1])
    const bytes = new Uint8Array([1, 2])
    const fn = () => 1
    const symbol = Symbol('s')
    const lone = 'a\uD800'
    for (const value of [set, bytes, symbol, lone, new Map([[1, 2]]).keys()]) {
      assert.equal(echo(value), value)
    }
    // A function is a method where a field is read (never data), opaque inside a list.
    assert.equal(echo(fn), null)
    assert.equal(echo([fn])[0], fn)
    // Inside containers too, in several outputs of one decision.
    const rules = new RuleSet([new Rule('r', F.go, { a: F.a, b: F.b })])
    const output = rules.evaluate({ go: true, a: [set, { s: set }], b: bytes }).output
    assert.equal(output.a[0], set)
    assert.equal(output.a[1].s, set)
    assert.equal(output.b, bytes)
    // A recorded decision keeps the type's name only: null.
    const recorded = rules.readDecision(JSON.stringify(rules.evaluate({ go: true, a: set, b: 1 }).toJSON()))
    assert.equal(recorded.output.a, null)
  })

  test('what the core refuses comes back as the very object read', () => {
    // Forged markers: a malformed decimal, an impossible date and datetime.
    const forged = [
      Object.create(Decimal.prototype, { text: { value: 'abc' } }),
      Object.create(LocalDate.prototype, { year: { value: 2026 }, month: { value: 2 }, day: { value: 30 } }),
      Object.create(NaiveDateTime.prototype, {
        ...Object.fromEntries(['year', 'month', 'day', 'hour', 'minute', 'second', 'microsecond'].map((k, i) => [k, { value: [2026, 1, 1, 25, 0, 0, 0][i] }])),
      }),
    ]
    for (const value of forged) assert.equal(echo(value), value)
    // An instant the core cannot hold (its range ends before year 9999 does in UTC).
    const late = new Date(Date.UTC(9999, 11, 31, 12))
    assert.equal(echo(late), late)
    // An aware datetime whose local date falls before year 1 (spec/data-model.md, *Datetimes*).
    const early = Object.create(ZonedDateTime.prototype, {
      epochMicroseconds: { value: -62135596800000000n },
      offsetSeconds: { value: -3600 },
      timeZone: { value: null },
    })
    assert.equal(echo(early), early)
  })

  test('the binding\'s own objects are opaque as data', () => {
    const decision = ECHO.evaluate({ go: true, x: 1 })
    const explanation = decision.explain()
    for (const value of [explanation, explanation.rules[0], decision.results[0], ECHO.summary(), ECHO, decision, ECHO.rules[0], F.x]) {
      assert.equal(echo(value), value)
    }
  })

  test('an output feeds another evaluation unchanged', () => {
    const value = { n: 2n ** 60n, d: decimal('0.10'), day: date(2026, 1, 1), at: zoned('2026-10-31T09:30:00+01:00[Europe/Madrid]') }
    const first = echo(value)
    const again = echo(first)
    assert.equal(again.n, value.n)
    assert.equal(again.d.text, '0.10')
    assert.equal(String(again.day), '2026-01-01')
    assert.equal(String(again.at), String(value.at))
    const equal = new RuleSet([new Rule('r', F.a.eq(F.b))]).evaluate({ a: first.d, b: decimal('0.1') })
    assert.deepEqual(equal.fired, ['r'])
  })
})

/** The explanation document of a decision, as the core writes it. */
const coreExplanation = (decision) => decision.explain().toJSON().explanation

/** A requirement against its JSON form (spec/json-format.md, *Explanations*). */
function sameRequirement(item, json) {
  if (item instanceof CauseGroup) {
    const [kind] = Object.keys(json)
    assert.equal(item.kind, kind)
    assert.equal(item.items.length, json[kind].length)
    item.items.forEach((child, i) => sameRequirement(child, json[kind][i]))
    return
  }
  assert.ok(item instanceof Cause)
  assert.deepEqual(item.node.toJSON(), json.condition)
  assert.equal(item.truth, String(json.truth === null ? 'unknown' : json.truth))
  assert.equal(item.negated, json.negated)
  assert.equal(item.holds, (item.truth === 'true' && !item.negated) || (item.truth === 'false' && item.negated))
  assert.deepEqual(
    item.values.map(([path]) => path),
    json.values.map((entry) => entry.path),
  )
  if (json.reason === undefined) {
    assert.equal(item.reason, null)
  } else {
    assert.ok(item.reason instanceof Reason)
    assert.equal(item.reason.code, json.reason.code)
    assert.deepEqual(item.reason.paths, json.reason.paths ?? [])
  }
  if (json.counterfactual === undefined) {
    assert.equal(item.counterfactual, null)
  } else {
    assert.ok(item.counterfactual instanceof Counterfactual)
    assert.deepEqual(item.counterfactual.path, json.counterfactual.path)
    assert.equal(item.counterfactual.op, json.counterfactual.op)
  }
}

describe('Explanation', () => {
  const rules = new RuleSet(
    [
      new Rule('a', F.x.gt(10).and(F.y.le(3).not()), {}, { description: 'big x' }),
      new Rule('b', F.x.between(0, 5)),
      new Rule('c', F.x.gt(1).and(F.x.gt(1).not())),
      new Rule('d', F.x.gt(1), null, { priority: 1 }),
      new Rule('e', F.x.gt(0), null, { priority: 5 }),
      new Rule('f', F.z.gt(1).or(F.v.gt(1), F.w.eq('a'))),
    ],
    { hitPolicy: 'priority' },
  )
  const decision = rules.evaluate({ x: 8, y: 2, w: 'b' })

  test('has the core JSON form, rule by rule', () => {
    const explanation = decision.explain()
    assert.ok(explanation instanceof Explanation)
    assert.equal(decision.explain(), explanation)
    const json = coreExplanation(decision)
    assert.deepEqual(explanation.selected, json.selected)
    assert.equal(explanation.rules.length, json.rules.length)
    explanation.rules.forEach((entry, i) => {
      const expected = json.rules[i]
      assert.ok(entry instanceof RuleExplanation)
      assert.equal(entry.rule, expected.rule)
      assert.equal(entry.status, expected.status)
      assert.equal(entry.achievable, expected.achievable ?? null)
      if (expected.note === undefined) {
        assert.equal(entry.note, null)
      } else {
        assert.ok(entry.note instanceof Note)
        assert.equal(entry.note.kind, expected.note.kind)
        assert.equal(entry.note.winner, expected.note.winner)
        assert.equal(entry.note.priority, expected.note.priority ?? null)
        assert.equal(entry.note.winnerPriority, expected.note.winner_priority ?? null)
      }
      sameRequirement(entry.requirement, expected.requirement)
    })
    assert.deepEqual(
      explanation.rules.map((entry) => entry.status),
      ['not_fired', 'not_fired', 'not_fired', 'fired', 'selected', 'unknown'],
    )
    const d = explanation.forRule('d')
    assert.deepEqual([d.note.kind, d.note.winner, d.note.priority, d.note.winnerPriority], ['lower_priority', 'e', 1, 5])
    assert.equal(explanation.forRule('c').achievable, false)
    assert.equal(explanation.forRule('f').causes[0].reason.code, 'missing_field')
    assert.deepEqual(explanation.forRule('f').causes[0].reason.paths, [['z']])
  })

  test('text: the whole, each rule and each cause, as the core writes it', () => {
    const explanation = decision.explain()
    const text = String(explanation)
    assert.ok(text.startsWith('HIT POLICY priority: selected e\n'))
    for (const entry of explanation.rules) {
      assert.ok(text.includes(String(entry)), entry.rule)
      for (const cause of entry.causes) {
        assert.ok(String(entry).includes(String(cause)), String(cause))
      }
    }
    const a = explanation.forRule('a')
    assert.equal(a.causes.length, 2)
    assert.equal(String(a.causes[0]), 'x > 10 is false (x = 8); short by 2')
    assert.ok(String(a).includes('big x'))
  })

  test('causes: values, counterfactuals and their gaps', () => {
    const a = decision.whyNot('a')
    const [big, small] = a.causes
    assert.deepEqual(big.values, [[['x'], 8]])
    assert.deepEqual({ ...big.counterfactual }, { path: ['x'], op: 'gt', target: 10, gap: 2 })
    assert.equal(small.negated, true)
    assert.equal(small.counterfactual.op, 'gt')
    assert.ok(big.node.sameAs(F.x.gt(10)))
    const gaps = new RuleSet([
      new Rule('day', F.d.gt(date(2026, 1, 10))),
      new Rule('at', F.t.gt(datetime(2026, 1, 1, 12))),
      new Rule('exact', F.p.gt(decimal('10.05'))),
    ]).evaluate({ d: date(2026, 1, 1), t: datetime(2026, 1, 1, 10, 30), p: decimal('9.9') })
    assert.deepEqual(gaps.whyNot('day').causes[0].counterfactual.gap, { days: 9 })
    assert.deepEqual(gaps.whyNot('at').causes[0].counterfactual.gap, { microseconds: 5400000000 })
    const exact = gaps.whyNot('exact').causes[0].counterfactual
    assert.ok(exact.gap instanceof Decimal)
    assert.equal(exact.gap.text, '0.15')
    assert.equal(exact.target.text, '10.05')
  })

  test('groups and the frozen structure', () => {
    const f = decision.whyNot('f')
    assert.ok(f.requirement instanceof CauseGroup)
    assert.equal(f.requirement.kind, 'any')
    assert.equal(f.requirement.items.length, 2)
    for (const value of [decision.explain(), f, f.requirement, f.requirement.items, f.causes[0], f.causes[0].values]) {
      assert.ok(Object.isFrozen(value))
    }
    assert.throws(() => new Cause(), TypeError)
  })

  test('whyNot and forRule: RuleNotFoundError listing the rules', () => {
    for (const call of [() => decision.whyNot('nope'), () => decision.explain().forRule('nope')]) {
      assert.throws(call, (error) => {
        assert.ok(error instanceof RuleNotFoundError)
        assert.equal(error.code, 'rule_not_found')
        assert.equal(error.message, 'no rule named "nope"')
        assert.equal(error.hint, 'the rules are: a, b, c, d, e, f')
        assert.equal(error.context.name, 'nope')
        assert.deepEqual(error.context.available, ['a', 'b', 'c', 'd', 'e', 'f'])
        return true
      })
    }
  })

  test('large priorities in notes are bigints', () => {
    const big = 2n ** 60n
    const rules = new RuleSet([new Rule('low', F.x, null, { priority: 1 }), new Rule('high', F.x, null, { priority: big })], { hitPolicy: 'priority' })
    const note = rules.evaluate({ x: true }).whyNot('low').note
    assert.deepEqual([note.priority, note.winnerPriority], [1, big])
  })
})

describe('Summary', () => {
  const notify = () => {}
  const rules = new RuleSet(
    [
      new Rule('vip', F.customer.years.gt(5).and(F.since.isNull()), { discount: 0.15, label: 'vip' }, { priority: 10, tags: ['gold'], do: notify }),
      new Rule('loyal', F.customer.years.gt(2), { discount: F.base.mul(0.05) }, { priority: 5, do: 'notify' }),
    ],
    { hitPolicy: 'priority', strict: true },
  )

  test('has the core JSON form', () => {
    const summary = rules.summary()
    assert.ok(summary instanceof Summary)
    const json = summary.toJSON()
    assert.equal(json.version, 1)
    const data = json.summary
    assert.equal(summary.hitPolicy, data.hit_policy)
    assert.equal(summary.aggregate, null)
    assert.equal(summary.logic, 'boolean')
    assert.equal(summary.strict, true)
    assert.equal(summary.rules.length, data.rules.length)
    summary.rules.forEach((rule, i) => {
      assert.ok(rule instanceof RuleSummary)
      assert.deepEqual(
        { name: rule.name, priority: rule.priority, outputs: rule.outputs, action: rule.action, actionName: rule.actionName },
        {
          name: data.rules[i].name,
          priority: data.rules[i].priority,
          outputs: data.rules[i].outputs,
          action: data.rules[i].action ?? false,
          actionName: data.rules[i].do ?? null,
        },
      )
    })
    assert.deepEqual(
      summary.rules.map((rule) => [rule.action, rule.actionName]),
      [
        [true, null],
        [true, 'notify'],
      ],
    )
    summary.fields.forEach((usage, i) => {
      assert.ok(usage instanceof FieldUsage)
      assert.deepEqual(usage.path, data.fields[i].path)
      assert.deepEqual(usage.conditions, data.fields[i].conditions)
      assert.deepEqual(usage.payloads, data.fields[i].payloads.map((use) => [use.rule, use.key]))
    })
    assert.deepEqual(
      summary.fields.map((usage) => usage.path),
      [['customer', 'years'], ['since'], ['base']],
    )
    assert.deepEqual(summary.outputs, ['discount', 'label'])
    assert.deepEqual(summary.tags, { gold: ['vip'] })
    assert.ok(Object.isFrozen(summary) && Object.isFrozen(summary.rules[0]) && Object.isFrozen(summary.tags))
  })

  test('text: the core writes it, marking actions given as code', () => {
    const text = String(rules.summary())
    assert.ok(text.startsWith('Rule set: 2 rules, hit policy priority, logic boolean, strict\n'), text)
    assert.match(text, /discount, label \+action/)
    assert.match(text, /discount DO notify/)
    assert.match(text, /Tags: gold \(vip\)/)
  })
})
