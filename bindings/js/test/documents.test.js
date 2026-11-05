// Documents (task F.2b): loads, dumps, load, dump and ruleFromText, with the
// behaviour of Python's `cleanrules._documents` and its tests
// (bindings/python/tests/test_documents.py).

import assert from 'node:assert/strict'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { describe, test } from 'node:test'
import { pathToFileURL } from 'node:url'

import { deno, root } from './support/cr.js'

const {
  Const,
  DecisionTable,
  Expr,
  ExpressionError,
  F,
  FormatError,
  RegistryError,
  Rule,
  RuleDefinitionError,
  RuleSet,
  RuleSyntaxError,
  UnsupportedError,
  decimal,
  dump,
  dumps,
  float,
  load,
  loads,
  ruleFromText,
} = root

const EXAMPLES = path.join(import.meta.dirname, '..', '..', '..', 'docs', 'examples')

/** Whether `call` throws `Class` with a message matching `pattern`. */
function throwsWith(call, Class, pattern, check = () => {}) {
  assert.throws(call, (error) => {
    assert.ok(error instanceof Class, `${error?.constructor?.name}: ${error?.message}`)
    assert.match(error.message, pattern)
    check(error)
    return true
  })
}

/** Whether the promise rejects with `Class` and a message matching `pattern`. */
async function rejectsWith(promise, Class, pattern, check = () => {}) {
  await assert.rejects(promise, (error) => {
    assert.ok(error instanceof Class, `${error?.constructor?.name}: ${error?.message}`)
    assert.match(error.message, pattern)
    check(error)
    return true
  })
}

/** A new empty directory, removed after the test. */
function tempDir(t) {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'cleanrules-'))
  t.after(() => fs.rmSync(dir, { recursive: true, force: true }))
  return dir
}

function shop() {
  return new RuleSet(
    [
      new Rule('vip', F.customer.years.gt(5).and(F.since.isNull()), { discount: decimal('0.15'), label: 'VIP' }, {
        priority: 10,
        tags: ['gold'],
        description: 'long-standing customers',
        metadata: { owner: 'risk', reviewed: [2025, 2026], weight: float(1) },
      }),
      new Rule('loyal', F.customer.years.gt(2), { discount: decimal('0.05') }, { priority: 5 }),
    ],
    { hitPolicy: 'priority' },
  )
}

const SHOP_TEXT = `HIT POLICY priority

RULE vip PRIORITY 10 TAGS gold
IF customer.years > 5 AND since IS NULL
THEN discount = 0.15, label = "VIP"

RULE loyal PRIORITY 5
IF customer.years > 2
THEN discount = 0.05`

