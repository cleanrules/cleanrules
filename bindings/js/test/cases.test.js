// Case files and the case runner (task T.9b, spec/tests.md): `Expectation`,
// `Case`, `Cases` and `CaseFile` of the idiomatic API, `RuleSet.test`,
// `iterTest` and `CaseRun`, and `Document.test` of the native layer. The core
// reads, checks and runs every case; the binding builds and reads.

import assert from 'node:assert/strict'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { describe, test } from 'node:test'

import { native, root } from './support/cr.js'

const {
  Bundle,
  Case,
  CaseFile,
  CaseResult,
  CaseRun,
  Cases,
  DecisionTable,
  Decimal,
  Expectation,
  F,
  Float,
  FormatError,
  MissingFieldError,
  Mismatch,
  Rule,
  RuleSet,
  TestRun,
  UnsupportedError,
  canonicalBytes,
  decimal,
  digest,
  dumps,
  float,
  loads,
} = root

const EXAMPLES = path.join(import.meta.dirname, '..', '..', '..', 'docs', 'examples')
const RULES = fs.readFileSync(path.join(EXAMPLES, 'discounts.json'), 'utf8')
const CASES = fs.readFileSync(path.join(EXAMPLES, 'cases', 'discounts.json'), 'utf8')
const INSPECT = Symbol.for('nodejs.util.inspect.custom')

const discounts = () => loads(RULES)
const exampleCases = () => loads(CASES)

const ages = (strict = false) =>
  new RuleSet([new Rule('adult', F.age.ge(18), { band: 'adult' }), new Rule('child', F.age.lt(18), { band: 'child' }, { id: 'CHILD' })], {
    strict,
  })

const three = (filePath = null) =>
  new CaseFile(
    new Cases([
      new Case('a', { age: 30 }, { expect: new Expectation({ selected: ['adult'] }) }),
      new Case('b', { age: 3 }, { expect: new Expectation({ selected: ['adult'] }) }),
      new Case('c', {}, { expect: new Expectation({ output: { band: 'adult' } }) }),
    ]),
    { path: filePath },
  )

