// Decision tables (spec/tables.md): a grid of inputs, outputs and rows, as
// Python's `cleanrules._tables`:
//
//   const table = new DecisionTable(
//     ['RULE', 'customer.years', 'order.total', 'THEN discount'],
//     [['vip', '> 5', '> 1000', decimal('0.15')], ['loyal', '> 2', '-', decimal('0.05')]],
//   )
//   table.evaluate({ customer: { years: 9 }, order: { total: 1500 } }).output
//
// A table is read and written as CSV, JSON or YAML (`loads`, `dumps`), or
// built from a header and rows (`spec/tables.md`, *Grids*), and compiles into
// an ordinary `RuleSet` (`table.ruleset`), which evaluates, explains and
// summarizes it. The core reads, checks and compiles every table; this module
// hands it the grid one item at a time (`gridNew` of the native layer) and
// makes the table's objects. It is made once per build.

import { CleanRulesError } from '../errors.js'
import { decode } from '../output.js'
import { registerOpaque } from '../read.js'
import { constantWire } from './expr.js'
import { INTERNAL, TableOutput, TableRow } from './results.js'
import { brand, options, sameBuild, show } from './shared.js'

const INSPECT = Symbol.for('nodejs.util.inspect.custom')
const DENO_INSPECT = Symbol.for('Deno.customInspect')

/** Only the binding makes tables from the core's. */
const NATIVE = Symbol('cleanrules.table')

const TABLE_OPTIONS = ['hitPolicy', 'aggregate', 'logic', 'strict', 'limits']

const iterable = (value) => value !== null && typeof value === 'object' && typeof value[Symbol.iterator] === 'function'

/**
 * Call `make`, which converts a value of the grid; a `CleanRulesError` it
 * throws without a pointer is thrown again at `pointer` (as Python locates
 * the values it cannot convert).
 */
function located(make, pointer) {
  try {
    return make()
  } catch (error) {
    if (!(error instanceof CleanRulesError) || error.context.pointer !== undefined) throw error
    const given = { code: error.code, hint: error.hint, context: { ...error.context, pointer } }
    if (error.cause !== undefined) given.cause = error.cause
    throw new error.constructor(error.message, given)
  }
}

