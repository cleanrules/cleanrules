// The static audit (task T.9a, spec/audit.md): `RuleSet.audit` and
// `DecisionTable.audit` of the idiomatic API, and `RuleSet.audit` and
// `Document.audit` of the native layer. The core audits; the binding reads
// its report.

import assert from 'node:assert/strict'
import { describe, test } from 'node:test'

import { native, root } from './support/cr.js'

const {
  Audit,
  AuditedRule,
  CheckResult,
  DecisionTable,
  Diagnostic,
  F,
  HitPolicyError,
  LocalDate,
  NotModelled,
  Rule,
  RuleSet,
  Undecided,
  UnsupportedError,
  date,
  decimal,
  digest,
  dumps,
} = root

const ages = (hitPolicy = 'unique') =>
  new RuleSet(
    [new Rule('adult', F.age.ge(18), { band: 'adult' }), new Rule('senior', F.age.ge(65), { band: 'senior' }, { id: 'AGE-2' })],
    { hitPolicy },
  )

const INSPECT = Symbol.for('nodejs.util.inspect.custom')

describe('RuleSet.audit', () => {
  test('an overlap under unique is an error whose witness throws a HitPolicyError', () => {
    const rules = ages()
    const report = rules.audit()
    assert.ok(report instanceof Audit)
    const [overlap, gap] = report.diagnostics
    assert.ok(overlap instanceof Diagnostic)
    assert.equal(overlap.code, 'overlap')
    assert.equal(overlap.severity, 'error')
    assert.deepEqual(overlap.rules, ['adult', 'senior'])
    assert.match(overlap.message, /hit_policy_error/)
    assert.throws(
      () => rules.evaluate(overlap.witness),
      (error) => error instanceof HitPolicyError && ['adult', 'senior'].every((r) => error.context.rules.includes(r)),
    )
    assert.equal(gap.code, 'gap')
    assert.deepEqual(gap.rules, [])
    assert.deepEqual(rules.evaluate(gap.witness).fired, [])
    assert.equal(report.maxSeverity, 'error')
    assert.equal(report.source, 'ruleset')
    assert.equal(report.document, null)
    assert.equal(report.hitPolicy, 'unique')
    assert.equal(report.logic, 'boolean')
    assert.equal(report.strict, false)
    assert.equal(report.aggregate, null)
    assert.equal(report.rulesetDigest, null)
    assert.deepEqual(report.limits, { maxTerms: 256, maxSteps: 1000000 })
    assert.ok(report.rules.every((r) => r instanceof AuditedRule))
    assert.deepEqual(
      report.rules.map((r) => [r.name, r.id, r.row]),
      [
        ['adult', null, null],
        ['senior', 'AGE-2', null],
      ],
    )
    assert.ok(report.checks.every((c) => c instanceof CheckResult))
    assert.deepEqual(
      report.checks.map((c) => c.check),
      ['types', 'fields', 'unsatisfiable', 'shadowed', 'overlap', 'gap'],
    )
    assert.equal(report.status('overlap'), 'complete')
    assert.equal(report.status('fields'), 'not_applicable')
    assert.equal(report.status('regions'), 'not_applicable')
    assert.deepEqual([report.notModelled.length, report.undecided.length], [0, 0])
    assert.ok(Object.isFrozen(report) && Object.isFrozen(overlap) && Object.isFrozen(overlap.witness))
  })

  test('the text, the JSON form and the inspection', () => {
    const report = ages().audit()
    const text = String(report)
    assert.equal(text.split('\n')[0], 'Audit of a rule set: 2 rules, hit policy unique, logic boolean')
    assert.match(text, /error {4}overlap {2}adult, senior: rules adult and senior both fire/)
    assert.match(text, / {11}witness: \{"age":/)
    assert.equal(text, native.RuleSet.read(JSON.stringify(ages())).audit())
    // Each diagnostic's text is the core's, as the audit's text has it.
    const first = String(report.diagnostics[0])
    assert.match(first, /^error {4}overlap {2}adult, senior: rules adult/)
    assert.match(first, /\n {11}witness: \{"age":\d+\}\n$/)
    assert.ok(report.diagnostics.every((d) => text.includes(String(d))))
    assert.equal(report.atLeast('error'), true)
    assert.throws(() => report.atLeast('fatal'), RangeError)
    assert.throws(() => report.atLeast(2), RangeError)
    assert.throws(() => dumps(report), (e) => e instanceof TypeError && /an audit is output only.*JSON\.stringify\(report\)/.test(e.message))
    const json = report.toJSON()
    assert.equal(json.version, 1)
    assert.equal(json.audit.diagnostics[0].code, 'overlap')
    assert.deepEqual(json.audit.diagnostics[0].witness, report.diagnostics[0].witness)
    // A copy each time.
    json.audit.rules.length = 0
    assert.equal(report.toJSON().audit.rules.length, 2)
    assert.equal(JSON.parse(JSON.stringify(report)).audit.source, 'ruleset')
    assert.equal(report[INSPECT](), 'Audit(rule set: 2 diagnostics, max severity error)')
    assert.equal(report[Symbol.for('Deno.customInspect')](), report[INSPECT]())
  })

  test('witnesses are native data, and JSON in the document', () => {
    const rules = new RuleSet(
      [new Rule('a', F.price.gt(decimal('0.5')).and(F.day.ge(date(2026, 1, 1)))), new Rule('b', F.price.gt(decimal('0.7')))],
      { hitPolicy: 'first' },
    )
    const report = rules.audit()
    const [overlap, gap] = report.diagnostics
    assert.equal(overlap.severity, 'info')
    assert.match(overlap.message, /a takes precedence over b/)
    assert.deepEqual([...rules.evaluate(overlap.witness).fired].sort(), ['a', 'b'])
    assert.deepEqual(rules.evaluate(gap.witness).fired, [])
    assert.equal(String(gap.witness.price), '0.5')
    assert.ok(gap.witness.day instanceof LocalDate)
    const written = report.toJSON().audit.diagnostics[1].witness
    assert.deepEqual(written.price, { $decimal: '0.5' })
    assert.deepEqual(written.day, { $date: '0001-01-01' })
  })

  test('no findings', () => {
    const report = new RuleSet([new Rule('minor', F.age.lt(18)), new Rule('adult', F.age.ge(18))], { hitPolicy: 'unique' }).audit()
    assert.deepEqual(report.diagnostics, [])
    assert.equal(report.maxSeverity, null)
    assert.match(String(report), /\nNo findings\.\n/)
    assert.equal(report[INSPECT](), 'Audit(rule set: 0 diagnostics)')
    assert.equal(report.atLeast('info'), false)
  })

  test('mixed types give the field and its groups', () => {
    const report = new RuleSet([new Rule('a', F.code.eq('x')), new Rule('b', F.code.gt(3))], { hitPolicy: 'rule_order' }).audit()
    const mixed = report.diagnostics.find((d) => d.code === 'mixed_types')
    assert.deepEqual(mixed.path, ['code'])
    assert.deepEqual(mixed.groups, ['number', 'string'])
    assert.equal(mixed.witness, null)
    assert.equal(mixed.pointer, null)
  })

  test('what was not decided', () => {
    const report = new RuleSet([new Rule('a', F.name.matches('^x').and(F.y.gt(0.5))), new Rule('b', F.y.lt(2))], {
      hitPolicy: 'first',
    }).audit()
    const [test] = report.notModelled
    assert.ok(test instanceof NotModelled)
    assert.deepEqual([test.rule, test.op, test.pointer, test.limit], ['a', 'matches', '/ruleset/rules/0/when/operands/0', null])
    assert.equal(report.status('unsatisfiable'), 'partial')
    assert.ok(report.undecided.every((u) => u instanceof Undecided))
    assert.deepEqual(
      report.undecided.map((u) => [u.check, [...u.rules], u.reason, u.limit, u.remaining]),
      [
        ['unsatisfiable', ['a'], 'not_modelled', null, null],
        ['overlap', ['a', 'b'], 'not_modelled', null, null],
      ],
    )
  })

  test('limits and the rule set digest', () => {
    const rules = new RuleSet([new Rule('a', F.y.gt(1)), new Rule('b', F.y.lt(2))], { hitPolicy: 'first' })
    const report = rules.audit({ limits: { maxSteps: 0 }, rulesetDigest: true })
    assert.deepEqual(report.limits, { maxTerms: 256, maxSteps: 0 })
    const [first] = report.undecided
    assert.deepEqual([first.reason, first.limit, first.remaining, first.rules.length], ['limit', 'max_steps', 2, 0])
    assert.equal(report.rulesetDigest, digest(rules))
    assert.equal(report.toJSON().audit.ruleset_digest, digest(rules))
    const many = new RuleSet([new Rule('a', F.x.eq(1).or(F.x.eq(3)).or(F.x.eq(5)))]).audit({ limits: { maxTerms: 1 } })
    assert.ok(many.notModelled.some((n) => n.limit === 'max_terms'))
    assert.equal(rules.audit(null).limits.maxSteps, 1000000)
    // The limits a report gives back are options again.
    assert.deepEqual(rules.audit({ limits: report.limits }).limits, report.limits)
    // A first, at least warning: an overlap (info) and a gap (warning).
    const underFirst = ages('first').audit()
    assert.equal(underFirst.maxSeverity, 'warning')
    assert.deepEqual(['info', 'warning', 'error'].map((s) => underFirst.atLeast(s)), [true, true, false])
  })

  test('bad options', () => {
    const rules = ages()
    assert.throws(() => rules.audit({ limits: { maxTerms: -1 } }), (e) => e instanceof RangeError && /limits\.maxTerms is -1/.test(e.message))
    assert.throws(() => rules.audit({ limits: { maxSteps: 1.5 } }), RangeError)
    assert.throws(() => rules.audit({ limits: { maxSteps: '10' } }), (e) => e instanceof TypeError && /limits\.maxSteps is a number/.test(e.message))
    assert.throws(() => rules.audit({ limits: 5 }), (e) => e instanceof TypeError && /limits must be an object/.test(e.message))
    assert.throws(() => rules.audit({ maxSteps: 10 }), /unknown option "maxSteps"; the options are limits, rulesetDigest/)
    assert.throws(() => rules.audit({ rulesetDigest: 1 }), (e) => e instanceof TypeError && /options\.rulesetDigest is a boolean/.test(e.message))
    assert.throws(() => rules.audit({ format: 'json' }), /unknown option "format"/)
    assert.throws(() => rules.audit(5), /options must be an object/)
  })

  test('graded logics: a rule set takes Kleene logic only, so none reaches the audit', () => {
    assert.throws(() => new RuleSet([new Rule('a', F.x.gt(1))], { logic: 'min' }), UnsupportedError)
  })

  test('only the engine makes reports', () => {
    for (const Class of [Audit, Diagnostic, AuditedRule, CheckResult, NotModelled, Undecided]) {
      assert.throws(() => new Class(), TypeError)
    }
  })
})

describe('DecisionTable.audit', () => {
  test('cells, rows and unlisted fields', () => {
    const table = new DecisionTable(
      ['RULE', 'country', 'THEN rate'],
      [
        ['es', 'ES', decimal('0.21')],
        ['pt', "'PT'", decimal('0.23')],
      ],
      { hitPolicy: 'first' },
    )
    const report = table.audit({ rulesetDigest: true })
    assert.equal(report.source, 'decision_table')
    assert.deepEqual(
      report.rules.map((r) => [r.name, r.row]),
      [
        ['es', 0],
        ['pt', 1],
      ],
    )
    const [unlisted] = report.diagnostics
    assert.equal(unlisted.code, 'unlisted_field')
    assert.equal(unlisted.pointer, '/decision_table/rows/0/inputs/0')
    assert.deepEqual(unlisted.path, ['ES'])
    assert.match(unlisted.message, /'ES'/)
    assert.equal(report.status('fields'), 'complete')
    assert.equal(report.rulesetDigest, digest(table.ruleset))
    const gap = report.diagnostics.find((d) => d.code === 'gap')
    assert.deepEqual(table.evaluate(gap.witness).fired, [])
    assert.equal(report[INSPECT](), 'Audit(decision table: 2 diagnostics, max severity warning)')
    assert.throws(() => table.audit({ limits: { maxSteps: -2 } }), RangeError)
  })
})

describe('the native layer', () => {
  const text = JSON.stringify({ version: 1, ruleset: ages().toJSON().ruleset })

  test('RuleSet.audit and Document.audit write text, JSON or YAML', () => {
    const rules = native.RuleSet.read(text)
    const document = native.Document.read(text)
    assert.equal(rules.audit(), document.audit())
    assert.match(rules.audit({ format: 'text' }), /^Audit of a rule set: 2 rules/)
    const json = JSON.parse(document.audit({ format: 'json', limits: { maxSteps: 5000 }, rulesetDigest: true }))
    assert.equal(json.audit.limits.max_steps, 5000)
    assert.equal(json.audit.ruleset_digest, rules.digest())
    assert.match(document.audit({ format: 'yaml' }), /^version: 1\naudit:\n/)
    assert.throws(() => document.audit({ format: 'csv' }), UnsupportedError)
    assert.throws(() => document.audit({ format: 'xml' }), RangeError)
    assert.throws(() => document.audit([]), (e) => e instanceof TypeError && /such as \{ limits: \{ maxSteps: 10000 \} \}, not an array/.test(e.message))
    assert.throws(() => document.audit({ limits: [] }), (e) => e instanceof TypeError && /limits must be an object/.test(e.message))
  })

  test('Document.audit audits what the core audits: tables by cell, rules, bundles; not conditions', () => {
    const table = native.Document.read('RULE,x,THEN y\nr,> 1,1\nr2,X,2\n', { format: 'csv' })
    const tableText = table.audit()
    assert.match(tableText, /^Audit of a decision table: 2 rules, hit policy first/)
    assert.match(tableText, /at row r2, input x \(\/decision_table\/rows\/1\/inputs\/0\)/)
    const rule = native.Document.read('RULE odd IF x > TRUE', { format: 'text' })
    const ruleJson = JSON.parse(rule.audit({ format: 'json' }))
    assert.equal(ruleJson.audit.document, 'rule')
    assert.equal(ruleJson.audit.diagnostics[0].pointer, '/rule/when')
    const bundle = native.Document.makeBundle(table, 'acme/t', '1')
    const bundleJson = JSON.parse(bundle.audit({ format: 'json' }))
    assert.equal(bundleJson.audit.document, 'bundle')
    assert.equal(bundleJson.audit.source, 'decision_table')
    assert.ok(bundleJson.audit.diagnostics.some((d) => d.pointer?.startsWith('/bundle/decision_table/rows/')))
    const condition = native.Document.read('{"version":1,"condition":{"op":"field","path":["x"]}}')
    assert.throws(() => condition.audit(), UnsupportedError)
  })

  test("a diagnostic's text names its cell, as the audit's text does", () => {
    const table = new DecisionTable(['RULE', 'country', 'THEN rate'], [['es', 'ES', 1]])
    const report = table.audit()
    const unlisted = report.diagnostics.find((d) => d.code === 'unlisted_field')
    assert.match(String(unlisted), /\n {11}at row es, input country \(\/decision_table\/rows\/0\/inputs\/0\)\n/)
    assert.ok(String(report).includes(String(unlisted)))
  })
})