describe('case files', () => {
  test('a case file loads into frozen objects and dumps back', () => {
    const cases = exampleCases()
    assert.ok(cases instanceof Cases)
    assert.equal(cases.length, 4)
    assert.match(cases.description, /^the cases/)
    const vip = cases.get('vip-gets-15')
    assert.equal(vip, cases.items[0])
    assert.equal(cases.get('nobody'), undefined)
    assert.deepEqual(vip.input, { customer: { years: 7 }, order: { total: 1500 } })
    assert.deepEqual(vip.expect.selected, ['vip'])
    assert.ok(vip.expect.output.discount instanceof Decimal)
    assert.equal(vip.expect.output.discount.text, '0.15')
    assert.deepEqual(vip.expect.unknown, ['blocked'])
    assert.equal(vip.expect.outputs, undefined)
    assert.equal(vip.expect.error, undefined)
    assert.equal(cases.items[3].expect.output, null, 'nothing selected')
    assert.equal(cases.items[1].expect.outputs, undefined)
    assert.deepEqual([...cases].map((c) => c.id), cases.items.map((c) => c.id))
    for (const value of [cases, cases.items, vip, vip.input, vip.expect, vip.expect.output, cases.metadata]) {
      assert.ok(Object.isFrozen(value))
    }
    const text = dumps(cases)
    assert.ok(text.startsWith('{"version":1,"cases":{"description":'))
    assert.equal(dumps(loads(text)), text)
    assert.equal(dumps(loads(dumps(cases, { format: 'yaml' }), { format: 'yaml' })), text)
    assert.match(digest(cases), /^sha256:[0-9a-f]{64}$/)
    assert.deepEqual(canonicalBytes(cases), canonicalBytes(loads(text)))
    assert.equal(cases[INSPECT](), 'Cases("the cases of the discount rules (docs/examples/discounts.json)": 4 cases)')
    assert.equal(vip[INSPECT](), 'Case("vip-gets-15")')
    assert.equal(vip.expect[INSPECT](), 'Expectation(selected, output, unknown)')
    assert.throws(() => loads(CASES, { actions: {} }), { name: 'TypeError', message: /never run them/ })
    assert.throws(() => dumps(cases, { format: 'text' }), UnsupportedError)
  })

  test('floats are written explicitly, so they read back as floats', () => {
    const cases = new Cases([new Case('f', { rate: float(1) }, { expect: new Expectation({ output: { rate: float(2), n: 2 } }) })])
    const item = cases.items[0]
    assert.ok(item.input.rate instanceof Float, 'a float that holds an integer stays a float')
    assert.ok(item.expect.output.rate instanceof Float)
    assert.equal(item.expect.output.n, 2)
    const json = JSON.stringify(cases)
    assert.match(json, /"rate":\{"\$float":1\}/)
    assert.match(json, /"rate":\{"\$float":2\}/)
    assert.equal(dumps(loads(json)), dumps(cases), 'JSON.stringify keeps the document')
    assert.match(dumps(cases), /"rate":1\.0/)
    const rebuilt = new Cases([new Case(item.id, item.input, { expect: new Expectation({ output: item.expect.output }) })])
    assert.equal(dumps(rebuilt), dumps(cases))
  })

  test('the constructors refuse what the reader refuses', () => {
    const refused = [
      [() => new Case('', {}), /cannot be empty/],
      [() => new Cases([new Case('a', {}), new Case('a', {})]), /two cases have the id "a"/],
      [() => new Case('blob', { x: new Set() }), /input\["x"\] is not a data value/],
      [() => new Case('nan', { x: Number.NaN }), /input\["x"\] is NaN/],
      [() => new Expectation({ selected: ['a', 'a'] }), /named twice in "selected"/],
      [() => new Expectation({ unknown: [''] }), /cannot be an empty string/],
      [() => new Expectation({ error: 'evaluation_error' }), /raises only/],
      [() => new Expectation({ error: 'missing_field', selected: ['a'] }), /given alone/],
      [() => new Expectation({ output: { n: 2n ** 70n } }), /output\["n"\] is out of range/],
      [() => new Expectation({ outputs: [{}, { n: Number.NaN }] }), /outputs\[1\]\["n"\] is NaN/],
      [() => new Cases([], { metadata: { blob: new Set() } }), /metadata\["blob"\]/],
    ]
    for (const [make, message] of refused) assert.throws(make, (error) => error instanceof FormatError && message.test(error.message))
    const threw = new Error('no')
    const getter = {
      get x() {
        throw threw
      },
    }
    assert.throws(() => new Case('g', getter), (error) => error instanceof FormatError && error.cause === threw && /reading input\["x"\] threw/.test(error.message))
    assert.deepEqual(new Case('big', { n: 2n ** 70n }).input, { n: null }, 'an input out of range is null, as in JSON')
    assert.deepEqual(new Expectation({ unknown: ['z', 'b'] }).unknown, ['b', 'z'], 'sorted by code point')
    assert.ok(new Expectation().isEmpty)
    assert.ok(!new Expectation({ output: null }).isEmpty)
    assert.ok(new Case('x', null).expect.isEmpty)
  })

  test('wrong arguments are type and range errors', () => {
    assert.throws(() => new Expectation({ selected: 'vip' }), { name: 'TypeError', message: /array of rule ids/ })
    assert.throws(() => new Expectation({ unknown: [1] }), { name: 'TypeError', message: /rule ids as strings/ })
    assert.throws(() => new Expectation({ output: [1] }), { name: 'TypeError', message: /or null/ })
    assert.throws(() => new Expectation({ outputs: {} }), { name: 'TypeError', message: /array of objects/ })
    assert.throws(() => new Expectation({ outputs: [1] }), { name: 'TypeError', message: /holds objects/ })
    assert.throws(() => new Expectation({ error: 1 }), { name: 'TypeError', message: /error is a string/ })
    assert.throws(() => new Expectation({ error: 'nope' }), { name: 'RangeError', message: /unknown error code/ })
    assert.throws(() => new Expectation({ selcted: [] }), { name: 'TypeError', message: /unknown option "selcted"/ })
    assert.throws(() => new Case(1, {}), { name: 'TypeError', message: /case id is a string/ })
    assert.throws(() => new Case('a'), { name: 'TypeError', message: /input is undefined/ })
    assert.throws(() => new Case('a', {}, { expect: {} }), { name: 'TypeError', message: /expect is an Expectation/ })
    assert.throws(() => new Case('a', {}, { description: 1 }), { name: 'TypeError', message: /description is a string/ })
    assert.throws(() => new Cases('a'), { name: 'TypeError', message: /array of Case objects/ })
    assert.throws(() => new Cases([{}]), { name: 'TypeError', message: /holds Case objects/ })
    assert.throws(() => new Cases([], { metadata: [1] }), { name: 'TypeError', message: /metadata is an object/ })
    assert.throws(() => new CaseFile([]), { name: 'TypeError', message: /holds Cases/ })
    assert.throws(() => CaseFile.read(1, { path: 'a.json' }), { name: 'TypeError', message: /string or a Uint8Array/ })
    assert.throws(() => CaseFile.read('{}', { path: 'a.json', format: 'csv' }), { name: 'RangeError', message: /JSON or YAML/ })
    assert.deepEqual(new Cases().items, [])
  })

  test('a case file keeps its path, digest and error', async () => {
    const read = CaseFile.read(CASES, { path: 'cases/discounts.json' })
    assert.equal(read.path, 'cases/discounts.json')
    assert.match(read.digest, /^sha256:/)
    assert.equal(read.error, null)
    assert.equal(dumps(read.cases), dumps(exampleCases()))
    assert.equal(read.case('vip-gets-15').id, 'vip-gets-15')
    assert.equal(read.case('nobody'), undefined)
    const built = new CaseFile(exampleCases())
    assert.equal(built.path, null)
    assert.equal(built.digest, null)
    const bad = CaseFile.read('[]', { path: 'bad.json' })
    assert.equal(bad.cases, null)
    assert.equal(bad.case('a'), undefined)
    assert.ok(bad.error instanceof FormatError)
    assert.equal(bad.error.context.pointer, '')
    assert.equal(CaseFile.read(dumps(exampleCases(), { format: 'yaml' }), { path: 'c.YML' }).cases.length, 4, 'YAML by its suffix')
    assert.equal(CaseFile.read(dumps(exampleCases(), { format: 'yaml' }), { format: 'yaml' }).cases.length, 4)
    assert.match(CaseFile.read(RULES, { path: 'rules.json' }).error.message, /not test cases/)
    assert.ok(Object.isFrozen(read))
    assert.equal(read[INSPECT](), 'CaseFile(cases/discounts.json: 4 cases)')
    assert.equal(bad[INSPECT](), 'CaseFile(bad.json: unreadable)')
    assert.equal(built[INSPECT](), 'CaseFile(without a path: 4 cases)')
    const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'cleanrules-cases-'))
    try {
      const file = path.join(dir, 'cases.json')
      fs.writeFileSync(file, CASES)
      const loaded = await CaseFile.load(file)
      assert.equal(loaded.digest, read.digest)
      assert.equal(loaded.path, file.split(path.sep).join('/'))
      assert.equal((await CaseFile.load(new URL(`file://${file}`))).digest, read.digest)
      await assert.rejects(CaseFile.load(path.join(dir, 'missing.json')))
      await assert.rejects(CaseFile.load(1), { name: 'TypeError' })
    } finally {
      fs.rmSync(dir, { recursive: true })
    }
  })
})

