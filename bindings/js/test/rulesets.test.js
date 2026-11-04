'use strict'

// Documents, rule sets, decisions and nodes: every native entry point.

const assert = require('node:assert/strict')
const fs = require('node:fs')
const path = require('node:path')
const { describe, test } = require('node:test')

const cr = require('./support/cr.js')
const { rules } = require('./support/probe.js')

const EXAMPLES = path.join(__dirname, '..', '..', '..', 'docs', 'examples')
const example = (name) => fs.readFileSync(path.join(EXAMPLES, name))
const DISCOUNTS_DATA = { customer: { years: 6, blocked: false }, order: { total: 2000 } }

describe('the package', () => {
  test('versions', () => {
    assert.equal(cr.FORMAT_VERSION, 1)
    assert.equal(cr.version, require('../package.json').version)
  })

  test('the root exports what is final; the native classes are in cleanrules/native', () => {
    const root = require('..')
    const native = require('../native')
    assert.deepEqual(Object.keys(native).sort(), ['Decision', 'Document', 'FORMAT_VERSION', 'Node', 'RuleSet'])
    for (const name of ['RuleSet', 'Document', 'Decision', 'Node']) {
      assert.equal(root[name], undefined, name)
    }
    assert.equal(typeof root.MissingFieldError, 'function')
    assert.equal(typeof root.decimal, 'function')
    // The package's own `exports` resolve both entry points.
    assert.equal(require.resolve('cleanrules'), path.join(__dirname, '..', 'index.js'))
    assert.equal(require.resolve('cleanrules/native'), path.join(__dirname, '..', 'native.js'))
  })

  test('ES modules import every name of both entry points', async () => {
    const { pathToFileURL } = require('node:url')
    for (const file of ['index.js', 'native.js']) {
      const module = await import(pathToFileURL(path.join(__dirname, '..', file)).href)
      const common = require(path.join('..', file))
      for (const name of Object.keys(common)) {
        assert.equal(module[name], common[name], `${file}: ${name}`)
      }
    }
  })

  test('an addon of another version is refused, naming the fix', () => {
    const { execFileSync } = require('node:child_process')
    const root = path.join(__dirname, '..')
    const code = `
      const binding = require(${JSON.stringify(path.join(root, 'binding.js'))})
      binding.version = () => '9.9.9'
      try { require(${JSON.stringify(root)}) } catch (error) { console.log(error.message) }
    `
    const output = execFileSync(process.execPath, ['-e', code], { encoding: 'utf8' })
    assert.match(output, /native addon is version 9\.9\.9, but the package is .*; reinstall cleanrules/)
  })

  test('works in a worker thread, with its own hooks', async () => {
    const { Worker } = require('node:worker_threads')
    const code = `
      const { parentPort } = require('node:worker_threads')
      const cr = { ...require(${JSON.stringify(path.join(__dirname, '..'))}), ...require(${JSON.stringify(path.join(__dirname, '..', 'native'))}) }
      const ruleset = cr.RuleSet.read('STRICT\\n\\nRULE a\\nIF x > 1\\nTHEN v = x', { format: 'text' })
      let missing = false
      try { ruleset.evaluate({}) } catch (error) { missing = error instanceof cr.MissingFieldError }
      parentPort.postMessage([ruleset.evaluate({ x: 2 }).outputJson(), missing])
    `
    const worker = new Worker(code, { eval: true })
    const [output, missing] = await new Promise((resolve, reject) => {
      worker.once('message', resolve)
      worker.once('error', reject)
    })
    await worker.terminate()
    assert.equal(output, '{"v":2}')
    assert.equal(missing, true)
  })
})

