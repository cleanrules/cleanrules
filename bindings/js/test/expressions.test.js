// Conditions built in code (task F.2a): `F`, `field`, `constant`, `and`,
// `or`, `not` and the methods of `Expr`, built by the core's constructors.

import assert from 'node:assert/strict'
import { describe, test } from 'node:test'
import { inspect } from 'node:util'

import { root } from './support/cr.js'

const { Const, Expr, ExpressionError, F, Field, and, constant, date, datetime, decimal, field, float, not, or } = root

/** The canonical text of a node. */
const text = (expr) => String(expr)

describe('operators and their text', () => {
  test('comparisons', () => {
    assert.equal(text(F.x.eq(1)), 'x = 1')
    assert.equal(text(F.x.ne('a')), 'x != "a"')
    assert.equal(text(F.x.lt(1.5)), 'x < 1.5e0')
    assert.equal(text(F.x.le(F.y)), 'x <= y')
    assert.equal(text(F.x.gt(2n)), 'x > 2')
    assert.equal(text(F.x.ge(decimal('0.10'))), 'x >= 0.10')
    assert.equal(F.x.ge(1).tag, 'ge')
  })

  test('connectives, as methods and as functions (fuzzytool: &, |, ~)', () => {
    assert.equal(text(F.a.and(F.b)), 'a AND b')
    assert.equal(text(F.a.or(F.b, F.c)), 'a OR b OR c')
    assert.equal(text(F.a.not()), 'NOT a')
    assert.equal(text(and(F.a.gt(1), F.b, not(F.c))), 'a > 1 AND b AND NOT c')
    assert.equal(text(or(F.a, F.b).and(F.c)), '(a OR b) AND c')
    // The core flattens nested connectives of the same kind.
    assert.ok(F.a.and(F.b).and(F.c).sameAs(and(F.a, F.b, F.c)))
    assert.equal(F.a.and(F.b, F.c).children().length, 3)
  })

  test('arithmetic', () => {
    assert.equal(text(F.price.mul(F.qty).add(1)), 'price * qty + 1')
    assert.equal(text(F.a.sub(F.b).div(2)), '(a - b) / 2')
    assert.equal(text(F.a.neg()), '-a')
    assert.equal(text(F.a.abs()), 'ABS(a)')
    assert.equal(F.a.add(1).kind, 'value')
  })

  test('predicates', () => {
    assert.equal(text(F.country.isIn(['PT', 'ES', 'ES'])), 'country IN ("ES", "PT")')
    assert.equal(text(F.country.isIn(new Set([1, 2]))), 'country IN (1, 2)')
    assert.equal(text(F.age.between(18, 65)), 'age BETWEEN 18 AND 65')
    assert.equal(F.age.between(18, 65, { closed: 'left' }).toJSON().closed, 'left')
    assert.equal(text(F.note.isNull()), 'note IS NULL')
    assert.equal(text(F.email.matches('.+@.+')), 'email MATCHES ".+@.+"')
    assert.equal(text(F.name.startsWith('A')), 'name STARTS WITH "A"')
    assert.equal(text(F.name.endsWith('z')), 'name ENDS WITH "z"')
    assert.equal(text(F.name.contains('m')), 'name CONTAINS "m"')
  })

  test('constants of every kind', () => {
    assert.equal(text(F.d.eq(date(2026, 11, 5))), 'd = DATE "2026-11-05"')
    assert.equal(text(F.d.eq(datetime(2026, 11, 5, 9, 30))), 'd = DATETIME "2026-11-05T09:30:00"')
    assert.equal(text(F.d.eq(new Date(Date.UTC(2026, 10, 5)))), 'd = DATETIME "2026-11-05T00:00:00Z"')
    assert.equal(text(F.x.eq(float(2))), 'x = 2e0')
    assert.equal(text(F.x.eq(true)), 'x = TRUE')
    assert.equal(constant(null).value, null)
    assert.equal(constant(9007199254740993n).value, 9007199254740993n)
    assert.ok(Object.is(constant(-0).value, -0))
    assert.equal(constant(decimal('1.50')).value.text, '1.50')
    assert.equal(constant(true).kind, 'any')
  })
})