describe('RuleSet.test', () => {
  test('the example cases pass against the example rules', () => {
    const run = discounts().test(CaseFile.read(CASES, { path: 'cases/discounts.json' }))
    assert.ok(run instanceof TestRun)
    assert.ok(run.allPassed)
    assert.deepEqual({ ...run.summary }, { total: 4, passed: 4, failed: 0, error: 0, partial: false, interrupted: null })
    assert.equal(run.source, 'ruleset')
    assert.equal(run.hitPolicy, 'priority')
    assert.equal(run.logic, 'boolean')
    assert.equal(run.strict, false)
    assert.equal(run.aggregate, null)
    assert.equal(run.rulesetDigest, null)
    assert.equal(run.bundle, null)
    assert.equal(run.engine.name, 'cleanrules')
    assert.equal(run.engine.limits.maxSize, 16 * 1024 * 1024)
    assert.deepEqual({ ...run.limits }, { maxExplained: 5, maxResults: null, maxSteps: null })
    assert.equal(run.files[0].path, 'cases/discounts.json')
    assert.ok(run.results.every((r) => r instanceof CaseResult && r.status === 'passed'))
    assert.ok(String(run).startsWith('Test run of a rule set: 3 rules, hit policy priority'))
    assert.ok(String(run).endsWith('4 results: 4 passed, 0 failed, 0 errors\n'))
    const document = run.toJSON()
    assert.equal(document.version, 1)
    assert.equal(document.test_run.summary.total, 4)
    assert.equal(run[INSPECT](), 'TestRun(rule set: 4 passed, 0 failed, 0 errors)')
    assert.ok(Object.isFrozen(run) && Object.isFrozen(run.results))
    assert.throws(() => dumps(run), { name: 'TypeError', message: /output only/ })
    assert.equal(discounts().test(exampleCases(), { rulesetDigest: true }).rulesetDigest, digest(discounts()))
  })

  test('failures list their mismatches; errors and unreadable files are results', () => {
    const cases = new Cases([
      new Case('every-key', { customer: { years: 7 }, order: { total: 1500 } }, {
        expect: new Expectation({ selected: ['loyal'], output: { discount: decimal('0.05') }, outputs: [{ discount: decimal('0.05') }], unknown: [] }),
      }),
      new Case('no-error', { customer: { years: 1 } }, { expect: new Expectation({ error: 'hit_policy_error' }) }),
    ])
    const run = discounts().test([cases, CaseFile.read('[]', { path: 'bad.json' })], { limits: { maxExplained: 1 } })
    assert.ok(!run.allPassed)
    const [every, noError, bad] = run.results
    assert.deepEqual(every.mismatches.map((m) => m.key), ['selected', 'output', 'outputs', 'unknown'])
    assert.ok(every.mismatches.every((m) => m instanceof Mismatch && Object.isFrozen(m)))
    assert.deepEqual(every.mismatches[0].expected, ['loyal'])
    assert.deepEqual(every.mismatches[0].actual, ['vip'])
    assert.equal(every.mismatches[1].actual.discount.text, '0.15')
    assert.equal(every.mismatches[2].actual[0].discount.text, '0.15')
    assert.equal(every.mismatches[1].reason, null)
    assert.match(every.explanationText, /HIT POLICY priority/)
    assert.equal(noError.explanationText, null, 'maxExplained: 1')
    assert.equal(noError.mismatches[0].expected, 'hit_policy_error')
    assert.equal(noError.mismatches[0].actual, null)
    assert.equal(bad.status, 'error')
    assert.equal(bad.id, null)
    assert.ok(bad.error instanceof FormatError)
    assert.deepEqual(bad.toJSON(), run.toJSON().test_run.results[2])
    assert.equal(bad[INSPECT](), 'CaseResult(bad.json: error)')
    assert.equal(every[INSPECT](), 'CaseResult("every-key": failed)')
    assert.match(String(run), /FAILED {2}"every-key"/)
    assert.match(String(run), /ERROR {3}bad.json/)
  })

  test('an unexpected error is the case result; an expected one passes', () => {
    const run = ages(true).test(new Cases([new Case('no-age', {}), new Case('young', { age: 1 })]))
    assert.equal(run.results[0].status, 'error')
    assert.ok(run.results[0].error instanceof MissingFieldError)
    assert.equal(run.results[1].status, 'passed')
    assert.ok(ages(true).test(new Cases([new Case('no-age', {}, { expect: new Expectation({ error: 'missing_field' }) })])).allPassed)
    const wrong = ages(true).test(new Cases([new Case('no-age', {}, { expect: new Expectation({ error: 'type_mismatch' }) })]))
    assert.ok(wrong.results[0].mismatches[0].actual instanceof MissingFieldError)
  })

  test('a single output of a multi-hit decision has a reason instead', () => {
    const rules = new RuleSet([new Rule('a', F.x.gt(0), { n: 1 }), new Rule('b', F.x.gt(1), { n: 2 })], { hitPolicy: 'rule_order' })
    const [mismatch] = rules.test(new Cases([new Case('two', { x: 2 }, { expect: new Expectation({ output: { n: 1 } }) })])).results[0].mismatches
    assert.equal(mismatch.actual, undefined)
    assert.equal(mismatch.reason, 'the decision has no single output (rule_order)')
  })

  test('a decision table runs through its rule set', () => {
    const table = loads(
      JSON.stringify({
        version: 1,
        decision_table: {
          hit_policy: 'unique',
          inputs: ['age'],
          outputs: [{ name: 'band' }],
          rows: [
            { inputs: ['>= 18'], outputs: ["'adult'"] },
            { inputs: ['< 18'], outputs: ["'child'"] },
          ],
        },
      }),
    )
    assert.ok(table instanceof DecisionTable)
    const run = table.test(new Cases([new Case('adult', { age: 30 }, { expect: new Expectation({ output: { band: 'adult' } }) })]), { rulesetDigest: true })
    assert.ok(run.allPassed)
    assert.equal(run.source, 'decision_table')
    assert.equal(run.rulesetDigest, digest(table.ruleset))
    const partial = table.iterTest(new Cases([new Case('a', { age: 1 }), new Case('b', { age: 2 })]), { limits: { maxResults: 1 } })
    assert.deepEqual([...partial].map((r) => r.id), ['a'])
    assert.deepEqual({ ...partial.testRun().summary.interrupted }, { reason: 'max_results', remaining: 1 })
  })
})