/** The decision tables of one build (`build` names it). */
function tables(native, expressions, rules, build) {
  const { Expr, nodeOf, wrap } = expressions
  // The native table of each `DecisionTable`.
  const natives = new WeakMap()

  /** A table of the core built from the grid `header` and `rows`. */
  function grid(header, rows, tableOptions) {
    const { hitPolicy, aggregate, logic, strict, limits } = options(tableOptions, TABLE_OPTIONS, "{ hitPolicy: 'priority' }")
    const builder = native.gridNew(hitPolicy, aggregate, logic, strict, limits)
    try {
      if (typeof header === 'string') {
        throw new TypeError('the header is an array of columns, not a string')
      }
      if (!iterable(header)) {
        throw new TypeError(`the header is an array of columns, not ${show(header)}`)
      }
      let c = 0
      for (const item of header) {
        if (typeof item === 'string') {
          builder.headerText(item)
        } else if (item instanceof Expr) {
          builder.headerInput(nodeOf(item))
        } else if (item instanceof TableOutput) {
          const forms = located(
            () => item.values.map((value, i) => {
              sameBuild(value, build)
              return constantWire(value, `value ${i}: `)
            }),
            `/header/${c}`,
          )
          builder.headerOutput(item.name, JSON.stringify(['l', forms]))
        } else {
          sameBuild(item, build)
          throw new TypeError(`a header item is a string, an Expr or a TableOutput, not ${show(item)}`)
        }
        c += 1
      }
      builder.start()
      if (typeof rows === 'string' || !iterable(rows)) {
        throw new TypeError(`the rows are an array of rows, not ${show(rows)}`)
      }
      let r = 0
      for (const row of rows) {
        if (typeof row === 'string') {
          throw new TypeError(`row ${r} is a string; a row is an array of cells`)
        }
        if (!iterable(row)) {
          throw new TypeError(`row ${r} is an array of cells, not ${show(row)}`)
        }
        let column = 0
        for (const cell of row) {
          if (typeof cell === 'string') {
            builder.cellText(cell)
          } else if (cell instanceof Expr) {
            builder.cellNode(nodeOf(cell))
          } else if (cell === null || cell === undefined) {
            // An empty cell (`undefined` too: a hole in a row).
            builder.cellValue('null')
          } else {
            sameBuild(cell, build)
            const form = located(() => constantWire(cell), `/rows/${r}/${column}`)
            builder.cellValue(JSON.stringify(form))
          }
          column += 1
        }
        builder.endRow()
        r += 1
      }
      return builder.finish()
    } finally {
      builder.free?.()
    }
  }

  /**
   * A decision table and the rule set it compiles into (`spec/tables.md`);
   * immutable.
   */
  class DecisionTable {
    #native
    #rows
    #ruleset

    /**
     * @param header The columns: a header field's text (`'RULE'`,
     *   `'PRIORITY'`, `'DESCRIPTION'`, `'THEN key'`, `"THEN key IN (...)"` or an
     *   input expression), an input `Expr`, or a `TableOutput`.
     * @param rows The rows, each an array of cells in column order, read one
     *   at a time: a string is read as in the table's CSV (`'> 5'`, `'-'`,
     *   `"'VIP'"` for the string), an `Expr` is an input's condition or an
     *   output's value, `null` and `undefined` are empty, and any other value
     *   is a constant (under an input `x`, the test `x = value`).
     * @param options `hitPolicy` (`'first'`), `aggregate`, `logic`
     *   (`'boolean'`), `strict` (`false`) and `limits` (`maxSize` for each
     *   text, `maxDepth`, `maxTableSize`).
     */
    constructor(header, rows, tableOptions) {
      const made = header === NATIVE ? rows : grid(header, rows, tableOptions)
      this.#native = made
      const [hitPolicy, aggregate, logic, strict] = JSON.parse(made.config())
      /** The hit policy: one of a rule set's, or `'output_order'`. */
      this.hitPolicy = hitPolicy
      /** The `collect` aggregate, or `null`. */
      this.aggregate = aggregate
      /** The logic's name. */
      this.logic = logic
      /** Whether evaluation is strict. */
      this.strict = strict
      /** The input expressions, in column order (frozen). */
      this.inputs = Object.freeze(made.inputs().map(wrap))
      /** The output columns (frozen). */
      this.outputs = Object.freeze(
        JSON.parse(made.outputs()).map(([name, values]) => new TableOutput(name, values.map((value) => decode(value, [])))),
      )
      natives.set(this, made)
      Object.freeze(this)
    }

    /** The rows, in order (frozen; made when first read). */
    get rows() {
      if (this.#rows === undefined) {
        const nodes = this.#native.rowNodes()
        let next = 0
        const cells = (present) =>
          present.map((has) => {
            if (!has) return null
            next += 1
            return wrap(nodes[next - 1])
          })
        this.#rows = Object.freeze(
          JSON.parse(this.#native.rows()).map(
            ([name, priority, description, inputs, outputs]) =>
              new TableRow(INTERNAL, name, priority === null ? null : decode(priority, []), description, cells(inputs), cells(outputs)),
          ),
        )
      }
      return this.#rows
    }

    /** The rule set the table compiles into: one rule per row (`spec/tables.md`). */
    get ruleset() {
      this.#ruleset ??= rules.fromNativeRuleSet(this.#native.ruleset())
      return this.#ruleset
    }

    /** Evaluate one input with the compiled rule set (`RuleSet.evaluate`, with its options). */
    evaluate(data, evaluateOptions) {
      return this.ruleset.evaluate(data, evaluateOptions)
    }

    /** Evaluate a batch with the compiled rule set (`RuleSet.evaluateMany`, with its options). */
    evaluateMany(rows, evaluateOptions) {
      return this.ruleset.evaluateMany(rows, evaluateOptions)
    }

    /** The summary of the compiled rule set (`RuleSet.summary`). */
    summary() {
      return this.ruleset.summary()
    }

    /**
     * The decision table document of the JSON format (`spec/tables.md`,
     * *JSON*), for `JSON.stringify` (`Expr.toJSON` says how); `dumps(table)`
     * writes it exactly.
     */
    toJSON() {
      return JSON.parse(native.stringifiable(this.#native.write('json')))
    }

    [INSPECT]() {
      const columns = this.inputs.length + this.outputs.length
      return `DecisionTable(${this.hitPolicy}: ${this.#native.rowCount} rows, ${columns} columns)`
    }

    [DENO_INSPECT]() {
      return this[INSPECT]()
    }
  }

  registerOpaque(DecisionTable)
  brand(build, DecisionTable)

  return {
    DecisionTable,
    /** The `DecisionTable` of a table of the core. */
    fromNativeTable: (made) => new DecisionTable(NATIVE, made),
    /** The table of the core of a `DecisionTable`. */
    tableNative: (table) => natives.get(table),
  }
}

export { tables }