describe('Expr', () => {
  test('is immutable, with its tag, kind, children, paths and JSON', () => {
    const node = F.customer.age.ge(18).and(F.country.isIn(['ES']))
    assert.ok(node instanceof Expr)
    assert.ok(Object.isFrozen(node))
    assert.equal(node.tag, 'and')
    assert.equal(node.kind, 'condition')
    assert.deepEqual(node.fieldPaths(), [['customer', 'age'], ['country']])
    const [left, right] = node.children()
    assert.ok(left.sameAs(F.customer.age.ge(18)))
    assert.ok(right.sameAs(F.country.isIn(['ES'])))
    assert.deepEqual(JSON.parse(JSON.stringify(F.x.gt(1))), {
      op: 'gt',
      left: { op: 'field', path: ['x'] },
      right: { op: 'const', value: 1 },
    })
    assert.throws(() => new Expr(), TypeError)
  })

  test('structural identity: sameAs, never ==', () => {
    assert.ok(F.x.gt(1).sameAs(F.x.gt(1)))
    assert.ok(!F.x.gt(1).sameAs(F.x.gt(1.0001)))
    // An int and a float are different constants (spec/ast.md).
    assert.ok(!F.x.gt(1).sameAs(F.x.gt(float(1))))
    assert.ok(!F.x.sameAs('x'))
  })

  test('fields and constants', () => {
    const price = field('items', 0, 'price')
    assert.ok(price instanceof Field)
    assert.deepEqual(price.path, ['items', 0, 'price'])
    assert.ok(price.sameAs(F.items.at(0).price))
    assert.ok(price.sameAs(F.items.at(0, 'price')))
    assert.equal(text(price), 'items[0].price')
    const one = constant(1)
    assert.ok(one instanceof Const)
    assert.equal(one.value, 1)
    assert.equal(one.tag, 'const')
  })

  test('JavaScript operators are refused; text where a string is wanted', () => {
    assert.throws(() => F.age >= 18, ExpressionError)
    assert.throws(() => F.age + 1, ExpressionError)
    assert.throws(() => F.age == 18, ExpressionError) // eslint-disable-line eqeqeq
    assert.equal(`${F.age.ge(18)}`, 'age >= 18')
    assert.equal(String(F), 'F')
    assert.throws(() => +F, ExpressionError)
  })

  test('inspect shows the class and the text', () => {
    assert.equal(inspect(F.x.gt(1)), 'Expr(x > 1)')
    assert.equal(inspect(F.customer.age), 'Field(customer.age)')
    assert.equal(inspect(constant(1)), 'Const(1)')
    assert.equal(inspect(F), 'F')
  })
})

describe('the F proxy', () => {
  test('then is never a field: await gives the field back', async () => {
    const age = F.customer.age
    assert.equal(age.then, undefined)
    assert.equal(F.then, undefined)
    assert.equal(await age, age)
    const awaited = await F.customer
    assert.ok(awaited.sameAs(field('customer')))
    const x = F.x
    assert.equal(await Promise.resolve(x), x)
    assert.equal(await (async () => x)(), x)
    // A field named then is reached another way.
    assert.deepEqual(field('then').path, ['then'])
    assert.deepEqual(F.x.at('then').path, ['x', 'then'])
  })

  test('symbols, toJSON, inspect and methods are the real members', () => {
    const x = F.x
    assert.equal(typeof x.gt, 'function')
    assert.equal(typeof x.toJSON, 'function')
    assert.equal(typeof x.toString, 'function')
    assert.equal(typeof x[Symbol.toPrimitive], 'function')
    assert.equal(typeof x[Symbol.for('nodejs.util.inspect.custom')], 'function')
    assert.equal(x[Symbol.iterator], undefined)
    assert.equal(x[Symbol('other')], undefined)
    assert.equal(x.constructor, Field)
    assert.ok(x instanceof Field && x instanceof Expr)
    assert.deepEqual(x.path, ['x'])
    assert.equal(x.tag, 'field')
  })

  test('names that clash with methods are reached with at() or field()', () => {
    for (const name of ['and', 'or', 'not', 'path', 'tag', 'kind', 'at', 'eq', 'isIn', 'constructor', 'toString']) {
      assert.deepEqual(F.x.at(name).path, ['x', name], name)
      assert.deepEqual(field(name).path, [name], name)
    }
    // On F itself every name is a field.
    assert.deepEqual(F.and.path, ['and'])
    assert.deepEqual(F.path.path, ['path'])
  })

  test('an index is ambiguous as a property: at(i) or field(..., i)', () => {
    assert.throws(() => F.items[0], (error) => error instanceof ExpressionError && /\.at\(0\)/.test(error.hint))
    assert.throws(() => F[0], (error) => error instanceof ExpressionError && /field\(0\)/.test(error.hint))
    assert.deepEqual(F.items.at(0).path, ['items', 0])
    assert.deepEqual(F.items.at('0').path, ['items', '0'])
    // A name that only starts with a digit is a key.
    assert.deepEqual(F.x['01'].path, ['x', '01'])
  })

  test('walk: the node and its descendants, in pre-order', () => {
    const node = F.a.gt(1).and(F.b.isNull().not())
    assert.deepEqual(
      [...node.walk()].map((n) => n.tag),
      ['and', 'gt', 'field', 'const', 'not', 'is_null', 'field'],
    )
    const paths = node.fieldPaths()
    assert.deepEqual(paths, [['a'], ['b']])
    assert.ok(Object.isFrozen(paths) && Object.isFrozen(paths[0]))
  })

  test('nodes cannot be changed, in strict and sloppy code', () => {
    // A function made with `new Function` is sloppy code.
    const sloppy = new Function('node', 'node.extra = 1')
    for (const node of [F.x, F.x.gt(1), constant(1), F]) {
      assert.throws(() => sloppy(node), /immutable; build a new one/)
      assert.throws(() => {
        node.extra = 1
      }, TypeError)
    }
  })

  test('fields cannot be changed', () => {
    const x = F.x
    assert.throws(() => {
      x.y = 1
    }, TypeError)
    assert.throws(() => {
      F.y = 1
    }, TypeError)
    assert.throws(() => {
      delete x.path
    }, TypeError)
  })
})