describe('iterTest and CaseRun', () => {
  test('leaving the loop early gives a partial, cancelled run', () => {
    const run = ages().iterTest([three('one.json'), three('two.json')])
    assert.ok(run instanceof CaseRun)
    assert.equal(run.remaining, 6)
    assert.equal(run.isFinished, false)
    for (const result of run) {
      assert.equal(result.id, 'a')
      break
    }
    assert.equal(run.isFinished, true, 'leaving the loop stops the run')
    const report = run.testRun()
    assert.equal(report.summary.total, 1)
    assert.deepEqual({ ...report.summary.interrupted }, { reason: 'cancelled', remaining: 5 })
    assert.equal(run.testRun(), report, 'made once')
    assert.deepEqual(run.next(), { value: undefined, done: true })
    assert.equal(run.remaining, 0)
    assert.equal(run.summary.total, 1)
    assert.ok(!report.allPassed)
    assert.match(String(report), /Partial run: interrupted by cancelled, 5 not run/)
    assert.match(report[INSPECT](), /partial/)
    assert.equal(run[INSPECT](), 'CaseRun(1 results, finished)')
    assert.throws(() => new CaseRun(), { name: 'TypeError', message: /no public constructor/ })
  })

  test('asking for the test run stops a run that is not finished', () => {
    const run = ages().iterTest(three())
    assert.equal(run.next().value.id, 'a')
    assert.equal(run[INSPECT](), 'CaseRun(1 results, 2 remaining)')
    assert.deepEqual({ ...run.testRun().summary.interrupted }, { reason: 'cancelled', remaining: 2 })
  })

  test('cancel and signal are asked between results', () => {
    let stop = false
    let asked = 0
    const run = ages().iterTest(three(), {
      cancel: () => {
        asked += 1
        return stop
      },
    })
    assert.equal(run.next().value.id, 'a')
    assert.equal(asked, 1)
    stop = true
    assert.deepEqual([...run], [])
    assert.deepEqual({ ...run.testRun().summary.interrupted }, { reason: 'cancelled', remaining: 2 })
    const controller = new AbortController()
    const signalled = ages().iterTest(three(), { signal: controller.signal })
    signalled.next()
    controller.abort()
    assert.equal(signalled.next().done, true)
    assert.equal(signalled.testRun().summary.interrupted.remaining, 2)
    const done = ages().test(three(), { cancel: () => false })
    assert.equal(done.summary.partial, false, 'a run that gave its last result is complete')
  })

  test('a cancel that throws stops the run and its error is thrown', () => {
    const lost = new Error('deadline lost')
    const run = ages().iterTest(three(), {
      cancel: () => {
        throw lost
      },
    })
    assert.throws(() => run.next(), (error) => error === lost)
    assert.deepEqual({ ...run.testRun().summary.interrupted }, { reason: 'cancelled', remaining: 3 })
    assert.throws(
      () =>
        ages().test(three(), {
          cancel: () => {
            throw lost
          },
        }),
      (error) => error === lost,
    )
    let inner
    const nested = ages().iterTest(three(), {
      cancel: () => {
        nested.next()
        return false
      },
    })
    assert.throws(
      () => {
        inner = nested.next()
      },
      { name: 'TypeError', message: /own cancel callback/ },
    )
    assert.equal(inner, undefined)
  })

  test('cancel() and the limits', () => {
    const run = ages().iterTest(three())
    run.cancel()
    assert.deepEqual([...run], [])
    assert.deepEqual({ ...run.summary.interrupted }, { reason: 'cancelled', remaining: 3 })
    const steps = ages().test(three(), { limits: { maxSteps: 5 } })
    assert.equal(steps.summary.total, 2)
    assert.deepEqual({ ...steps.summary.interrupted }, { reason: 'max_steps', remaining: 1 })
    assert.deepEqual({ ...steps.limits }, { maxExplained: 5, maxResults: null, maxSteps: 5 })
    const none = ages().test(three(), { limits: { maxExplained: 0 } })
    assert.ok(none.results.every((r) => r.explanationText === null))
  })

  test('options and files are checked', () => {
    assert.throws(() => ages().test(three(), { limits: { maxResults: -1 } }), { name: 'RangeError' })
    assert.throws(() => ages().test(three(), { limits: { maxSteps: 1.5 } }), { name: 'RangeError' })
    assert.throws(() => ages().test(three(), { limits: { maxSteps: '1' } }), { name: 'TypeError' })
    assert.throws(() => ages().test(three(), { limits: { maxRows: 1 } }), { name: 'TypeError', message: /unknown option "maxRows"/ })
    assert.throws(() => ages().test(three(), { cancel: 1 }), { name: 'TypeError', message: /cancel is a function/ })
    assert.throws(() => ages().test(three(), { signal: {} }), { name: 'TypeError', message: /AbortSignal/ })
    assert.throws(() => ages().test(three(), { rulesetDigest: 1 }), { name: 'TypeError', message: /rulesetDigest/ })
    assert.throws(() => ages().test(three(), { limit: 1 }), { name: 'TypeError', message: /unknown option/ })
    assert.throws(() => ages().test('cases.json'), { name: 'TypeError', message: /array of CaseFile/ })
    assert.throws(() => ages().test(['cases.json']), { name: 'TypeError', message: /CaseFile.load/ })
    assert.throws(() => ages().test([{}]), { name: 'TypeError', message: /a case file is a CaseFile or Cases/ })
    const run = ages().test([three('a.json'), three('a.json'), three(), three()])
    const errors = run.results.filter((r) => r.status === 'error').map((r) => [r.path, r.id])
    assert.deepEqual(errors, [['a.json', null], [null, null]])
  })
})