describe('loads and dumps', () => {
  for (const format of ['json', 'yaml']) {
    test(`rule sets round trip in ${format}`, () => {
      const rules = shop()
      const text = dumps(rules, { format })
      const back = loads(text, { format })
      assert.ok(back instanceof RuleSet)
      assert.equal(dumps(back), dumps(rules))
      const vip = back.rule('vip')
      assert.ok(vip.when.sameAs(rules.rule('vip').when))
      assert.deepEqual([vip.priority, vip.tags, vip.description], [10, ['gold'], 'long-standing customers'])
      // A float that JavaScript would write as 1 stays a float: the core's rule is kept.
      assert.match(dumps(back), /"weight":1\.0\}/)
      assert.deepEqual(Object.keys(vip.then), ['discount', 'label'])
      assert.ok(vip.then.discount.sameAs(rules.rule('vip').then.discount))
      const data = { customer: { years: 9 } }
      assert.deepEqual(back.evaluate(data).output, rules.evaluate(data).output)
      assert.equal(back.hitPolicy, 'priority')
    })
  }

  test('documents are the canonical JSON; conditions and rules read back as themselves', () => {
    assert.deepEqual(JSON.parse(dumps(F.x.gt(1))), {
      version: 1,
      condition: { op: 'gt', left: { op: 'field', path: ['x'] }, right: { op: 'const', value: 1 } },
    })
    const condition = loads(dumps(F.x.gt(1).and(F.y.isIn(['a', 'b']))))
    assert.ok(condition instanceof Expr)
    assert.ok(condition.sameAs(F.x.gt(1).and(F.y.isIn(['a', 'b']))))
    const rule = loads(dumps(shop().rule('vip')))
    assert.ok(rule instanceof Rule)
    assert.equal(rule.name, 'vip')
    assert.deepEqual(rule.toJSON(), shop().rule('vip').toJSON())
    assert.equal(dumps(F.x.gt(1), { format: 'text' }), 'x > 1')
    // A constant used as a condition is a document too.
    assert.ok(loads(dumps(root.constant(true))) instanceof Const)
  })

  test('YAML follows the JSON subset of the core schema', () => {
    const text = 'version: 1\ncondition:\n  op: eq\n  left: {op: field, path: [d]}\n  right: {op: const, value: 2026-10-31}\n'
    assert.equal(String(loads(text, { format: 'yaml' })), 'd = "2026-10-31"')
  })

  test('errors have pointers, lines and columns, as the core reports them', () => {
    throwsWith(() => loads('{"version": 1, "condition": {"op": "const", "value": true, "note": 1}}'), FormatError, /.+/, (error) => {
      assert.equal(error.context.pointer, '/condition/note')
    })
    throwsWith(() => loads('version: 1\ncondition: {op: const, value: true}\nversion: 1\n', { format: 'yaml' }), FormatError, /.+/, (error) => {
      assert.deepEqual([error.context.line, error.context.column], [3, 1])
    })
    throwsWith(() => loads(new Uint8Array([0xff, 0xfe])), FormatError, /UTF-8/)
    throwsWith(() => loads('RULE r\nIF x >\nTHEN y = 1', { format: 'text' }), RuleSyntaxError, /expected a field/, (error) => {
      assert.deepEqual([error.context.line, error.context.column], [3, 1])
    })
    throwsWith(() => loads('a AND\n  x MATCHES "("', { format: 'text' }), ExpressionError, /.+/, (error) => {
      assert.deepEqual([error.context.line, error.context.column], [2, 3])
    })
    throwsWith(() => loads('{}', { format: 'xml' }), RangeError, /unknown format "xml"/)
    throwsWith(() => loads(42), TypeError, /the text must be a string or bytes/)
  })

  test('limits: each one is applied, and YAML has its own size limit', () => {
    const text = dumps(shop())
    throwsWith(() => loads(text, { limits: { maxSize: 10 } }), FormatError, /.+/, (error) => {
      assert.equal(error.code, 'limit_exceeded')
    })
    assert.ok(loads(text, { limits: { maxSize: text.length } }) instanceof RuleSet)
    throwsWith(() => loads('NOT NOT NOT x', { format: 'text', limits: { maxDepth: 2 } }), FormatError, /.+/, (error) => {
      assert.equal(error.code, 'limit_exceeded')
    })
    const yaml = `version: 1\ncondition: {op: const, value: true}\n# ${'x'.repeat(1024 * 1024)}\n`
    throwsWith(() => loads(yaml, { format: 'yaml' }), FormatError, /.+/, (error) => assert.equal(error.code, 'limit_exceeded'))
    assert.ok(loads(yaml, { format: 'yaml', limits: { maxSize: 2 * 1024 * 1024 } }) instanceof Expr)
  })

  test('value nodes are not documents', () => {
    throwsWith(() => dumps(F.x.add(1)), ExpressionError, /only a condition can be a document, not the value x \+ 1/, (error) => {
      assert.match(error.hint, /F\.x\.gt\(0\)/)
    })
  })

  test('actions are never written as code: their names are, or omitActions leaves them out', () => {
    const record = () => {}
    const rules = new RuleSet([new Rule('a', F.x.gt(1)), new Rule('b', F.x.gt(2), null, { do: record })])
    throwsWith(() => dumps(rules), UnsupportedError, /action of rule "b"/, (error) => {
      assert.equal(error.context.pointer, '/ruleset/rules/1/do')
      assert.equal(error.context.rule, 'b')
    })
    const back = loads(dumps(rules, { omitActions: true }))
    assert.equal(back.rule('b').do, null)
    assert.equal(back.rule('b').actionName, null)
    throwsWith(() => dumps(rules.rule('b')), UnsupportedError, /.+/, (error) => assert.equal(error.context.pointer, '/rule/do'))
    assert.equal(dumps(rules, { format: 'text', omitActions: true }), 'HIT POLICY first\n\nRULE a\nIF x > 1\n\nRULE b\nIF x > 2')
    throwsWith(() => dumps(rules, { omitActions: 'yes' }), TypeError, /omitActions is a boolean/)
    // null is false, as left out.
    throwsWith(() => dumps(rules, { omitActions: null }), UnsupportedError, /action of rule "b"/)
    // Named actions are written, read back unbound, and bound by `actions`.
    const named = new RuleSet([new Rule('a', F.x.gt(1), null, { do: 'notify' })])
    const unbound = loads(dumps(named))
    assert.deepEqual([unbound.rule('a').actionName, unbound.rule('a').do], ['notify', null])
    const calls = []
    const bound = loads(dumps(named), { actions: { notify: ({ rule }) => calls.push(rule.name) } })
    bound.run({ x: 2 })
    assert.deepEqual(calls, ['a'])
    const rule = loads(dumps(named.rule('a')), { actions: new Map([['notify', () => calls.push('rule')]]) })
    assert.equal(typeof rule.do, 'function')
    throwsWith(() => loads(dumps(named), { actions: {} }), RegistryError, /notify/)
  })

  test('the text syntax round trips, without metadata or code', () => {
    const rules = loads(SHOP_TEXT, { format: 'text' })
    assert.ok(rules instanceof RuleSet)
    assert.equal(dumps(rules, { format: 'text' }), SHOP_TEXT)
    assert.ok(rules.rule('vip').then.discount.sameAs(root.constant(decimal('0.15'))))
    assert.equal(String(rules.evaluate({ customer: { years: 9 }, since: null }).output.discount), '0.15')
    throwsWith(() => dumps(shop().rule('vip'), { format: 'text' }), UnsupportedError, /metadata/)
    throwsWith(() => dumps(shop(), { format: 'csv' }), UnsupportedError, /decision tables/)
  })

  test('what dumps and loads refuse', () => {
    throwsWith(() => dumps({}), TypeError, /dumps takes a condition, a rule, a rule set or a decision table, not a Object/)
    throwsWith(() => dumps(F.x.gt(1), { formt: 'json' }), TypeError, /unknown option "formt"/)
    throwsWith(() => loads('{}', { action: {} }), TypeError, /unknown option "action"/)
    throwsWith(() => loads('{}'), FormatError, /.+/)
  })
})

