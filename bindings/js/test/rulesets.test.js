// Documents, rule sets, decisions and nodes: every native entry point.

import assert from 'node:assert/strict'
import fs from 'node:fs'
import path from 'node:path'
import { execFileSync } from 'node:child_process'
import { createRequire } from 'node:module'
import { describe, test } from 'node:test'
import { pathToFileURL } from 'node:url'
import { Worker } from 'node:worker_threads'

import cr, { build, deno, napiBuilt, native, root } from './support/cr.js'
import { INTERNAL } from '../lib/wasm-ready.js'
import { rules } from './support/probe.js'

const EXAMPLES = path.join(import.meta.dirname, '..', '..', '..', 'docs', 'examples')
const example = (name) => fs.readFileSync(path.join(EXAMPLES, name))
const DISCOUNTS_DATA = { customer: { years: 6, blocked: false }, order: { total: 2000 } }

const JS = path.join(import.meta.dirname, '..')
const require = createRequire(import.meta.url)
const pkg = JSON.parse(fs.readFileSync(path.join(JS, 'package.json'), 'utf8'))
const ENTRIES = build === 'wasm' ? ['index.wasm.js', 'native.wasm.js'] : ['index.js', 'native.js']

describe(`the package (${build})`, () => {
  test('versions', () => {
    assert.equal(cr.FORMAT_VERSION, 1)
    assert.equal(cr.version, pkg.version)
  })

  test('the root exports the idiomatic API; the native classes are in cleanrules/native', () => {
    assert.deepEqual(Object.keys(native).sort(), ['Decision', 'Document', 'FORMAT_VERSION', 'Node', 'RuleSet'])
    assert.deepEqual(Object.keys(root).sort(), [
      'ActionContext',
      'ActionError',
      'Cause',
      'CauseGroup',
      'CleanRulesError',
      'Const',
      'Counterfactual',
      'Decimal',
      'Decision',
      'DecisionTable',
      'EvaluationError',
      'Explanation',
      'Expr',
      'ExpressionError',
      'F',
      'FORMAT_VERSION',
      'Field',
      'FieldUsage',
      'Float',
      'FormatError',
      'HitPolicyError',
      'LocalDate',
      'MissingFieldError',
      'NaiveDateTime',
      'Note',
      'Reason',
      'RegistryError',
      'ResultTree',
      'Rule',
      'RuleDefinitionError',
      'RuleExplanation',
      'RuleNotFoundError',
      'RuleResult',
      'RuleSet',
      'RuleSummary',
      'RuleSyntaxError',
      'Summary',
      'TableOutput',
      'TableRow',
      'TypeMismatchError',
      'UnsupportedError',
      'ZonedDateTime',
      'and',
      'constant',
      'date',
      'datetime',
      'decimal',
      'dump',
      'dumps',
      'evaluateTree',
      'evaluateValueTree',
      'field',
      'float',
      'load',
      'loads',
      'not',
      'or',
      'ruleFromText',
      'version',
    ])
    // The root's RuleSet and Decision are the idiomatic ones, not the native classes.
    assert.notEqual(root.RuleSet, native.RuleSet)
    assert.notEqual(root.Decision, native.Decision)
    assert.equal(root.Document, undefined)
    assert.equal(root.Node, undefined)
  })

  const wasmBuilt = fs.existsSync(path.join(JS, 'wasm', 'cleanrules_bg.wasm'))
  const both = async () => [
    await import(pathToFileURL(path.join(JS, 'native.js')).href),
    await import(pathToFileURL(path.join(JS, 'native.wasm.js')).href),
  ]
  const roots = async () => [
    await import(pathToFileURL(path.join(JS, 'index.js')).href),
    await import(pathToFileURL(path.join(JS, 'index.wasm.js')).href),
  ]
  // The native layers themselves, internal functions included.
  const layers = async () => [
    (await import(pathToFileURL(path.join(JS, 'lib', 'napi.js')).href)).default,
    (await import(pathToFileURL(path.join(JS, 'lib', 'wasm.js')).href)).default,
  ]

  test('both builds export the same names, classes and members', { skip: !(napiBuilt && wasmBuilt) }, async () => {
    for (const [a, b] of [
      ['index.js', 'index.wasm.js'],
      ['native.js', 'native.wasm.js'],
    ]) {
      const names = async (file) => Object.keys(await import(pathToFileURL(path.join(JS, file)).href)).sort()
      assert.deepEqual(await names(a), await names(b), `${a} and ${b}`)
    }
    // The only differences: wasm-bindgen's `free()`, which releases an object
    // early (`[Symbol.dispose]()` does too, and does nothing on the addon), and
    // its two internal members, `__wrap` and `__destroy_into_raw`.
    const WASM_ONLY = new Set(['free', '__destroy_into_raw', 'static __wrap'])
    // On Node 22, the addon's constructors are plain functions with their own
    // `arguments` and `caller`; they belong to every such function, not to
    // the API.
    const INTRINSIC = new Set(['arguments', 'caller'])
    const members = (Class) =>
      [
        ...Object.getOwnPropertyNames(Class.prototype),
        ...Object.getOwnPropertySymbols(Class.prototype).map(String),
        ...Object.getOwnPropertyNames(Class)
          .filter((name) => !INTRINSIC.has(name))
          .map((name) => `static ${name}`),
      ].sort()
    const [napi, wasm] = await both()
    for (const name of ['Document', 'RuleSet', 'Decision', 'Node']) {
      assert.deepEqual(
        members(wasm[name]).filter((member) => !WASM_ONLY.has(member)),
        members(napi[name]),
        name,
      )
    }
    // The idiomatic API: each build makes its own classes, with the same members.
    const [napiRoot, wasmRoot] = await roots()
    for (const name of ['Expr', 'Field', 'Const', 'Rule', 'RuleSet', 'Decision', 'DecisionTable']) {
      assert.notEqual(napiRoot[name], wasmRoot[name], name)
      assert.deepEqual(members(wasmRoot[name]), members(napiRoot[name]), name)
    }
    for (const name of ['loads', 'dumps', 'load', 'dump', 'ruleFromText', 'evaluateTree', 'evaluateValueTree']) {
      assert.notEqual(napiRoot[name], wasmRoot[name], name)
      assert.equal(wasmRoot[name].length, napiRoot[name].length, name)
    }
    // The shared classes are the same objects.
    for (const name of ['Explanation', 'Summary', 'ZonedDateTime', 'CleanRulesError', 'ResultTree', 'TableOutput', 'TableRow', 'ActionContext']) {
      assert.equal(napiRoot[name], wasmRoot[name], name)
    }
    // The internal functions and classes the API uses (`src/*/build.rs`).
    const [napiLayer, wasmLayer] = await layers()
    for (const name of INTERNAL) {
      assert.equal(typeof napiLayer[name], 'function', `napi ${name}`)
      assert.equal(typeof wasmLayer[name], 'function', `wasm ${name}`)
    }
    const internal = (layer) =>
      Object.keys(layer)
        .filter((name) => /^(node|rule|decision|explanation|summary|action|document|write|grid|file|maxValues)[A-Z]|^(NodeList|RuleList|Native[A-Z]\w*|stringifiable)$/.test(name))
        .sort()
    assert.deepEqual(internal(napiLayer), [...INTERNAL].sort())
    assert.deepEqual(internal(wasmLayer), [...INTERNAL].sort())
    for (const name of ['NodeList', 'RuleList', 'NativeRule', 'NativeTable', 'NativeGrid', 'NativeTree']) {
      assert.deepEqual(
        members(wasmLayer[name]).filter((member) => !WASM_ONLY.has(member)),
        members(napiLayer[name]),
        name,
      )
    }
  })

  test('objects of the two builds do not mix: a TypeError names the other build', { skip: !(napiBuilt && wasmBuilt) }, async () => {
    const [napi, wasm] = await roots()
    const mixed = (call, from) =>
      assert.throws(call, (error) => {
        assert.ok(error instanceof TypeError, String(error))
        assert.match(error.message, new RegExp(`comes from the ${from} build of cleanrules`))
        assert.match(error.message, /import everything from one entry point/)
        return true
      })
    const napiNode = napi.F.x.gt(1)
    const napiRule = new napi.Rule('r', napiNode)
    mixed(() => new wasm.Rule('r', napiNode), 'Node addon')
    mixed(() => new wasm.RuleSet([napiRule]), 'Node addon')
    mixed(() => wasm.F.y.eq(napi.F.x), 'Node addon')
    mixed(() => wasm.and(wasm.F.y, napiNode), 'Node addon')
    mixed(() => wasm.F.y.sameAs(napiNode), 'Node addon')
    mixed(() => wasm.F.y.isIn([napi.constant(1)]), 'Node addon')
    mixed(() => new wasm.Rule('r', wasm.F.y, { v: napi.F.x }), 'Node addon')
    mixed(() => new napi.Rule('r', wasm.F.x), 'WebAssembly')
    // Within one build, nothing changes.
    assert.ok(napi.F.x.gt(1).sameAs(napiNode))
  })

  test('both builds build, refuse and give back the same with the idiomatic API', { skip: !(napiBuilt && wasmBuilt) }, async () => {
    const [napi, wasm] = await roots()
    const calls = [
      (r) => r.field(),
      (r) => r.field('a', -1),
      (r) => r.field(true),
      (r) => r.F.x.gt(NaN),
      (r) => r.F.x.gt(r.decimal('1e40')),
      (r) => r.F.x.gt(2n ** 64n),
      (r) => r.F.x.isIn([1, 'a']),
      (r) => r.F.x.matches('('),
      (r) => r.and(r.F.x),
      (r) => r.F.x.add(1).and(r.F.y),
      (r) => r.constant(null).not(),
      (r) => new r.Rule('r', r.F.x.add(1)),
      (r) => new r.Rule('r', r.F.x, { v: r.F.y.gt(1) }),
      (r) => new r.Rule('r', r.F.x, null, { do: '' }),
      (r) => new r.Rule('r', r.F.x, null, { priority: 2n ** 63n }),
      (r) => new r.Rule('r', r.F.x, null, { metadata: { d: r.decimal('1e40') } }),
      (r) => new r.RuleSet([], { hitPolicy: 'last' }),
      (r) => new r.RuleSet([], { aggregate: 'sum' }),
      (r) => new r.RuleSet([], { logic: 'fuzzy' }),
      (r) => new r.RuleSet([], { strict: 1 }),
      (r) => new r.RuleSet([new r.Rule('a', r.F.x), new r.Rule('a', r.F.y)]),
      (r) => new r.RuleSet([new r.Rule('a', r.F.x, null, { do: 'n' })]).bind({}),
      (r) => new r.RuleSet([new r.Rule('a', r.F.x, null, { do: () => { throw new Error('boom') } })]).run({ x: true }),
      (r) => new r.RuleSet([new r.Rule('a', r.F.x)], { hitPolicy: 'rule_order' }).evaluate({ x: true }).output,
      (r) => new r.RuleSet([new r.Rule('a', r.F.x)]).evaluate({ x: true }).whyNot('b'),
      (r) => new r.RuleSet([new r.Rule('a', r.F.x)]).rule('b'),
      (r) => new r.RuleSet([new r.Rule('a', r.F.x, null, { do: () => {} })]).toJSON(),
      (r) => r.F.x.between(1, 2, { close: 'left' }),
      // Documents, tables and trees (F.2b).
      (r) => r.loads('{"version": 1, "condition": {"op": "nope"}}'),
      (r) => r.loads('RULE r\nIF x >\nTHEN y = 1', { format: 'text' }),
      (r) => r.loads('x\n"1\n', { format: 'csv' }),
      (r) => r.loads('{}', { format: 'xml' }),
      (r) => r.loads(42),
      (r) => r.loads('RULE a IF x DO n', { format: 'text', actions: {} }),
      (r) => r.loads('{}', { limits: { maxSize: -1 } }),
      (r) => r.dumps(r.F.x.add(1)),
      (r) => r.dumps({}),
      (r) => r.dumps(new r.RuleSet([]), { format: 'csv' }),
      (r) => r.dumps(new r.Rule('a', r.F.x, null, { metadata: { m: 1 } }), { format: 'text' }),
      (r) => r.ruleFromText('IF a\nIF b'),
      (r) => r.ruleFromText('IF a', { do: 'n' }),
      (r) => r.ruleFromText(new Uint8Array([0xff])),
      (r) => new r.DecisionTable(['x', "THEN risk IN ('high', 'low')"], [['1', "'low'"], ['2', 'high']]),
      (r) => new r.DecisionTable(['x'], [['> )']]),
      (r) => new r.DecisionTable(['x'], [[1], [Number.NaN]]),
      (r) => new r.DecisionTable(['x', new r.TableOutput('y', [r.decimal('1e40')])], []),
      (r) => new r.DecisionTable(['x'], [[r.decimal('1e40')]]),
      (r) => new r.DecisionTable(['x'], [], { hitPolicy: 'sometimes' }),
      (r) => new r.DecisionTable(['x'], [], { strict: 1 }),
      (r) => new r.DecisionTable(['x'], [], { logic: 'nope' }),
      (r) => new r.DecisionTable(['x', 3], []),
      (r) => new r.DecisionTable(['x'], Array.from({ length: 10 }, () => ['> 1']), { limits: { maxTableSize: 7 } }),
      (r) => r.evaluateTree(r.F.x.gt(1), {}, { strict: true }),
      (r) => r.evaluateTree(r.F.x.add(1), {}),
      (r) => r.evaluateTree(r.F.x.gt(1), {}, { logic: 'nope' }),
      (r) => r.evaluateValueTree('x', {}),
    ]
    const outcome = (call, r) => {
      try {
        call(r)
        return 'no error'
      } catch (error) {
        return `${error.constructor.name}: ${error.message} (${error.hint})`
      }
    }
    for (const call of calls) {
      const expected = outcome(call, napi)
      assert.notEqual(expected, 'no error', String(call))
      assert.equal(outcome(call, wasm), expected, String(call))
    }
    // What comes back, explanations and summaries included.
    const result = (r) => {
      const rules = new r.RuleSet([
        new r.Rule('a', r.F.x.gt(10).and(r.F.d.lt(r.date(2026, 1, 1))), { v: r.F.x, d: r.F.d, z: r.F.z }, { metadata: { m: [1, 2n ** 60n] } }),
        new r.Rule('b', r.F.x.between(0, 5)),
      ])
      const decision = rules.evaluate({ x: 8, d: r.date(2026, 2, 1), z: new Date(0) })
      const explanation = decision.explain()
      const table = r.loads('HIT POLICY,priority\nRULE,x,THEN v,"THEN r IN (\'a\', \'b\')"\nhi,> 1,1.5,\'a\'\nlo,-,0,\'b\'\n', { format: 'csv' })
      const tree = r.evaluateTree(r.F.x.between(1, 5).and(r.F.y.isNull()), { x: 3, y: new Set() })
      const loaded = r.loads(r.dumps(rules, { format: 'yaml' }), { format: 'yaml' })
      const text = r.ruleFromText('RULE t PRIORITY 2 TAGS a IF x > 1 THEN v = x * 2 DO act')
      return JSON.stringify(
        [
          decision.toJSON(),
          explanation.toJSON(),
          String(explanation),
          explanation.rules.map(String),
          rules.summary().toJSON(),
          String(rules.summary()),
          rules.get('a').metadata,
          rules.toJSON(),
          r.dumps(loaded, { format: 'yaml' }),
          [table.toJSON(), r.dumps(table, { format: 'csv' }), table.rows, table.outputs, table.ruleset.toJSON()],
          table.evaluate({ x: 2 }).output,
          [tree.toJSON(), tree.truth, tree.children.map((child) => [String(child.node), child.truth])],
          [text.toJSON(), text.actionName, text.priority, text.tags],
        ],
        (_, value) => (typeof value === 'bigint' ? `${value}n` : value),
      )
    }
    assert.equal(result(wasm), result(napi))
  })

  test('both builds throw the same errors, of the same classes', { skip: !(napiBuilt && wasmBuilt) }, async () => {
    const [napi, wasm] = await both()
    const RULES = 'STRICT\n\nRULE a\nIF x > 1\nTHEN v = x\nDO act'
    const calls = [
      (n) => new n.RuleSet(),
      (n) => new n.Decision(),
      (n) => n.RuleSet.fromDocument({}),
      (n) => n.RuleSet.read(42),
      (n) => n.RuleSet.read('{}', 'json'),
      (n) => n.RuleSet.read('{}', { format: 7 }),
      (n) => n.RuleSet.read('{}', { format: 'xml' }),
      (n) => n.RuleSet.read('{}', { limits: 'big' }),
      (n) => n.RuleSet.read('{}', { limits: { maxSize: -1 } }),
      (n) => n.RuleSet.read('{}', { limits: { maxSize: '1' } }),
      (n) => n.RuleSet.read(RULES, { format: 'text' }).evaluate(),
      (n) => n.RuleSet.read(RULES, { format: 'text' }).evaluate({}),
      (n) => n.RuleSet.read(RULES, { format: 'text' }).evaluate({ x: 2 }).ruleName('0'),
      (n) => n.RuleSet.read(RULES, { format: 'text' }).evaluate({ x: 2 }).firedAt(3),
      (n) => n.RuleSet.read(RULES, { format: 'text' }).evaluateMany('rows'),
      (n) => n.RuleSet.read(RULES, { format: 'text' }).evaluateMany([{ x: 2 }, undefined]),
      (n) => n.RuleSet.read(RULES, { format: 'text' }).checkActions('act'),
      (n) => n.RuleSet.read(RULES, { format: 'text' }).checkActions([1]),
      (n) => n.RuleSet.read(RULES, { format: 'text' }).run({ x: 2 }, {}),
      (n) => n.RuleSet.read(RULES, { format: 'text' }).run({ x: 2 }, { act: 1 }),
      (n) => n.Node.read('x > 1', { format: 'text' }).evaluateTree({}, { strict: 'yes' }),
      (n) => n.Node.read('x > 1', { format: 'text' }).evaluateTree({}, { logic: 'nope' }),
      (n) => n.Document.read(new DataView(new ArrayBuffer(1))),
    ]
    const outcome = (call, n) => {
      try {
        call(n)
        return 'no error'
      } catch (error) {
        return `${error.constructor.name}: ${error.message}`
      }
    }
    for (const call of calls) {
      const expected = outcome(call, napi)
      assert.notEqual(expected, 'no error', String(call))
      assert.equal(outcome(call, wasm), expected, String(call))
    }
  })

  test('package.json picks the build by condition', () => {
    for (const [subpath, base] of [
      ['.', './index'],
      ['./native', './native'],
    ]) {
      const conditions = pkg.exports[subpath]
      assert.deepEqual(Object.keys(conditions), ['types', 'deno', 'browser', 'node', 'default'])
      assert.equal(conditions.node, `${base}.js`)
      assert.equal(conditions.deno, `${base}.deno.js`)
      for (const name of ['browser', 'default']) {
        assert.equal(conditions[name], `${base}.wasm.js`, `${subpath} ${name}`)
      }
    }
    // The WebAssembly build initialised by hand, in every runtime, and the
    // module itself, for bundlers that copy it.
    assert.deepEqual(pkg.exports['./wasm'], { types: './wasm.d.ts', default: './wasm.js' })
    assert.ok(pkg.files.includes('wasm.js') && pkg.files.includes('wasm.d.ts'))
    assert.equal(pkg.exports['./wasm/cleanrules_bg.wasm'], './wasm/cleanrules_bg.wasm')
    // Node resolves the package's own `exports` to the native addon.
    assert.equal(require.resolve('cleanrules'), path.join(JS, 'index.js'))
    assert.equal(require.resolve('cleanrules/native'), path.join(JS, 'native.js'))
  })

  // Where no prebuilt addon exists, Node runs the WebAssembly build with
  // `--conditions=browser`.
  test('node --conditions=browser loads the WebAssembly build', { skip: deno || !wasmBuilt }, () => {
    const code = `
      const native = await import('cleanrules/native')
      const root = await import('cleanrules')
      const ruleset = native.RuleSet.read('RULE a\\nIF x > 1\\nTHEN v = x', { format: 'text' })
      console.log(JSON.stringify([
        import.meta.resolve('cleanrules/native').endsWith('/native.wasm.js'),
        typeof native.RuleSet.prototype.free,
        root.version,
        ruleset.evaluate({ x: 2 }).outputJson(),
      ]))
    `
    const output = execFileSync(
      process.execPath,
      ['--conditions=browser', '--input-type=module', '-e', code],
      { cwd: JS, encoding: 'utf8' },
    )
    assert.deepEqual(JSON.parse(output), [true, 'function', pkg.version, '{"v":2}'])
  })

  test('CommonJS requires both entry points on Node (the native addon)', { skip: !napiBuilt }, () => {
    const code = `
      const root = require('cleanrules')
      const native = require('cleanrules/native')
      const ruleset = native.RuleSet.read('RULE a\\nIF x > 1\\nTHEN v = x', { format: 'text' })
      console.log(JSON.stringify([root.version, typeof root.MissingFieldError, ruleset.evaluate({ x: 2 }).outputJson()]))
    `
    const output = execFileSync(process.execPath, ['--no-warnings', '-e', code], { cwd: JS, encoding: 'utf8' })
    assert.deepEqual(JSON.parse(output), [pkg.version, 'function', '{"v":2}'])
  })

  test('an addon of another version is refused, naming the fix', { skip: build === 'wasm' || deno }, () => {
    const code = `
      const binding = require(${JSON.stringify(path.join(JS, 'binding.cjs'))})
      binding.version = () => '9.9.9'
      try { require(${JSON.stringify(path.join(JS, 'index.js'))}) } catch (error) { console.log(error.message) }
    `
    const output = execFileSync(process.execPath, ['--no-warnings', '-e', code], { encoding: 'utf8' })
    assert.match(output, /native addon is version 9\.9\.9, but the package is .*; reinstall cleanrules/)
  })

  test('works in a worker thread, with its own instance and hooks', async () => {
    const urls = ENTRIES.map((file) => pathToFileURL(path.join(JS, file)).href)
    const code = `
      const { parentPort } = require('node:worker_threads')
      Promise.all(${JSON.stringify(urls)}.map((url) => import(url))).then(([root, native]) => {
        const ruleset = native.RuleSet.read('STRICT\\n\\nRULE a\\nIF x > 1\\nTHEN v = x', { format: 'text' })
        let missing = false
        try { ruleset.evaluate({}) } catch (error) { missing = error instanceof root.MissingFieldError }
        parentPort.postMessage([ruleset.evaluate({ x: 2 }).outputJson(), missing])
      })
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