describe('Bundle.test', () => {
  const good = dumps(new Cases([new Case('adult', { age: 30 }, { expect: new Expectation({ selected: ['adult'] }) })]))

  test("a bundle's case files run against its rules, checked against their digests", () => {
    const bundle = new Bundle(ages(), { id: 'acme/ages', revision: '1', cases: [{ path: 'cases/good.json', content: good }] })
    const read = []
    const run = bundle.test((ref) => {
      read.push(ref.path)
      return new TextEncoder().encode(good)
    })
    assert.deepEqual(read, ['cases/good.json'])
    assert.ok(run.allPassed)
    assert.deepEqual({ ...run.bundle }, { id: 'acme/ages', revision: '1' })
    assert.equal(run.results[0].path, 'cases/good.json')
    const changed = bundle.test(() => `${good} `)
    assert.equal(changed.results[0].error.code, 'decision_mismatch')
    assert.equal(changed.results[0].error.context.pointer, '/bundle/cases/0/digest')
    const partial = bundle.iterTest(() => good, { limits: { maxResults: 0 } })
    assert.deepEqual([...partial], [])
    assert.deepEqual({ ...partial.testRun().summary.interrupted }, { reason: 'max_results', remaining: 1 })
    assert.throws(() => bundle.test(() => 1), { name: 'TypeError', message: /string or a Uint8Array/ })
    assert.throws(() => bundle.test('dir'), { name: 'TypeError', message: /function given each CaseFileRef/ })
    assert.throws(() => bundle.test(() => Promise.resolve(good)), { name: 'TypeError', message: /promise/ })
    const failure = new Error('cannot open')
    assert.throws(
      () =>
        bundle.test(() => {
          throw failure
        }),
      (error) => error === failure,
    )
  })
})