describe('ruleFromText', () => {
  test('reads one rule; rule_1 without a RULE line', () => {
    const rule = ruleFromText('if total > 100 then discount = 0.1')
    assert.deepEqual([rule.name, rule.priority, rule.do], ['rule_1', 0, null])
    assert.ok(rule.when.sameAs(F.total.gt(100)))
    assert.ok(rule instanceof Rule)
    const record = () => {}
    const acting = ruleFromText('RULE big TAGS a IF total > 100', { do: record })
    assert.deepEqual([acting.name, acting.tags, acting.do, acting.actionName], ['big', ['a'], record, null])
    // A name the text gives is bound to the function.
    const named = ruleFromText(new TextEncoder().encode('RULE big IF total > 100 DO notify'), { do: record })
    assert.deepEqual([named.actionName, named.do], ['notify', record])
    const fromRegistry = ruleFromText('RULE big IF total > 100 DO notify', { actions: { notify: record } })
    assert.equal(fromRegistry.do, record)
    const result = new RuleSet([acting]).run({ total: 200 })
    assert.deepEqual(result.fired, ['big'])
  })

  test('what it refuses', () => {
    throwsWith(() => ruleFromText('total > 100'), RuleSyntaxError, /expected RULE or IF/, (error) => {
      assert.deepEqual([error.context.line, error.context.column], [1, 1])
    })
    throwsWith(() => ruleFromText('IF a\nIF b'), RuleSyntaxError, /end of the text/, (error) => {
      assert.deepEqual([error.context.line, error.context.column], [2, 1])
      assert.match(error.hint, /rule set/)
    })
    throwsWith(() => ruleFromText('IF a', { do: () => {}, actions: {} }), TypeError, /do or actions, not both/)
    throwsWith(() => ruleFromText('IF a', { do: 'notify' }), RuleDefinitionError, /do must be a function, not the string "notify"/)
    throwsWith(() => ruleFromText('IF NOT NOT NOT a', { limits: { maxDepth: 2 } }), FormatError, /.+/)
    throwsWith(() => ruleFromText('IF a DO notify', { actions: {} }), RegistryError, /notify/)
  })
})