describe('Document', () => {
  test('reads every kind of document in every format', () => {
    assert.equal(cr.Document.read(example('discounts.json')).kind, 'ruleset')
    assert.equal(cr.Document.read(example('discounts.yaml'), { format: 'yaml' }).kind, 'ruleset')
    assert.equal(cr.Document.read(example('discounts.rules'), { format: 'text' }).kind, 'ruleset')
    assert.equal(cr.Document.read(example('risk.csv'), { format: 'csv' }).kind, 'decision_table')
    assert.equal(cr.Document.read('x > 1', { format: 'text' }).kind, 'condition')
    assert.equal(cr.Document.read('RULE a\nIF x > 1', { format: 'text' }).kind, 'rule')
  })

  test('writes back what it read', () => {
    const json = cr.Document.read(example('discounts.json'))
    assert.equal(cr.Document.read(json.write('yaml'), { format: 'yaml' }).write(), json.write())
    assert.equal(cr.Document.read(json.write('text'), { format: 'text' }).write(), json.write())
    const table = cr.Document.read(example('risk.csv'), { format: 'csv' })
    // On Windows, git may check the example out with CRLF line ends.
    assert.equal(table.write('csv'), example('risk.csv').toString().replace(/\r\n/g, '\n'))
    assert.equal(cr.Document.read(table.write(), { format: 'json' }).write('csv'), table.write('csv'))
    assert.throws(() => table.write('text'), cr.UnsupportedError)
  })

  test('strings and bytes read the same', () => {
    const bytes = example('discounts.rules')
    assert.equal(cr.Document.read(bytes, { format: 'text' }).write(), cr.Document.read(bytes.toString(), { format: 'text' }).write())
    assert.equal(cr.Document.read(new Uint8Array(bytes), { format: 'text' }).write(), cr.Document.read(bytes, { format: 'text' }).write())
  })

  test('limits: each one is applied', () => {
    const json = example('discounts.json').toString()
    const exceeded = (run) => assert.throws(run, (e) => e instanceof cr.FormatError && e.code === 'limit_exceeded')
    exceeded(() => cr.Document.read(json, { format: 'json', limits: { maxSize: 100 } }))
    exceeded(() => cr.Document.read(json, { format: 'json', limits: { maxNesting: 3 } }))
    exceeded(() => cr.Document.read(json, { format: 'json', limits: { maxDepth: 1 } }))
    exceeded(() => cr.Document.read(example('discounts.yaml'), { format: 'yaml', limits: { maxYamlSize: 100 } }))
    exceeded(() => cr.Document.read(example('discounts.rules'), { format: 'text', limits: { maxTextSize: 50 } }))
    exceeded(() => cr.Document.read(example('risk.csv'), { format: 'csv', limits: { maxTableSize: 5 } }))
    // Within the limits, and raising them, reads.
    cr.Document.read(json, { format: 'json', limits: { maxSize: 1e7, maxNesting: 64, maxDepth: 64 } })
    cr.Document.read(example('discounts.yaml'), { format: 'yaml', limits: { maxSize: 1e6 } })
  })
})

