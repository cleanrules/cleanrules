// Decision tables (task F.2b): read and written as CSV, JSON and YAML, built
// from a grid, with the behaviour of Python's `cleanrules._tables` and its
// tests (bindings/python/tests/test_tables.py).

import assert from 'node:assert/strict'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { describe, test } from 'node:test'

import { root } from './support/cr.js'

const {
  CleanRulesError,
  DecisionTable,
  Expr,
  ExpressionError,
  F,
  FormatError,
  RegistryError,
  RuleDefinitionError,
  RuleSet,
  RuleSyntaxError,
  TableOutput,
  TableRow,
  UnsupportedError,
  date,
  decimal,
  dump,
  dumps,
  load,
  loads,
} = root

const EXAMPLE = path.join(import.meta.dirname, '..', '..', '..', 'docs', 'examples', 'risk.csv')
const EXAMPLE_TEXT = fs.readFileSync(EXAMPLE, 'utf8')

/** Whether `call` throws `Class` with a message matching `pattern`. */
function throwsWith(call, Class, pattern, check = () => {}) {
  assert.throws(call, (error) => {
    assert.ok(error instanceof Class, `${error?.constructor?.name}: ${error?.message}`)
    assert.match(error.message, pattern)
    check(error)
    return true
  })
}

const risk = () => loads(EXAMPLE_TEXT, { format: 'csv' })
const INSPECT = Symbol.for('nodejs.util.inspect.custom')