describe('load and dump', () => {
  test('files in every format, by extension', async (t) => {
    const dir = tempDir(t)
    const rules = shop()
    for (const name of ['rules.json', 'rules.yaml', 'rules.yml']) {
      await dump(rules, path.join(dir, name))
      assert.equal(dumps(await load(path.join(dir, name))), dumps(rules))
    }
    assert.ok(fs.readFileSync(path.join(dir, 'rules.yaml'), 'utf8').startsWith('version: 1\nruleset:\n'))
    // A file: URL works too.
    assert.ok((await load(pathToFileURL(path.join(dir, 'rules.json')))) instanceof RuleSet)
    await rejectsWith(dump(rules, path.join(dir, 'rules.txt')), RangeError, /cannot tell the format of "rules\.txt" from its extension/)
    await dump(rules, path.join(dir, 'rules.txt'), { format: 'yaml' })
    assert.ok((await load(path.join(dir, 'rules.txt'), { format: 'yaml' })) instanceof RuleSet)
    // A malformed escape in a file: URL is named as written (no URIError).
    await rejectsWith(load(new URL('file:///nowhere/%E0%A4%A')), RangeError, /cannot tell the format of "%E0%A4%A"/)
    await rejectsWith(load(42), TypeError, /load takes a path \(a string or a file: URL\), not the number 42/)
    await rejectsWith(load(path.join(dir, 'missing.json')), Error, /missing\.json|No such file|not found/i)
  })

  test('a text file ends with a line break; CSV keeps its line breaks', async (t) => {
    const dir = tempDir(t)
    const file = path.join(dir, 'pricing.rules')
    fs.writeFileSync(file, SHOP_TEXT)
    assert.equal(dumps(await load(file), { format: 'text' }), SHOP_TEXT)
    await dump(await load(file), path.join(dir, 'copy.rules'))
    assert.equal(fs.readFileSync(path.join(dir, 'copy.rules'), 'utf8'), `${SHOP_TEXT}\n`)
    await rejectsWith(load(path.join(dir, 'pricing.txt')), RangeError, /format: 'json'/)
    const csv = path.join(dir, 't.csv')
    fs.writeFileSync(csv, 'RULE,x,DESCRIPTION\na,> 1,"two\r\nlines"\n')
    const table = await load(csv)
    assert.ok(table instanceof DecisionTable)
    assert.equal(table.rows[0].description, 'two\r\nlines')
    await dump(table, path.join(dir, 'copy.csv'))
    assert.equal(fs.readFileSync(path.join(dir, 'copy.csv'), 'utf8'), dumps(table, { format: 'csv' }))
  })

  test('a file larger than the limit is not read', async (t) => {
    const dir = tempDir(t)
    const big = path.join(dir, 'big.yaml')
    fs.writeFileSync(big, `# ${'x'.repeat(1024 * 1024)}\n`)
    await rejectsWith(load(big), FormatError, /big\.yaml has 1048579 bytes, more than the limit of 1048576/, (error) => {
      assert.equal(error.code, 'limit_exceeded')
      assert.match(error.hint, /limits\.maxSize/)
    })
    // Within a raised limit, it is read (and holds no document).
    await rejectsWith(load(big, { limits: { maxSize: 2 * 1024 * 1024 } }), FormatError, /.+/, (error) => {
      assert.notEqual(error.code, 'limit_exceeded')
    })
    const json = path.join(dir, 'small.json')
    fs.writeFileSync(json, dumps(shop()))
    await rejectsWith(load(json, { limits: { maxSize: 10 } }), FormatError, /small\.json has \d+ bytes/)
    const rules = path.join(dir, 'r.rules')
    fs.writeFileSync(rules, 'IF x')
    await rejectsWith(load(rules, { limits: { maxTextSize: 2 } }), FormatError, /r\.rules has 4 bytes, more than the limit of 2/)
    // A text of one rule is a rule document.
    assert.ok((await load(rules, { actions: {} })) instanceof Rule)
  })

  test('the documented examples load', async () => {
    // Git may check the examples out with CRLF (Windows); dumps writes LF.
    const example = (name) => fs.readFileSync(path.join(EXAMPLES, name), 'utf8').replaceAll('\r\n', '\n')
    const fromYaml = await load(path.join(EXAMPLES, 'discounts.yaml'))
    const fromJson = await load(path.join(EXAMPLES, 'discounts.json'))
    assert.ok(fromYaml instanceof RuleSet)
    assert.equal(dumps(fromYaml), dumps(fromJson))
    assert.equal(example('discounts.yaml'), dumps(fromYaml, { format: 'yaml' }))
    const fromText = await load(path.join(EXAMPLES, 'discounts.rules'))
    assert.equal(dumps(fromText), dumps(fromJson))
    assert.equal(example('discounts.rules'), `${dumps(fromText, { format: 'text' })}\n`)
  })

  // Browsers have no file system: there load and dump reject. Node stands in
  // for one without `process.getBuiltinModule` (Deno's is always there).
  test('without a file system, load and dump reject, naming loads and dumps', { skip: deno }, async () => {
    const saved = process.getBuiltinModule
    process.getBuiltinModule = undefined
    try {
      await rejectsWith(load('rules.json'), UnsupportedError, /load needs a file system/, (error) => {
        assert.match(error.hint, /loads\(text, \{ format \}\)/)
        assert.equal(error.context.operation, 'load')
      })
      await rejectsWith(dump(F.x.gt(1), 'x.json'), UnsupportedError, /dump needs a file system/, (error) => {
        assert.match(error.hint, /dumps\(value, \{ format \}\)/)
      })
    } finally {
      process.getBuiltinModule = saved
    }
  })
})