describe('RuleSet', () => {
  test('reads from every format, and from a document', () => {
    const fromJson = cr.RuleSet.read(example('discounts.json'))
    assert.equal(cr.RuleSet.read(example('discounts.yaml'), { format: 'yaml' }).write(), fromJson.write())
    assert.equal(cr.RuleSet.read(example('discounts.rules'), { format: 'text' }).write(), fromJson.write())
    assert.equal(cr.RuleSet.fromDocument(cr.Document.read(example('discounts.json'))).write(), fromJson.write())
    const table = cr.RuleSet.read(example('risk.csv'), { format: 'csv' })
    assert.equal(table.length, 4)
    // A rule document is a rule set of that rule; a condition is not one.
    assert.equal(rules('RULE a\nIF x > 1').length, 1)
    assert.throws(() => cr.RuleSet.read('x > 1', { format: 'text' }), cr.FormatError)
    assert.throws(() => cr.RuleSet.fromDocument(cr.Document.read('x > 1', { format: 'text' })), cr.FormatError)
  })

  test('tells its configuration', () => {
    const ruleset = cr.RuleSet.read(example('discounts.rules'), { format: 'text' })
    assert.equal(ruleset.length, 3)
    assert.deepEqual(ruleset.ruleNames(), ['blocked', 'vip', 'loyal'])
    assert.equal(ruleset.hitPolicy, 'priority')
    assert.equal(ruleset.aggregate, null)
    assert.equal(ruleset.logic, 'boolean')
    assert.equal(ruleset.strict, false)
    assert.deepEqual(ruleset.fieldPaths(), [
      ['customer', 'blocked'],
      ['customer', 'years'],
      ['order', 'total'],
      ['customer', 'newsletter'],
    ])
    const collect = rules('HIT POLICY collect\nAGGREGATE sum\nSTRICT\n\nRULE a\nIF items[0].n > 1\nTHEN v = 1')
    assert.equal(collect.aggregate, 'sum')
    assert.equal(collect.strict, true)
    assert.deepEqual(collect.fieldPaths(), [['items', 0, 'n']])
  })

  test('writes and summarises in every format', () => {
    const ruleset = cr.RuleSet.read(example('discounts.rules'), { format: 'text' })
    assert.equal(JSON.parse(ruleset.write()).version, 1)
    assert.match(ruleset.write('yaml'), /^version: 1/)
    assert.match(ruleset.write('text'), /RULE vip/)
    assert.throws(() => ruleset.write('csv'), cr.UnsupportedError)
    const summary = JSON.parse(ruleset.summary('json'))
    assert.equal(summary.summary.rules.length, 3)
    assert.match(ruleset.summary('yaml'), /^version: 1/)
    assert.match(ruleset.summary(), /^Rule set: 3 rules, hit policy priority/)
    assert.equal(ruleset.summary('text'), ruleset.summary())
    assert.throws(() => ruleset.summary('csv'), cr.UnsupportedError)
  })

  test('evaluates native data and JSON alike', () => {
    const ruleset = cr.RuleSet.read(example('discounts.rules'), { format: 'text' })
    const native = ruleset.evaluate(DISCOUNTS_DATA)
    const json = ruleset.evaluateJson(JSON.stringify(DISCOUNTS_DATA))
    assert.equal(native.write(), json.write())
    assert.equal(native.explain(), json.explain())
    assert.equal(ruleset.evaluateJson(Buffer.from(JSON.stringify(DISCOUNTS_DATA))).write(), json.write())
    assert.throws(() => ruleset.evaluateJson('{'), cr.FormatError)
    assert.throws(() => ruleset.evaluateJson('[[[1]]]', { maxNesting: 2 }), cr.FormatError)
  })

  test('evaluates batches, in order', () => {
    const ruleset = rules('RULE big\nIF x > 1\nTHEN v = x')
    const decisions = ruleset.evaluateMany([{ x: 2 }, { x: 0 }, { x: 3 }])
    assert.deepEqual(
      decisions.map((d) => d.outputJson()),
      ['{"v":2}', null, '{"v":3}'],
    )
    const fromJson = ruleset.evaluateManyJson('[{"x": 2}, {"x": 0}, {"x": 3}]')
    assert.deepEqual(
      fromJson.map((d) => d.write()),
      decisions.map((d) => d.write()),
    )
    assert.deepEqual(ruleset.evaluateMany([]), [])
    assert.equal(ruleset.evaluateMany([null])[0].outputJson(), null)
    assert.throws(
      () => ruleset.evaluateMany([{ x: 2 }, undefined]),
      (e) => e instanceof cr.EvaluationError && e.code === 'evaluation_error' && e.context.row === 1,
    )
    assert.throws(() => ruleset.evaluateManyJson('{"x": 1}'), (e) => e.code === 'evaluation_error')
  })

  test('a batch error names the row, and chains what a getter threw', () => {
    const ruleset = rules('STRICT\n\nRULE a\nIF x > 1')
    assert.throws(
      () => ruleset.evaluateMany([{ x: 2 }, { x: 0 }, {}]),
      (e) => e instanceof cr.MissingFieldError && e.context.row === 2,
    )
    assert.throws(
      () => ruleset.evaluateManyJson('[{"x": 2}, {}]'),
      (e) => e instanceof cr.MissingFieldError && e.context.row === 1,
    )
    const thrown = new Error('row two')
    const bad = {
      get x() {
        throw thrown
      },
    }
    assert.throws(
      () => ruleset.evaluateMany([{ x: 2 }, bad]),
      (e) => e.code === 'evaluation_error' && e.context.row === 1 && e.cause === thrown,
    )
  })

  test('reads a recorded decision with its rule set, in JSON and YAML', () => {
    const ruleset = cr.RuleSet.read(example('discounts.rules'), { format: 'text' })
    const decision = ruleset.evaluate(DISCOUNTS_DATA)
    const fromJson = ruleset.readDecision(decision.write())
    assert.equal(fromJson.write(), decision.write())
    assert.equal(fromJson.explain(), decision.explain())
    assert.deepEqual(fromJson.selected(), decision.selected())
    const fromYaml = ruleset.readDecision(decision.write('yaml'), { format: 'yaml' })
    assert.equal(fromYaml.write(), decision.write())
    assert.throws(() => ruleset.readDecision(decision.write(), { format: 'text' }), cr.UnsupportedError)
    assert.throws(() => ruleset.readDecision(decision.write(), { format: 'json', limits: { maxSize: 10 } }), cr.FormatError)
  })
})

