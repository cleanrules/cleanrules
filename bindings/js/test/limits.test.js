// The `max_values` limit on native data (spec/data-model.md, *Reading native
// data*, *Limits*): an input beyond it is refused with the core's
// `limit_exceeded`, in every mode, in the native layer and the idiomatic API.

import assert from 'node:assert/strict'
import { describe, test } from 'node:test'

import cr, { root } from './support/cr.js'

const { F, FormatError, Rule, RuleSet, DecisionTable, constant, evaluateTree, evaluateValueTree } = root

/** An array holding the same array twice, `depth` times over: 2 ** depth references. */
function diamond(depth) {
  let node = 1
  for (let i = 0; i < depth; i += 1) {
    node = [node, node]
  }
  return node
}

/** Whether `call` throws the core's `limit_exceeded` for `max_values`; `check` sees the error. */
function beyond(call, check = () => {}) {
  assert.throws(call, (error) => {
    assert.ok(error instanceof FormatError, String(error))
    assert.equal(error.code, 'limit_exceeded')
    assert.equal(error.context.name, 'max_values')
    assert.match(error.message, /max_values/)
    assert.ok(error.hint)
    check(error)
    return true
  })
}

const truths = (decision) => decision.results.map((r) => r.truth)

describe('max_values', () => {
  test('a diamond of depth 30 is refused within the default limit, in every mode', () => {
    const rules = new RuleSet([new Rule('r', F.d.isNull()), new Rule('s', F.x.eq(1))])
    const started = Date.now()
    beyond(
      () => rules.evaluate({ d: diamond(30), x: 1 }),
      (error) => {
        assert.equal(error.context.rule, undefined)
        assert.equal(error.context.row, undefined)
      },
    )
    // A million values, not a thousand million.
    assert.ok(Date.now() - started < 30_000)
    const strict = new RuleSet([new Rule('r', F.d.isNull())], { strict: true })
    beyond(() => strict.evaluate({ d: diamond(30) }))
  })

  test('the count is exact', () => {
    // diamond(3): 8 ints and 7 lists, plus the int at `x`: 16 values.
    const rules = new RuleSet([new Rule('r', F.d.isNull()), new Rule('s', F.x.eq(1))])
    const data = { d: diamond(3), x: 1 }
    assert.deepEqual(truths(rules.evaluate(data, { limits: { maxValues: 16 } })), ['false', 'true'])
    beyond(() => rules.evaluate(data, { limits: { maxValues: 15 } }))
    // A shared object counts each time it is converted.
    const both = new RuleSet([new Rule('a', F.a.isNull()), new Rule('b', F.b.isNull())])
    const shared = diamond(3)
    assert.equal(both.evaluate({ a: shared, b: shared }, { limits: { maxValues: 30 } }).results[1].truth, 'false')
    beyond(() => both.evaluate({ a: shared, b: shared }, { limits: { maxValues: 29 } }))
    // A list along the paths counts its length, and what is read at a position.
    const second = new RuleSet([new Rule('r', F.xs.at(1).eq(20))])
    assert.equal(second.evaluate({ xs: [10, 20, 30] }, { limits: { maxValues: 4 } }).results[0].truth, 'true')
    beyond(() => second.evaluate({ xs: [10, 20, 30] }, { limits: { maxValues: 3 } }))
    // Holes of a list converted whole count: the list, two holes and `b`.
    // eslint-disable-next-line no-sparse-arrays
    const holes = { a: [, ,], b: 1 }
    assert.deepEqual(truths(both.evaluate(holes, { limits: { maxValues: 4 } })), ['false', 'false'])
    beyond(() => both.evaluate(holes, { limits: { maxValues: 3 } }))
    // A read error from a getter is a value placed in the tree.
    const failing = new RuleSet([new Rule('r', F.f.broken.gt(1))])
    const getter = {
      f: {
        get broken() {
          throw new Error('no')
        },
      },
    }
    assert.equal(failing.evaluate(getter, { limits: { maxValues: 1 } }).results[0].truth, 'unknown')
    beyond(() => failing.evaluate(getter, { limits: { maxValues: 0 } }))
    // The default allows a million values: 2 ** 19 - 1, not 2 ** 20 - 1.
    const d = new RuleSet([new Rule('r', F.d.isNull())])
    assert.equal(d.evaluate({ d: diamond(18) }).results[0].truth, 'false')
    beyond(() => d.evaluate({ d: diamond(19) }))
  })

  test('a huge declared length fails fast', () => {
    const huge = []
    huge.length = 2 ** 32 - 1
    // Along the paths, before the list is allocated.
    beyond(() => new RuleSet([new Rule('r', F.a.at(0).isNull())]).evaluate({ a: huge }))
    // Where a path ends, before its elements are read.
    beyond(() => new RuleSet([new Rule('r', F.a.isNull())]).evaluate({ a: huge }))
  })

  test('a batch reports the row of the first input beyond the limit', () => {
    const one = new RuleSet([new Rule('s', F.x.eq(1))])
    beyond(
      () => one.evaluateMany([{ x: 1 }, { x: [1, 1, 1] }, { x: 1 }], { limits: { maxValues: 2 } }),
      (error) => assert.equal(error.context.row, 1),
    )
    assert.deepEqual(
      one.evaluateMany([{ x: 1 }, { x: [1] }], { limits: { maxValues: 2 } }).map((d) => d.results[0].truth),
      ['true', 'unknown'],
    )
    // Read before evaluating: the limit comes before an evaluation error.
    const strict = new RuleSet([new Rule('s', F.y.eq(1))], { strict: true })
    beyond(
      () => strict.evaluateMany([{}, { y: [1, 1, 1] }], { limits: { maxValues: 2 } }),
      (error) => assert.equal(error.context.row, 1),
    )
  })

  test('runs, tables, trees, constants and metadata take the limit too', () => {
    const ran = []
    const rules = new RuleSet([new Rule('r', F.x.isNull(), {}, { do: () => ran.push('r') })])
    beyond(() => rules.run({ x: diamond(30) }))
    // diamond(2): 4 ints and 3 lists.
    rules.run({ x: diamond(2) }, { limits: { maxValues: 7 } })
    beyond(() => rules.run({ x: diamond(2) }, { limits: { maxValues: 6 } }))
    assert.deepEqual(ran, [])
    const table = new DecisionTable(['x'], [['> 0']])
    beyond(() => table.evaluate({ x: [1] }, { limits: { maxValues: 1 } }))
    beyond(() => table.evaluateMany([{ x: [1] }], { limits: { maxValues: 1 } }))
    beyond(() => evaluateTree(F.x.eq(1), { x: diamond(30) }))
    beyond(() => evaluateTree(F.x.eq(1), { x: diamond(2) }, { limits: { maxValues: 6 } }))
    assert.ok(evaluateTree(F.x.eq(1), { x: diamond(2) }, { limits: { maxValues: 7 } }).reason)
    beyond(() => evaluateValueTree(F.x, { x: diamond(2) }, { limits: { maxValues: 6 } }))
    // Metadata and constants, under the default limit, naming the rule when known.
    beyond(
      () => new Rule('big', F.x.eq(1), {}, { metadata: { m: diamond(20) } }),
      (error) => assert.equal(error.context.rule, 'big'),
    )
    beyond(() => constant(diamond(20)))
    beyond(
      () => new Rule('paid', F.x.eq(1), { m: diamond(20) }),
      (error) => assert.equal(error.context.rule, 'paid'),
    )
    assert.throws(() => rules.evaluate({}, { limit: 1 }), TypeError)
    assert.throws(() => rules.evaluate({}, { limits: { maxValues: -1 } }), RangeError)
  })

  test('the native layer takes the limits as its last argument', () => {
    const rules = cr.RuleSet.read('IF x == 1', { format: 'text' })
    assert.equal(rules.evaluate({ x: 1 }, { maxValues: 1 }).truth(0), 'true')
    assert.equal(rules.evaluate({ x: 1 }).truth(0), 'true')
    beyond(() => rules.evaluate({ x: 1 }, { maxValues: 0 }))
    beyond(
      () => rules.evaluateMany([{ x: 1 }, { x: 1 }], { maxValues: 0 }),
      (error) => assert.equal(error.context.row, 0),
    )
    const node = cr.Node.read('x', { format: 'text' })
    assert.ok(node.evaluateValueTree({ x: [1, 1] }, { limits: { maxValues: 3 } }))
    beyond(() => node.evaluateValueTree({ x: [1, 1] }, { limits: { maxValues: 2 } }))
    beyond(() => rules.run({ x: 1 }, {}, { maxValues: 0 }))
  })
})