// `toJSON()` gives objects for `JSON.stringify`, which writes the number 1.0
// as `1`, an int: floats that hold an integer come as `{"$float": n}`
// (`spec/json-format.md`, *Values*), so the text means the same document.
describe('toJSON and JSON.stringify', () => {
  const big = 2n ** 60n
  const payload = { n: float(1), i: float(Infinity), b: big, h: 0.5, e: float(1e300), d: decimal('2.50') }
  const rules = new RuleSet(
    [
      new Rule('a', F.x.ge(float(2)).and(F.y.isIn([float(3), 4])), payload, { priority: big, metadata: { $k: float(7), m: big } }),
      new Rule('b', F.x.lt(10), { n: 1 }),
    ],
    { hitPolicy: 'priority' },
  )

  test('a decision written with JSON.stringify reads back with its rule set', () => {
    // The reproduction: JSON.stringify used to write 1.0 as 1, and readDecision
    // refused it ("not the rule's constant").
    const decision = rules.evaluate({ x: float(3), y: 4 })
    const text = JSON.stringify(decision)
    assert.match(text, /"n":\{"op":"const","value":\{"\$float":1\}\}/)
    assert.match(text, /"value":\{"\$int":"1152921504606846976"\}/)
    assert.match(text, /"value":\{"\$float":"inf"\}/)
    const read = rules.readDecision(text)
    assert.deepEqual(read.fired, decision.fired)
    assert.deepEqual(read.output, decision.output)
    assert.deepEqual(read.results.map((r) => r.output), decision.results.map((r) => r.output))
  })

  test('loads(JSON.stringify(x)) is x for conditions, rules, rule sets and tables', () => {
    const table = new DecisionTable(['RULE', 'x', 'THEN rate'], [['a', '> 1', float(2)], ['b', float(3), 0.5]])
    for (const value of [rules, rules.rules[0], table]) {
      assert.equal(dumps(loads(JSON.stringify(value))), dumps(value), String(value))
    }
    // A node's toJSON() is the node, without the document around it.
    for (const node of [rules.rules[0].when, F.z.between(float(1), 2.5)]) {
      assert.equal(dumps(loads(`{"version":1,"condition":${JSON.stringify(node)}}`)), dumps(node), String(node))
    }
    assert.match(JSON.stringify(rules), /"metadata":\{"\$k":\{"\$float":7\},"m":\{"\$int":"1152921504606846976"\}\}/)
  })

  test('explanations, result trees, summaries and errors type their floats too', () => {
    const decision = rules.evaluate({ x: float(1), y: float(3) })
    assert.match(JSON.stringify(decision.explain()), /"value":\{"\$float":1\}/)
    assert.match(JSON.stringify(rules.summary()), /"priority":\{"\$int":"1152921504606846976"\}/)
    const { evaluateTree } = root
    assert.equal(
      JSON.stringify(evaluateTree(F.x.gt(float(1)), { x: float(2) })),
      '{"op":"gt","truth":true,"children":[{"op":"field","path":["x"],"value":{"$float":2}},{"op":"const","value":{"$float":1}}]}',
    )
    const strict = new RuleSet([new Rule('s', F.x.gt(float(1)))], { strict: true })
    assert.throws(
      () => strict.evaluate({ x: 's' }),
      (error) => JSON.stringify(error).includes('"right":{"op":"const","value":{"$float":1}}'),
    )
  })

  test('the limit: JSON.stringify loses the sign of a float zero; dumps keeps it', () => {
    const negative = F.x.eq(-0)
    assert.equal(dumps(negative), '{"version":1,"condition":{"op":"eq","left":{"op":"field","path":["x"]},"right":{"op":"const","value":-0.0}}}')
    // JSON.stringify(-0) is "0": the float stays a float, without its sign.
    const text = JSON.stringify(negative)
    assert.equal(text, '{"op":"eq","left":{"op":"field","path":["x"]},"right":{"op":"const","value":{"$float":0}}}')
    const again = loads(`{"version":1,"condition":${text}}`)
    assert.equal(dumps(again), dumps(F.x.eq(float(0))))
    assert.notEqual(dumps(again), dumps(negative))
    assert.equal(dumps(loads(dumps(negative))), dumps(negative))
  })

  test('the limit: plain objects list integer-like keys first; dumps keeps the order', () => {
    const ordered = new RuleSet([new Rule('a', F.go, new Map([['b', 1], ['10', 2], ['2', 3]]))])
    assert.match(dumps(ordered), /"then":\{"b":.*,"10":.*,"2":/)
    assert.match(JSON.stringify(ordered), /"then":\{"2":.*,"10":.*,"b":/)
    // A payload's order is part of the rule, so the decision no longer matches it.
    throwsWith(() => ordered.readDecision(JSON.stringify(ordered.evaluate({ go: true }))), FormatError, /./, (error) => {
      assert.equal(error.code, 'decision_mismatch')
    })
    // Without a payload evaluated, it reads back.
    assert.deepEqual(ordered.readDecision(JSON.stringify(ordered.evaluate({ go: false }))).fired, [])
  })
})