describe('Document.test of the native layer', () => {
  const { Document } = native

  test('runs case files against the rules a document holds, as text, JSON or YAML', () => {
    const rules = Document.read(RULES)
    const text = rules.test([{ path: 'cases/discounts.json', content: CASES }])
    assert.equal(text, String(discounts().test(CaseFile.read(CASES, { path: 'cases/discounts.json' }))))
    const json = JSON.parse(rules.test([{ path: 'cases/discounts.json', content: new TextEncoder().encode(CASES) }], { format: 'json', rulesetDigest: true }))
    assert.equal(json.test_run.summary.passed, 4)
    assert.equal(json.test_run.ruleset_digest, digest(discounts()))
    assert.match(rules.test([{ cases: Document.read(CASES) }], { format: 'yaml' }), /^version: 1\ntest_run:/)
    const partial = JSON.parse(rules.test([{ content: CASES }], { format: 'json', limits: { maxResults: 1, maxSteps: 100, maxExplained: 0 } }))
    assert.deepEqual(partial.test_run.summary.interrupted, { reason: 'max_results', remaining: 3 })
    let asked = 0
    const cancelled = JSON.parse(
      rules.test([{ content: CASES }], {
        format: 'json',
        cancel: () => {
          asked += 1
          return asked > 2
        },
      }),
    )
    assert.deepEqual(cancelled.test_run.summary.interrupted, { reason: 'cancelled', remaining: 2 })
    assert.match(rules.test(), /0 results: 0 passed, 0 failed, 0 errors/)
  })

  test('a bundle finds its files by path', () => {
    const good = dumps(new Cases([new Case('adult', { age: 30 })]))
    const bundle = Document.read(dumps(new Bundle(ages(), { id: 'acme/ages', revision: '1', cases: [{ path: 'cases/good.json', content: good }] })))
    const run = JSON.parse(bundle.test([{ path: 'cases/good.json', content: good }], { format: 'json' }))
    assert.equal(run.test_run.bundle.id, 'acme/ages')
    assert.throws(() => bundle.test([]), { name: 'RangeError', message: /not among the files given/ })
    assert.throws(() => bundle.test([{ path: 'cases/good.json', cases: Document.read(good) }]), { name: 'TypeError', message: /as content/ })
  })

  test('its arguments are checked', () => {
    const rules = Document.read(RULES)
    const errors = [
      [() => Document.read(CASES).test([]), 'TypeError', /a rule set, a decision table or a bundle, not a case file/],
      [() => rules.test('x'), 'TypeError', /files/],
      [() => rules.test([1]), 'TypeError', /files\[0\] is an object/],
      [() => rules.test([{ path: 1, content: '' }]), 'TypeError', /files\[0\].path is a string/],
      [() => rules.test([{ content: 1 }]), 'TypeError', /files\[0\].content must be a string or bytes/],
      [() => rules.test([{ content: '', cases: Document.read(CASES) }]), 'TypeError', /content or cases, not both/],
      [() => rules.test([{ cases: rules }]), 'TypeError', /files\[0\].cases is a Document that holds a case file, not a rule set/],
      [() => rules.test([{ cases: {} }]), 'TypeError', /holds a case file, not another object/],
      [() => rules.test([], 1), 'TypeError', /options must be an object/],
      [() => rules.test([], { limits: 1 }), 'TypeError', /limits must be an object/],
      [() => rules.test([], { limits: { maxResults: -1 } }), 'RangeError', /limits.maxResults/],
      [() => rules.test([], { format: 'csv' }), 'UnsupportedError', /./],
      [() => rules.test([], { cancel: 1 }), 'TypeError', /options.cancel is a function/],
      [() => rules.test([{ content: CASES }], { cancel: () => 1 }), 'TypeError', /must return a boolean/],
    ]
    for (const [call, name, message] of errors) {
      assert.throws(call, (error) => (error.name === name || error.constructor.name === name) && message.test(error.message), String(message))
    }
    const thrown = new Error('stop')
    assert.throws(
      () =>
        rules.test([{ content: CASES }], {
          cancel: () => {
            throw thrown
          },
        }),
      (error) => error === thrown,
    )
  })
})