describe('DecisionTable', () => {
  test('a table compiles into a rule set', () => {
    const table = risk()
    assert.ok(table instanceof DecisionTable)
    assert.deepEqual([table.hitPolicy, table.aggregate, table.logic, table.strict], ['priority', null, 'boolean', false])
    assert.deepEqual(table.inputs.map(String), ['customer.years', 'order.total'])
    assert.ok(table.inputs[0] instanceof Expr)
    assert.ok(table.outputs[1] instanceof TableOutput)
    assert.deepEqual([table.outputs[1].name, table.outputs[1].values], ['risk', ['high', 'medium', 'low']])
    assert.deepEqual([table.outputs[0].name, table.outputs[0].values], ['discount', []])
    const first = table.rows[0]
    assert.ok(first instanceof TableRow)
    assert.deepEqual([first.name, first.priority, first.description], ['new', null, 'New customers with large orders are checked first'])
    assert.ok(first.inputs[0].sameAs(F.customer.years.lt(1)))
    assert.equal(table.rows[2].inputs[0], null)
    assert.equal(table.rows.length, 4)
    assert.equal(table.rows, table.rows)
    assert.ok(table.ruleset instanceof RuleSet)
    assert.equal(table.ruleset, table.ruleset)
    // Output priorities become rule priorities: high, then medium, then low.
    assert.deepEqual(
      table.ruleset.rules.map((rule) => [rule.name, rule.priority]),
      [
        ['new', 3],
        ['vip', 1],
        ['big', 2],
        ['default', 1],
      ],
    )
    const decision = table.evaluate({ customer: { years: 9 }, order: { total: 6000 } })
    assert.deepEqual([String(decision.output.discount), decision.output.risk], ['0.05', 'medium'])
    assert.ok(String(decision.explain()).startsWith('HIT POLICY priority: selected big\n'))
    assert.equal(table[INSPECT](), 'DecisionTable(priority: 4 rows, 4 columns)')
  })

  test('the example is canonical, in every format and file', async (t) => {
    const table = risk()
    assert.equal(dumps(table, { format: 'csv' }), EXAMPLE_TEXT)
    assert.equal(dumps(table), JSON.stringify(table.toJSON()))
    assert.equal(table.toJSON().version, 1)
    const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'cleanrules-'))
    t.after(() => fs.rmSync(dir, { recursive: true, force: true }))
    for (const name of ['risk.json', 'risk.yaml', 'risk.csv']) {
      await dump(table, path.join(dir, name))
      const back = await load(path.join(dir, name))
      assert.ok(back instanceof DecisionTable)
      assert.equal(dumps(back), dumps(table))
    }
    assert.equal(fs.readFileSync(path.join(dir, 'risk.csv'), 'utf8'), EXAMPLE_TEXT)
    assert.ok((await load(EXAMPLE)) instanceof DecisionTable)
  })

  test('CSV and JSON errors have lines, columns and pointers', () => {
    throwsWith(() => loads('RULE,x\na,>  \n', { format: 'csv' }), RuleSyntaxError, /.+/, (error) => {
      assert.deepEqual([error.context.line, error.context.column], [2, 6])
    })
    throwsWith(() => loads('x\n"1\n', { format: 'csv' }), FormatError, /.+/, (error) => {
      assert.deepEqual([error.context.line, error.context.column], [2, 1])
    })
    throwsWith(() => loads("x,\"THEN y IN ('a')\"\n> 1,'b'\n", { format: 'csv' }), RuleDefinitionError, /.+/, (error) => {
      assert.deepEqual([error.context.line, error.context.column], [2, 5])
      assert.match(error.hint, /values/)
    })
    const document =
      '{"version": 1, "decision_table": {"inputs": ["x"], "outputs": [],' +
      ' "rows": [{"inputs": ["IN ("], "outputs": []}]}}'
    throwsWith(() => loads(document), RuleSyntaxError, /column/, (error) => {
      assert.equal(error.context.pointer, '/decision_table/rows/0/inputs/0')
    })
  })

  test('only tables are CSV, and tables are not text', () => {
    throwsWith(() => dumps(new RuleSet([]), { format: 'csv' }), UnsupportedError, /decision tables/)
    throwsWith(() => dumps(risk(), { format: 'text' }), UnsupportedError, /no text form/)
    throwsWith(() => loads('x\n> 1\n', { format: 'csv', limits: { maxSize: 3 } }), FormatError, /.+/, (error) => {
      assert.equal(error.code, 'limit_exceeded')
    })
  })

  test('tables are immutable and delegate to their rule set', () => {
    const table = risk()
    assert.ok(Object.isFrozen(table) && Object.isFrozen(table.inputs) && Object.isFrozen(table.outputs))
    assert.ok(Object.isFrozen(table.rows) && Object.isFrozen(table.rows[0]) && Object.isFrozen(table.rows[0].inputs))
    assert.throws(() => {
      table.hitPolicy = 'first'
    }, TypeError)
    const decisions = table.evaluateMany([{ customer: { years: 9 } }, { order: { total: 6000 } }])
    assert.deepEqual(
      decisions.map((decision) => decision.fired),
      [
        ['vip', 'default'],
        ['big', 'default'],
      ],
    )
    assert.deepEqual(
      table.summary().rules.map((rule) => rule.name),
      ['new', 'vip', 'big', 'default'],
    )
  })

  test('rows build the table their CSV would', () => {
    const header = [
      'RULE',
      'PRIORITY',
      'customer.years',
      'order.total',
      'THEN discount',
      "THEN risk IN ('high', 'medium', 'low')",
      'DESCRIPTION',
    ]
    const table = new DecisionTable(
      header,
      [
        ['new', null, '< 1', '> 1000', 0, "'high'", 'New customers with large orders are checked first'],
        ['vip', null, '> 5', '-', decimal('0.15'), "'low'", ' Long-standing customers '],
        ['big', '', '-', '> 5000', '0.05', "'medium'"],
        [null, undefined, null, null, '0', "'low'"],
      ],
      { hitPolicy: 'PRIORITY' },
    )
    assert.equal(dumps(table, { format: 'csv' }), EXAMPLE_TEXT.replace('default,', ','))
    assert.equal(table.hitPolicy, 'priority')
    assert.deepEqual(
      table.ruleset.rules.map((rule) => rule.name),
      ['new', 'vip', 'big', 'row_4'],
    )
    // Rows may be any iterable, read one at a time, and so may their cells.
    function* rows() {
      yield new Set(['> 1'])
    }
    assert.equal(new DecisionTable(['x'], rows()).rows.length, 1)
  })

  test('cells may be values and expressions; a header may hold expressions and outputs', () => {
    const table = new DecisionTable(
      [F.age, 'since', new TableOutput('band', ['minor', 'adult']), 'THEN note'],
      [
        [F.age.lt(18), null, "'minor'"],
        [18, date(2026, 1, 1), "'adult'", F.age.mul(2)],
        [true, null, "'adult'", "'x'", null, '  '],
      ],
      { aggregate: null },
    )
    assert.deepEqual(
      table.outputs.map((output) => [output.name, output.values]),
      [
        ['band', ['minor', 'adult']],
        ['note', []],
      ],
    )
    assert.deepEqual(table.inputs.map(String), ['age', 'since'])
    assert.deepEqual(
      table.rows.map((row) => row.inputs.map((cell) => (cell === null ? null : String(cell)))),
      [
        ['age < 18', null],
        ['age = 18', 'since = DATE "2026-01-01"'],
        ['age = TRUE', null],
      ],
    )
    assert.ok(!table.strict && table.rows[1].outputs[1] !== null)
    assert.deepEqual(table.evaluate({ age: 18, since: date(2026, 1, 1) }).output, { band: 'adult', note: 36 })
    // An output's values are constants, decimals included.
    const priced = new DecisionTable(['x', new TableOutput('d', [decimal('0.10'), decimal('0.05')])], [['> 1', decimal('0.05')]])
    assert.equal(String(priced.outputs[0].values[0]), '0.10')
    assert.equal(dumps(priced, { format: 'csv' }), 'HIT POLICY,first\nx,"THEN d IN (0.10, 0.05)"\n> 1,0.05\n')
  })

  test('text cells are the text syntax', () => {
    // "high" is the field high: the string is written between single quotes.
    throwsWith(
      () => new DecisionTable(['x', "THEN risk IN ('high', 'low')"], [['1', "'low'"], ['2', 'high']]),
      RuleDefinitionError,
      /.+/,
      (error) => {
        assert.equal(error.context.pointer, '/rows/1/1')
        assert.equal(error.hint, "high is a field; the string is written between single quotes: 'high'")
      },
    )
    throwsWith(() => new DecisionTable(['x'], [['> )']]), RuleSyntaxError, /\(column 3 of the text\)/, (error) => {
      assert.equal(error.context.pointer, '/rows/0/0')
    })
  })

  test('grid errors point into the grid', () => {
    const at = (call, Class, pointer) =>
      throwsWith(call, Class, /.+/, (error) => assert.equal(error.context.pointer, pointer))
    at(() => new DecisionTable(['RULE', 'x', 'rule'], []), FormatError, '/header/2')
    at(() => new DecisionTable(['x'], [['1', null, '2']]), FormatError, '/rows/0/2')
    at(() => new DecisionTable(['x', 'PRIORITY'], [['1', 1.5]]), FormatError, '/rows/0/1')
    at(() => new DecisionTable(['x'], [[F.y.gt(1)]]), UnsupportedError, '/rows/0/0')
    // Values the binding cannot convert are located too.
    at(() => new DecisionTable(['x'], [[1], [Number.NaN]]), ExpressionError, '/rows/1/0')
    at(() => new DecisionTable(['x'], [[2n ** 64n]]), ExpressionError, '/rows/0/0')
    at(() => new DecisionTable(['x'], [[decimal('1e40')]]), ExpressionError, '/rows/0/0')
    at(() => new DecisionTable(['x', new TableOutput('y', [new Set()])]), ExpressionError, '/header/1')
    at(() => new DecisionTable(['x', new TableOutput('y', [decimal('1e40')])], []), ExpressionError, '/header/1')
    at(() => new DecisionTable([], []), FormatError, '/header')
  })

  test('settings and shapes are checked', () => {
    throwsWith(() => new DecisionTable(['x'], [], { hitPolicy: 'sometimes' }), RuleDefinitionError, /unknown hit policy "sometimes"/, (error) => {
      assert.match(error.hint, /output_order/)
    })
    throwsWith(() => new DecisionTable(['x'], [], { hitPolicy: 'collect', aggregate: 'mean' }), RuleDefinitionError, /aggregate/)
    throwsWith(() => new DecisionTable(['x'], [], { hitPolicy: 'collect', aggregate: '' }), RuleDefinitionError, /unknown aggregate ""/)
    throwsWith(() => new RuleSet([], { hitPolicy: 'collect', aggregate: '' }), RuleDefinitionError, /unknown aggregate ""/)
    assert.equal(new RuleSet([], { hitPolicy: 'collect', aggregate: null }).aggregate, null)
    throwsWith(() => new DecisionTable(['x'], [], { strict: 1 }), RuleDefinitionError, /strict must be a boolean, not a number/)
    throwsWith(() => new DecisionTable(['x'], [], { hitPolicy: 3 }), RuleDefinitionError, /hitPolicy is a string, not a number/)
    throwsWith(() => new DecisionTable(['x'], [], { logic: 'nope' }), RegistryError, /nope/)
    throwsWith(() => new DecisionTable(['x'], [], { limits: { maxTableSize: -1 } }), RangeError, /maxTableSize/)
    throwsWith(() => new DecisionTable(['x'], [], { hitpolicy: 'first' }), TypeError, /unknown option "hitpolicy"/)
    throwsWith(() => new DecisionTable('x', []), TypeError, /not a string/)
    throwsWith(() => new DecisionTable(null, []), TypeError, /the header is an array of columns, not null/)
    throwsWith(() => new DecisionTable(['x', 3], []), TypeError, /a header item is a string, an Expr or a TableOutput, not the number 3/)
    throwsWith(() => new DecisionTable(['x'], ['> 1']), TypeError, /row 0 is a string/)
    throwsWith(() => new DecisionTable(['x'], [1]), TypeError, /row 0 is an array of cells, not the number 1/)
    throwsWith(() => new DecisionTable(['x'], 7), TypeError, /the rows are an array of rows, not the number 7/)
    throwsWith(() => new TableOutput(1), TypeError, /name is a string/)
    throwsWith(() => new TableOutput('y', 'ab'), TypeError, /array of constants/)
    throwsWith(() => new TableRow(), TypeError, /no public constructor/)
    const table = new DecisionTable(['x', 'THEN y'], [['> 1', 1]], { hitPolicy: 'COLLECT', aggregate: 'SUM', strict: true })
    assert.deepEqual([table.hitPolicy, table.aggregate, table.strict], ['collect', 'sum', true])
    assert.equal(new DecisionTable(['x'], [], { logic: 'boolean' }).logic, 'boolean')
  })

  test('rows are read one at a time; the table size limit applies and can be raised', () => {
    function* failing() {
      yield ['> 1']
      throw new Error('the source failed')
    }
    throwsWith(() => new DecisionTable(['x'], failing()), Error, /the source failed/)
    throwsWith(() => new DecisionTable(['x'], Array.from({ length: 10 }, () => ['> 1']), { limits: { maxTableSize: 7 } }), FormatError, /.+/, (error) => {
      assert.deepEqual([error.code, error.context.pointer], ['limit_exceeded', '/rows/1/0'])
    })
    const text = `x\n${'> 1\n'.repeat(3)}`
    throwsWith(() => loads(text, { format: 'csv', limits: { maxTableSize: 7 } }), FormatError, /.+/, (error) => {
      assert.equal(error.code, 'limit_exceeded')
    })
    assert.ok(loads(text, { format: 'csv', limits: { maxTableSize: 12 } }) instanceof DecisionTable)
    throwsWith(() => new DecisionTable(['x'], [['> 1'.padEnd(20)]], { limits: { maxSize: 10 } }), FormatError, /.+/, (error) => {
      assert.equal(error.code, 'limit_exceeded')
    })
  })

  test('a built table reads back as itself', () => {
    const table = new DecisionTable(['x', 'THEN y'], [['> 1', "'a'"]])
    assert.equal(dumps(loads(dumps(table))), dumps(table))
    assert.equal(dumps(loads(dumps(table, { format: 'yaml' }), { format: 'yaml' }), { format: 'csv' }), dumps(table, { format: 'csv' }))
    assert.ok(loads(dumps(table, { format: 'csv' }), { format: 'csv', actions: {} }) instanceof DecisionTable)
    assert.ok(new CleanRulesError('x') instanceof Error)
  })
})
