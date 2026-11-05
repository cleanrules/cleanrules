// Conditions as data (spec/ast.md): the `F` proxy, `field`, `constant`,
// `and`, `or`, `not`, and the methods of `Expr`. A condition is a tree of
// nodes that the core evaluates later, never a JavaScript function, so the
// same rule can be explained, written and audited.
//
//   F.customer.age.ge(18).and(F.country.isIn(['ES', 'PT']))
//
// Every node is built by the core's constructors (`src/common/build.rs`),
// which apply the construction rules, so a bad node fails here, at once, with
// an `ExpressionError`. This module adds the JavaScript side: methods instead
// of operators (fuzzytool's `&`, `|` and `~` are `.and()`, `.or()` and
// `.not()`), and clear errors for what cannot work on nodes. It is made once
// per build (`expressions(native, build)`), whose native nodes it wraps.

import { ExpressionError } from '../errors.js'
import { decode } from '../output.js'
import { encodeData, registerOpaque } from '../read.js'
import { brand, options, sameBuild, show } from './shared.js'

const INSPECT = Symbol.for('nodejs.util.inspect.custom')
const DENO_INSPECT = Symbol.for('Deno.customInspect')

/** Only the binding makes nodes. */
const PRIVATE = Symbol('cleanrules.node')

/** A property name that looks like an index. */
const INDEX = /^(0|[1-9][0-9]*)$/

/** The error of `F.items[0]`: a property name is always a string in JavaScript. */
function ambiguous(at, key, how) {
  const call = how === 'field' ? ['field(', ')'] : ['.at(', ')']
  return new ExpressionError(`${at}: a property name is a key in JavaScript, so an index is ambiguous`, {
    hint: `use ${call[0]}${key}${call[1]} for an index, or ${call[0]}'${key}'${call[1]} for a key named "${key}"`,
  })
}

/** The error of changing a node, in strict and sloppy code alike. */
function immutable() {
  return new TypeError('condition nodes are immutable; build a new one')
}

const COMPARE_SYMBOLS = { eq: '==', ne: '!=', lt: '<', le: '<=', gt: '>', ge: '>=' }
const CLOSED = ['both', 'left', 'right', 'none']

/** The `ExpressionError` of a constant read as `form`, or `null`. */
function constantProblem(form, thrown, value) {
  if (form === null || typeof form !== 'object') return null
  const kinds = {
    NaN: ['a constant cannot be NaN', 'test for missing values with .isNull()'],
    X: [
      `${show(value)} cannot be a constant`,
      'constants are null, booleans, numbers, bigints, strings, decimal(), date(), datetime(), Date and Temporal values',
    ],
    IO: [`${show(value)} does not fit in 64 bits`, 'use decimal(...) for larger exact numbers'],
    E: [`reading the constant threw ${form[1]}`, 'pass a plain value'],
    l: [`${show(value)} cannot be a constant`, 'constants are scalars; test membership with .isIn([...])'],
    O: [`${show(value)} cannot be a constant`, 'constants are scalars, never objects or maps'],
  }
  const found = kinds[form[0]]
  if (found === undefined) return null
  const problem = { hint: found[1] }
  if (form[0] === 'E') problem.cause = thrown[form[2]]
  return new ExpressionError(found[0], problem)
}

/** The reader's encoding of the constant `value`; throws for what is not one. */
function constantWire(value, what = '') {
  if (value === undefined) {
    throw new ExpressionError(`${what}undefined is not a constant`, { hint: 'use null for a null constant' })
  }
  const [form, thrown] = encodeData(value)
  const problem = constantProblem(form, thrown, value)
  if (problem !== null) {
    if (what === '') throw problem
    throw new ExpressionError(`${what}${problem.message}`, { hint: problem.hint, cause: problem.cause })
  }
  return form
}

/** A frozen copy of field paths. */
function frozenPaths(paths) {
  return Object.freeze(paths.map((path) => Object.freeze(path)))
}

