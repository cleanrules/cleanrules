// Evaluating one node (task F.2b): evaluateTree, evaluateValueTree and the
// tree of an action's rule, with the behaviour of Python's
// `cleanrules._trees`.

import assert from 'node:assert/strict'
import { describe, test } from 'node:test'

import cr, { root } from './support/cr.js'

const {
  ActionContext,
  EvaluationError,
  ExpressionError,
  F,
  MissingFieldError,
  Reason,
  RegistryError,
  ResultTree,
  Rule,
  RuleSet,
  UnsupportedError,
  constant,
  decimal,
  evaluateTree,
  evaluateValueTree,
} = root

/** Whether `call` throws `Class` with a message matching `pattern`. */
function throwsWith(call, Class, pattern, check = () => {}) {
  assert.throws(call, (error) => {
    assert.ok(error instanceof Class, `${error?.constructor?.name}: ${error?.message}`)
    assert.match(error.message, pattern)
    check(error)
    return true
  })
}

const INSPECT = Symbol.for('nodejs.util.inspect.custom')

describe('evaluateTree', () => {
  test('gives each node its truth, value and reason', () => {
    const condition = F.age.ge(18).and(F.country.isIn(['ES']))
    const tree = evaluateTree(condition, { age: 20 })
    assert.ok(tree instanceof ResultTree)
    assert.equal(tree.truth, 'unknown')
    assert.equal(tree.value, null)
    assert.ok(tree.node.sameAs(condition))
    assert.equal(tree.reason.code, 'missing_field')
    assert.ok(tree.reason instanceof Reason)
    const [age, country] = tree.children
    assert.deepEqual([age.truth, age.reason], ['true', null])
    assert.deepEqual(
      age.children.map((child) => [String(child.node), child.truth, child.value]),
      [
        ['age', null, 20],
        ['18', null, 18],
      ],
    )
    assert.equal(country.reason.code, 'missing_field')
    assert.deepEqual(country.reason.paths, [['country']])
    assert.deepEqual(country.children[0].reason.paths, [['country']])
    assert.equal(tree[INSPECT](), 'ResultTree(age >= 18 AND country IN ("ES"): unknown)')
    // The JSON form is the core's (spec/json-format.md, *Result trees*).
    const json = tree.toJSON()
    assert.deepEqual(json.children[0], {
      op: 'ge',
      truth: true,
      children: [
        { op: 'field', path: ['age'], value: 20 },
        { op: 'const', value: 18 },
      ],
    })
    assert.equal(json.truth, null)
    assert.equal(JSON.stringify(tree), JSON.stringify(json))
    assert.ok(Object.isFrozen(tree) && Object.isFrozen(tree.children))
    throwsWith(() => new ResultTree(), TypeError, /no public constructor/)
  })

  test('a between has its two halves; a field used as a condition has its value', () => {
    const between = evaluateTree(F.x.between(1, 5, { closed: 'right' }), { x: 7 })
    assert.deepEqual(
      between.children.map((half) => [String(half.node), half.truth]),
      [
        ['x > 1', 'true'],
        ['x <= 5', 'false'],
      ],
    )
    const field = evaluateTree(F.vip, { vip: true })
    assert.equal(field.truth, 'true')
    assert.deepEqual(field.children.map((child) => child.value), [true])
    assert.equal(evaluateTree(constant(false), null).truth, 'false')
  })

  test('values come back native, opaque objects as themselves; lists are frozen', () => {
    const set = new Set([1])
    const tree = evaluateValueTree(F.items, { items: [decimal('1.50'), set] })
    assert.equal(String(tree.value[0]), '1.50')
    assert.equal(tree.value[1], set)
    assert.ok(Object.isFrozen(tree.value) && !Object.isFrozen(set))
    assert.equal(evaluateValueTree(F.price.mul(F.quantity), { price: 2.5, quantity: 4 }).value, 10)
    const opaque = evaluateValueTree(F.thing, { thing: set })
    assert.equal(opaque.value, set)
    assert.equal(evaluateValueTree(F.x.div(0), { x: 1 }).reason.code, 'division_by_zero')
    assert.equal(evaluateValueTree(F.x.add(1), { x: 1 })[INSPECT](), 'ResultTree(x + 1: 2)')
    assert.equal(evaluateValueTree(F.name, { name: 'a' })[INSPECT](), 'ResultTree(name: "a")')
  })

  test('strict mode throws; options and arguments are checked', () => {
    throwsWith(() => evaluateTree(F.x.gt(1), {}, { strict: true }), MissingFieldError, /.+/, (error) => {
      assert.deepEqual(error.context.path, ['x'])
    })
    throwsWith(() => evaluateValueTree(F.x.add(1), { x: 'a' }, { strict: true }), EvaluationError, /.+/)
    throwsWith(() => evaluateTree(F.x.gt(1), {}, { logic: 'nope' }), RegistryError, /nope/)
    throwsWith(() => evaluateTree(F.x.gt(1), {}, { strict: 'yes' }), TypeError, /options\.strict is a boolean/)
    throwsWith(() => evaluateTree(F.x.gt(1), {}, { strict: true, mode: 1 }), TypeError, /unknown option "mode"/)
    throwsWith(() => evaluateValueTree(F.x, {}, { logic: 'boolean' }), TypeError, /unknown option "logic"/)
    throwsWith(() => evaluateTree(F.x.gt(1)), TypeError, /the input is undefined/)
    throwsWith(() => evaluateTree('x > 1', {}), ExpressionError, /evaluateTree takes a node, not the string "x > 1"/, (error) => {
      assert.match(error.hint, /F\.total\.gt\(100\)/)
    })
    throwsWith(() => evaluateValueTree(null, {}), ExpressionError, /evaluateValueTree takes a node, not null/)
    throwsWith(() => evaluateTree(F.x.mul(2), {}), ExpressionError, /evaluateTree takes a condition, not the value x \* 2/, (error) => {
      assert.equal(error.hint, 'compare it, or evaluate it with evaluateValueTree')
    })
    assert.ok(UnsupportedError)
  })

  test('a getter that evaluates inside a getter keeps its own objects', () => {
    const inner = new Set(['inner'])
    const data = {
      get x() {
        return evaluateValueTree(F.y, { y: inner }).value === inner ? 1 : 0
      },
      z: new Set(['outer']),
    }
    const tree = evaluateValueTree(F.z, data)
    assert.equal(tree.value, data.z)
    assert.equal(evaluateTree(F.x.eq(1), data).truth, 'true')
  })
})

describe('ActionContext.tree', () => {
  test('is the result tree of the rule condition, from the input already read', () => {
    const trees = []
    let reads = 0
    const data = {
      get x() {
        reads += 1
        return 3
      },
      thing: new Set(),
    }
    const rules = new RuleSet([
      new Rule('a', F.x.gt(1).and(F.thing.isNull().not()), null, {
        do: (context) => {
          assert.ok(context instanceof ActionContext)
          trees.push(context.tree, context.tree)
        },
      }),
    ])
    rules.run(data)
    assert.equal(reads, 1)
    const [tree, again] = trees
    assert.equal(tree, again)
    assert.ok(tree instanceof ResultTree)
    assert.equal(tree.truth, 'true')
    assert.ok(tree.node.sameAs(rules.rule('a').when))
    assert.equal(tree.children[0].children[0].value, 3)
    assert.equal(tree.children[1].children[0].children[0].value, data.thing)
    // Read from the native layer too, through the same module.
    assert.equal(typeof cr.FORMAT_VERSION, 'number')
  })
})