describe('errors, from the core and from the binding', () => {
  const expression = (call, pattern) =>
    assert.throws(call, (error) => error instanceof ExpressionError && error.code === 'expression_error' && pattern.test(error.message))

  test('constants that are not constants', () => {
    expression(() => F.x.gt(NaN), /NaN/)
    expression(() => F.x.gt(float(NaN)), /NaN/)
    expression(() => F.x.gt(2n ** 64n), /does not fit in 64 bits/)
    expression(() => F.x.gt(decimal('1e40')), /38 digits/)
    expression(() => F.x.eq(new Set([1])), /a Set cannot be a constant/)
    expression(() => F.x.eq([1, 2]), /an array cannot be a constant/)
    expression(() => F.x.eq({ a: 1 }), /cannot be a constant/)
    expression(() => F.x.eq(() => 1), /a function cannot be a constant/)
    expression(() => F.x.eq(undefined), /undefined is not a constant/)
    expression(() => constant(F.x), /cannot be a constant/)
    expression(() => F.x.eq(null), /comparing with null is never true/)
  })

  test('nodes the core refuses', () => {
    expression(() => field(), /cannot be empty/)
    expression(() => field('a', ''), /empty string/)
    expression(() => field('a', -1), /cannot be the number -1/)
    expression(() => field('a', 1.5), /cannot be the number 1.5/)
    expression(() => field(true), /cannot be a boolean/)
    expression(() => and(F.a), /at least two operands/)
    expression(() => F.a.and(), /at least two operands/)
    expression(() => F.a.add(1).and(F.b), /value/)
    expression(() => F.a.gt(1).add(1), /condition/)
    expression(() => F.a.isIn([]), /at least one value/)
    expression(() => F.a.isIn([1, 'a']), /mix type groups/)
    expression(() => F.a.isIn([null]), /null/)
    expression(() => F.a.isIn('abc'), /collection of constants/)
    expression(() => F.a.isIn(new Map()), /collection of constants/)
    expression(() => F.a.isIn([NaN]), /value 0: a constant cannot be NaN/)
    expression(() => F.a.between(true, false), /.+/)
    expression(() => F.a.between(1, 2, { closed: 'open' }), /closed must be/)
    expression(() => F.a.matches('(?=x)'), /.+/)
    expression(() => F.a.matches(5), /matches takes a string/)
    expression(() => F.a.startsWith(F.b), /startsWith takes a string/)
    expression(() => constant(null).and(F.a), /null/)
    expression(() => and(F.a, true), /JavaScript boolean/)
    expression(() => not(false), /JavaScript boolean/)
    assert.throws(() => F.a.between(1, 2, 'left'), /options must be an object/)
    assert.throws(() => F.a.between(1, 2, { close: 'left' }), /unknown option "close"; the options are closed/)
  })
})