describe('Decision', () => {
  const ruleset = cr.RuleSet.read(example('discounts.rules'), { format: 'text' })
  const decision = ruleset.evaluate(DISCOUNTS_DATA)

  test('tells each rule`s result', () => {
    assert.equal(decision.length, 3)
    assert.deepEqual([0, 1, 2].map((i) => decision.ruleName(i)), ['blocked', 'vip', 'loyal'])
    assert.deepEqual([0, 1, 2].map((i) => decision.firedAt(i)), [false, true, true])
    assert.deepEqual([0, 1, 2].map((i) => decision.truth(i)), ['false', 'true', 'true'])
    assert.equal(ruleset.evaluate({ order: { total: 2000 } }).truth(1), 'unknown')
    assert.deepEqual(decision.selected(), [1])
  })

  test('gives outputs in the JSON format', () => {
    assert.equal(decision.outputJson(), '{"discount":{"$decimal":"0.15"}}')
    assert.equal(decision.outputsJson(), '[{"discount":{"$decimal":"0.15"}}]')
    assert.equal(decision.ruleOutputJson(1), '{"discount":{"$decimal":"0.15"}}')
    assert.equal(decision.ruleOutputJson(0), null)
    assert.equal(ruleset.evaluate({}).outputJson(), null)
    const collect = rules('HIT POLICY collect\nAGGREGATE sum\n\nRULE a\nIF TRUE\nTHEN v = 1\n\nRULE b\nIF TRUE\nTHEN v = 2')
    assert.equal(collect.evaluate({}).outputJson(), '{"v":3}')
    assert.equal(collect.evaluate({}).outputsJson(), '[{"v":1},{"v":2}]')
  })

  test('writes itself and its explanation', () => {
    const written = JSON.parse(decision.write())
    assert.equal(written.version, 1)
    assert.ok(Array.isArray(written.decision.results ?? written.decision.rules ?? []))
    assert.match(decision.write('yaml'), /^version: 1/)
    assert.equal(JSON.parse(decision.explain('json')).explanation.selected[0], 'vip')
    assert.match(decision.explain('yaml'), /^version: 1/)
    assert.match(decision.explain(), /^HIT POLICY priority: selected vip/)
    assert.equal(decision.explain('text'), decision.explain())
    assert.throws(() => decision.explain('csv'), cr.UnsupportedError)
  })
})

describe('Node', () => {
  test('reads and writes in JSON, YAML and text', () => {
    const node = cr.Node.read('customer.years > 5 AND NOT blocked', { format: 'text' })
    assert.equal(node.write('text'), 'customer.years > 5 AND NOT blocked')
    assert.equal(cr.Node.read(node.write()).write('text'), node.write('text'))
    assert.equal(cr.Node.read(node.write('yaml'), { format: 'yaml' }).write(), node.write())
    // A value node.
    assert.equal(cr.Node.read('price * 1.21', { format: 'text' }).write('text'), 'price * 1.21')
    assert.throws(() => node.write('csv'), cr.UnsupportedError)
    assert.deepEqual(node.fieldPaths(), [['customer', 'years'], ['blocked']])
  })

  test('evaluates a condition on native data and on JSON', () => {
    const node = cr.Node.read('x > 1', { format: 'text' })
    const tree = JSON.parse(node.evaluateTree({ x: 2 }))
    assert.equal(tree.truth, true)
    assert.equal(node.evaluateTreeJson('{"x": 2}'), node.evaluateTree({ x: 2 }))
    assert.equal(node.evaluateTree({ x: 2 }, { logic: 'boolean', strict: false }), node.evaluateTree({ x: 2 }))
    assert.equal(JSON.parse(node.evaluateTree({})).truth, null)
    assert.throws(() => node.evaluateTree({}, { strict: true }), cr.MissingFieldError)
    assert.throws(() => node.evaluateTreeJson('{}', { logic: 'boolean', strict: true }), cr.MissingFieldError)
    assert.throws(() => node.evaluateTree({ x: 2 }, { logic: 'nope' }), cr.RegistryError)
    assert.throws(() => node.evaluateTreeJson('{'), cr.FormatError)
    assert.throws(() => node.evaluateTreeJson('[[1]]', { limits: { maxNesting: 1 } }), cr.FormatError)
  })

  test('evaluates a value on native data and on JSON', () => {
    const node = cr.Node.read('price * 2', { format: 'text' })
    assert.equal(JSON.parse(node.evaluateValueTree({ price: 3 })).value, 6)
    assert.equal(node.evaluateValueTreeJson('{"price": 3}'), node.evaluateValueTree({ price: 3 }))
    assert.throws(() => node.evaluateValueTree({}, { strict: true }), cr.MissingFieldError)
    assert.throws(() => node.evaluateValueTreeJson('[[1]]', { limits: { maxNesting: 1 } }), cr.FormatError)
  })

  test('strict mode chains what a getter threw', () => {
    const thrown = new Error('getter')
    const node = cr.Node.read('a.b', { format: 'text' })
    const data = {
      get a() {
        throw thrown
      },
    }
    assert.throws(
      () => node.evaluateValueTree(data, { strict: true }),
      (e) => e.code === 'evaluation_error' && e.cause === thrown && e.context.rule === undefined,
    )
  })
})