describe('from the API review', () => {
  test('the run cannot be read from its own cancel callback; cancel() there stops it', () => {
    let run
    let refused = 0
    run = ages().iterTest(three(), {
      cancel: () => {
        for (const read of [() => run.summary, () => run.remaining, () => run.isFinished, () => run.testRun(), () => run.next()]) {
          assert.throws(read, { name: 'TypeError', message: /cannot be (read|advanced) from its own cancel callback/ })
          refused += 1
        }
        assert.equal(run[INSPECT](), 'CaseRun(running)')
        return false
      },
    })
    assert.equal(run.next().value.id, 'a')
    assert.equal(refused, 5)
    assert.equal(run.summary.total, 1, 'the run is intact')
    assert.equal(run.remaining, 2)
    // The natural trigger: stop after the first failure.
    const stopping = ages().iterTest(three(), { cancel: () => stopping.cancel() })
    assert.deepEqual([...stopping], [])
    const report = stopping.testRun()
    assert.deepEqual({ ...report.summary.interrupted }, { reason: 'cancelled', remaining: 3 })
    assert.equal(report.summary.total, 0)
    assert.equal(report.allPassed, false)
  })

  test('test() runs the rest in the core, and honours cancel()', () => {
    let asked = 0
    const run = ages().test(three(), {
      cancel: () => {
        asked += 1
        return asked > 1
      },
    })
    assert.equal(run.summary.total, 1)
    assert.equal(run.summary.partial, true)
  })

  test('leaving a for...of loop stops the run, unlike a Python break', () => {
    const run = ages().iterTest(three())
    for (const result of run) {
      assert.equal(result.id, 'a')
      break
    }
    assert.equal(run.isFinished, true)
    assert.deepEqual([...run], [])
    assert.deepEqual({ ...run.testRun().summary.interrupted }, { reason: 'cancelled', remaining: 2 })
  })

  test('a run has the iterator helpers where the runtime has them', { skip: typeof Iterator === 'undefined' || typeof Iterator.prototype.take !== 'function' }, () => {
    const run = ages().iterTest(three())
    assert.deepEqual(run.take(2).map((r) => r.id).toArray(), ['a', 'b'])
    assert.ok(run instanceof Iterator)
  })

  test('parts left out are undefined; only output takes null; toJSON writes the fragment', () => {
    const expect = new Expectation({ selected: ['vip'], output: { rate: float(1) } })
    assert.equal(expect.outputs, undefined)
    assert.equal(expect.unknown, undefined)
    assert.equal(expect.error, undefined)
    assert.deepEqual(JSON.parse(JSON.stringify(expect)), { selected: ['vip'], output: { rate: { $float: 1 } } })
    assert.deepEqual(new Expectation().toJSON(), {})
    assert.equal(new Expectation({ output: null }).output, null)
    for (const name of ['selected', 'outputs', 'unknown', 'error']) {
      assert.throws(() => new Expectation({ [name]: null }), { name: 'TypeError', message: new RegExp(`${name} is not checked when it is left out`) })
    }
    const item = new Case('a', { x: float(2) }, { expect, description: 'one' })
    assert.deepEqual(item.toJSON(), {
      id: 'a',
      description: 'one',
      input: { x: { $float: 2 } },
      expect: { selected: ['vip'], output: { rate: { $float: 1 } } },
    })
    const loaded = exampleCases().items[3]
    assert.deepEqual(loaded.toJSON(), JSON.parse(CASES).cases.items[3])
    assert.deepEqual(loaded.expect.toJSON(), JSON.parse(CASES).cases.items[3].expect)
  })

  test('a mismatch gives back an expected float as a float', () => {
    const rules = new RuleSet([new Rule('a', F.x.gt(0), { n: 1 })])
    const [mismatch] = rules.test(new Cases([new Case('f', { x: 1 }, { expect: new Expectation({ output: { n: float(1) } }) })])).results[0].mismatches
    assert.ok(mismatch.expected.n instanceof Float)
    assert.equal(mismatch.actual.n, 1)
  })

  test("the core's answers: total, partial, allPassed and the engine", () => {
    const run = ages().test(three(), { limits: { maxResults: 1 } })
    assert.deepEqual([run.summary.total, run.summary.partial, run.allPassed, run.engine.isCurrent], [1, true, false, true])
  })

  test('a file: URL loads by its path', async () => {
    const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'cleanrules-url-'))
    try {
      const file = path.join(dir, 'a b.json')
      fs.writeFileSync(file, CASES)
      const { pathToFileURL } = await import('node:url')
      const loaded = await CaseFile.load(pathToFileURL(file))
      assert.equal(loaded.path, file.split(path.sep).join('/'))
      await assert.rejects(CaseFile.load(new URL('https://example.com/a.json')), { name: 'TypeError' })
    } finally {
      fs.rmSync(dir, { recursive: true })
    }
  })

  test("a bundle's files are loaded up to the core's bound", () => {
    const good = dumps(new Cases([new Case('adult', { age: 30 })]))
    const bundle = new Bundle(ages(), { id: 'acme/ages', revision: '1', cases: [{ path: 'cases/good.json', content: good }], limits: { maxSize: 10 } })
    const [result] = bundle.test(() => good).results
    // Checked against its digest, then read under the bundle's limit.
    assert.equal(result.error.code, 'limit_exceeded')
    const [big] = bundle.test(() => good + ' '.repeat(17 << 20)).results
    assert.equal(big.error.code, 'limit_exceeded')
    assert.match(big.error.message, /more than the limit of 10/)
  })

  test('the native layer refuses cases that are not a Document, a string included', () => {
    const rules = native.Document.read(RULES)
    assert.throws(() => rules.test([{ cases: 'text' }]), { name: 'TypeError', message: /files\[0\]\.cases is a Document that holds a case file, not a string/ })
  })
})