/** The conditions and their builders of one build (`build` names it), over its native layer. */
function expressions(native, build) {
  // The native node of each `Expr` and of its proxy.
  const nodes = new WeakMap()
  // `[tag, kind, detail]` of each native node, read once.
  const details = new WeakMap()

  function nodeOf(expr) {
    const node = nodes.get(expr)
    if (node === undefined) {
      sameBuild(expr, build)
      throw new TypeError(`not a condition node: ${show(expr)}`)
    }
    return node
  }

  function detailsOf(expr) {
    const node = nodeOf(expr)
    let found = details.get(node)
    if (found === undefined) {
      found = JSON.parse(native.nodeDetails(node))
      details.set(node, found)
    }
    return found
  }

  /** `value` if it is a node, else a constant. */
  function lift(value) {
    return value instanceof Expr ? value : constant(value)
  }

  /** An operand of `and`, `or` and `not`. */
  function liftCondition(value) {
    if (typeof value === 'boolean') {
      throw new ExpressionError(`cannot combine a condition with the JavaScript boolean ${value}`, {
        hint: 'compare fields with .eq(...) (not === or ==); for a constant condition write constant(true)',
      })
    }
    return lift(value)
  }

  /** The nodes of `exprs` in a native list (wasm-bindgen passes them one by one). */
  function withList(exprs, use) {
    const list = new native.NodeList()
    try {
      for (const expr of exprs) list.push(nodeOf(expr))
      return use(list)
    } finally {
      list.free?.()
    }
  }

  function compare(op, left, right) {
    if (right === null) {
      throw new ExpressionError(`comparing with null is never true: ${left} ${COMPARE_SYMBOLS[op]} null`, {
        hint: 'missing values are unknown; test for them with .isNull()',
      })
    }
    return wrap(native.nodeCompare(op, nodeOf(left), nodeOf(lift(right))))
  }

  function arith(op, left, right) {
    return wrap(native.nodeArith(op, nodeOf(left), nodeOf(lift(right))))
  }

  function text(value, method) {
    if (typeof value !== 'string') {
      throw new ExpressionError(`${method} takes a string, not ${show(value)}`, {
        hint: 'the text is a literal (or, for matches, a pattern), never a node',
      })
    }
    return value
  }

  /** A node of a condition tree, immutable. Build nodes with `F`, `field`, `constant` and methods. */
  class Expr {
    constructor(token) {
      if (token !== PRIVATE) {
        throw new TypeError('Expr has no public constructor; build conditions with F, field(), constant(), and(), or() and not()')
      }
    }

    /** The node's tag, such as `"gt"` or `"and"` (`spec/ast.md`, *Catalogue*). */
    get tag() {
      return detailsOf(this)[0]
    }

    /** Whether the node is a `condition`, a `value`, or `any` (it depends on the data). */
    get kind() {
      return detailsOf(this)[1]
    }

    /** The direct child nodes, in order. */
    children() {
      return Object.freeze(native.nodeChildren(nodeOf(this)).map(wrap))
    }

    /** This node and all its descendants, in pre-order (parents first). */
    *walk() {
      const stack = [this]
      while (stack.length > 0) {
        const node = stack.pop()
        yield node
        stack.push(...[...node.children()].reverse())
      }
    }

    /** The paths of every field the node reads, in order of first appearance. */
    fieldPaths() {
      return frozenPaths(nodeOf(this).fieldPaths())
    }

    /** Whether `other` is a node with the same structure, operators and constants. */
    sameAs(other) {
      if (!(other instanceof Expr)) {
        sameBuild(other, build)
        return false
      }
      return native.nodeSameAs(nodeOf(this), nodeOf(other))
    }

    /** The canonical text (`spec/text.md`, *Printing*), as explanations write it: `x > 5 AND NOT vip`. */
    toString() {
      return nodeOf(this).write('text')
    }

    /**
     * The node in the JSON format (`spec/json-format.md`, *Conditions*).
     * Floats that hold an integer are `{"$float": n}` (`stringifiable`), so
     * `JSON.stringify` writes the same node, but for the sign of a float zero
     * and the order of integer-like keys, which plain objects list first.
     */
    toJSON() {
      return JSON.parse(native.stringifiable(nodeOf(this).write('json')))
    }

    /**
     * Its text where a string is wanted (`${node}`); anywhere else a node is
     * not a JavaScript value: `F.age >= 18` would compare objects, not build
     * a condition.
     */
    [Symbol.toPrimitive](hint) {
      if (hint === 'string') return this.toString()
      throw new ExpressionError(`a condition node is not a JavaScript ${hint === 'number' ? 'number' : 'value'}: ${this.toString()}`, {
        hint: 'compare with .eq(), .gt()...; combine with .and(), .or(), .not(); compute with .add()...; get its text with String(node)',
      })
    }

    [INSPECT]() {
      return `${this.constructor.name}(${this.toString()})`
    }

    [DENO_INSPECT]() {
      return this[INSPECT]()
    }

    // --- Comparisons ---

    /** `this = other`: equal values (a type mismatch is unknown). */
    eq(other) {
      return compare('eq', this, other)
    }

    /** `this != other`. */
    ne(other) {
      return compare('ne', this, other)
    }

    /** `this < other`. */
    lt(other) {
      return compare('lt', this, other)
    }

    /** `this <= other`. */
    le(other) {
      return compare('le', this, other)
    }

    /** `this > other`. */
    gt(other) {
      return compare('gt', this, other)
    }

    /** `this >= other`. */
    ge(other) {
      return compare('ge', this, other)
    }

    // --- Connectives ---

    /** `this AND others...` (fuzzytool's `&`). */
    and(...others) {
      return connective('and', [this, ...others])
    }

    /** `this OR others...` (fuzzytool's `|`). */
    or(...others) {
      return connective('or', [this, ...others])
    }

    /** `NOT this` (fuzzytool's `~`). */
    not() {
      return wrap(native.nodeUnary('not', nodeOf(this)))
    }

    // --- Arithmetic ---

    /** `this + other`. */
    add(other) {
      return arith('add', this, other)
    }

    /** `this - other`. */
    sub(other) {
      return arith('sub', this, other)
    }

    /** `this * other`. */
    mul(other) {
      return arith('mul', this, other)
    }

    /** `this / other`. */
    div(other) {
      return arith('div', this, other)
    }

    /** `-this`. */
    neg() {
      return wrap(native.nodeUnary('neg', nodeOf(this)))
    }

    /** The absolute value. */
    abs() {
      return wrap(native.nodeUnary('abs', nodeOf(this)))
    }

    // --- Predicates ---

    /** Membership in constants of one type group: `F.country.isIn(['ES', 'PT'])`. */
    isIn(values) {
      const iterable = values !== null && typeof values === 'object' && typeof values[Symbol.iterator] === 'function'
      if (!iterable || values instanceof Map) {
        throw new ExpressionError(`isIn needs a collection of constants, not ${show(values)}`, {
          hint: "e.g. F.country.isIn(['ES', 'PT'])",
        })
      }
      const forms = Array.from(values, (value, i) => {
        sameBuild(value, build)
        return constantWire(value, `isIn: value ${i}: `)
      })
      return wrap(native.nodeIsIn(nodeOf(this), JSON.stringify(['l', forms])))
    }

    /** A range test: `F.age.between(18, 65)`, ends included unless `closed` says otherwise. */
    between(low, high, betweenOptions) {
      const { closed = 'both' } = options(betweenOptions, ['closed'], "{ closed: 'left' }")
      if (!CLOSED.includes(closed)) {
        throw new ExpressionError(`closed must be 'both', 'left', 'right' or 'none', not ${show(closed)}`)
      }
      return wrap(native.nodeBetween(nodeOf(this), nodeOf(lift(low)), nodeOf(lift(high)), closed))
    }

    /** Whether the value is missing or null; never unknown. */
    isNull() {
      return wrap(native.nodeUnary('is_null', nodeOf(this)))
    }

    /** A regular expression search (the Rust `regex` syntax: linear time, no lookaround). */
    matches(pattern) {
      return wrap(native.nodeMatches(nodeOf(this), text(pattern, 'matches')))
    }

    /** Whether a string starts with the literal `prefix`. */
    startsWith(prefix) {
      return wrap(native.nodeStringTest('starts_with', nodeOf(this), text(prefix, 'startsWith')))
    }

    /** Whether a string ends with the literal `suffix`. */
    endsWith(suffix) {
      return wrap(native.nodeStringTest('ends_with', nodeOf(this), text(suffix, 'endsWith')))
    }

    /** Whether a string contains the literal `part` (not list membership: that is `isIn`). */
    contains(part) {
      return wrap(native.nodeStringTest('contains', nodeOf(this), text(part, 'contains')))
    }
  }

  /** A value looked up in the input by path: `F.customer.age`, `field('items', 0, 'price')`. */
  class Field extends Expr {
    /** The keys (strings) and indices (numbers) of the path, outermost first. */
    get path() {
      return Object.freeze([...detailsOf(this)[2]])
    }

    /**
     * The field one or more segments further: `F.items.at(0)` for an index,
     * `F.x.at('and')` for a key that is also a method's name.
     */
    at(...segments) {
      return field(...detailsOf(this)[2], ...segments)
    }
  }

  /** A constant: null, a boolean, a number, a bigint, a string, a decimal, a date or a datetime. */
  class Const extends Expr {
    /** The constant, as a JavaScript value (`spec/data-model.md`, *Values given back*). */
    get value() {
      return decode(detailsOf(this)[2], [])
    }
  }

  const refuse = {
    set() {
      throw immutable()
    },
    defineProperty() {
      throw immutable()
    },
    deleteProperty() {
      throw immutable()
    },
  }

  // Every node is a proxy, so that changing one throws in sloppy code too; a
  // field's properties are also fields one key further (`F.a.b`), except its
  // own members.
  const nodeHandler = { ...refuse }
  const fieldHandler = {
    ...refuse,
    get(target, key, receiver) {
      if (typeof key === 'symbol' || key in target) {
        return Reflect.get(target, key, receiver)
      }
      // `await field` and promise resolution look for `then`: a field is not
      // a promise.
      if (key === 'then') return undefined
      if (INDEX.test(key)) throw ambiguous(`${detailsOf(target)[2].join('.')}[${key}]`, key, '.at')
      return field(...detailsOf(target)[2], key)
    },
  }

  /** The `Expr` of a native node. */
  function wrap(node) {
    const found = JSON.parse(native.nodeDetails(node))
    details.set(node, found)
    const [tag] = found
    const Class = tag === 'field' ? Field : tag === 'const' ? Const : Expr
    const expr = new Class(PRIVATE)
    Object.freeze(expr)
    nodes.set(expr, node)
    const proxy = new Proxy(expr, Class === Field ? fieldHandler : nodeHandler)
    nodes.set(proxy, node)
    return proxy
  }

  /**
   * The `Expr` of a node's JSON text (an explanation's cause), read as the
   * core wrote it: through `JSON.parse`, a float constant such as `0.0`
   * would come back as the int `0`.
   */
  function wrapJson(text) {
    return wrap(native.Node.read(text))
  }

  /** A field by its path: keys (strings) and indices (whole numbers from 0). */
  function field(...path) {
    return wrap(native.nodeField(path))
  }

  /** A constant: `constant(true)`, `constant(decimal('0.10'))`. Values next to a node become constants by themselves. */
  function constant(value) {
    sameBuild(value, build)
    return wrap(native.nodeConstant(JSON.stringify(constantWire(value))))
  }

  function connective(tag, operands) {
    const exprs = operands.map(liftCondition)
    return withList(exprs, (list) => wrap(native.nodeConnective(tag, list)))
  }

  /** `a AND b AND ...`: at least two conditions. */
  function and(...operands) {
    return connective('and', operands)
  }

  /** `a OR b OR ...`: at least two conditions. */
  function or(...operands) {
    return connective('or', operands)
  }

  /** `NOT x`. */
  function not(operand) {
    return wrap(native.nodeUnary('not', nodeOf(liftCondition(operand))))
  }

  // `F`: every property starts a field path, `then` excepted (so that `await`
  // and promise resolution never take it for a promise).
  const root = Object.freeze(
    Object.create(null, {
      [Symbol.toPrimitive]: {
        value(hint) {
          if (hint === 'string') return 'F'
          throw new ExpressionError('F is not a JavaScript value', { hint: 'start a field path with F.name' })
        },
      },
      [Symbol.toStringTag]: { value: 'F' },
      [INSPECT]: { value: () => 'F' },
      [DENO_INSPECT]: { value: () => 'F' },
    }),
  )
  const F = new Proxy(root, {
    ...refuse,
    get(target, key) {
      if (typeof key === 'symbol') return Reflect.get(target, key)
      if (key === 'then') return undefined
      if (INDEX.test(key)) throw ambiguous(`F[${key}]`, key, 'field')
      return field(key)
    },
  })

  registerOpaque(Expr)
  brand(build, Expr)

  return { Const, Expr, F, Field, and, constant, field, lift, nodeOf, not, or, wrap, wrapJson }
}

export { constantWire, expressions }
