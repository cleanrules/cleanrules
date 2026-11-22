// Bundles (task R.5, spec/bundles.md): making a bundle of a rule set or a
// decision table, reading and writing it in JSON and YAML, its getters, its
// case files, the five relations of a comparison, and the errors; and a
// decision written with its rule set digest (`dumps(decision, {
// rulesetDigest })`, which closes R.2's deviation).

import assert from 'node:assert/strict'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { describe, test } from 'node:test'

import { native, root } from './support/cr.js'

const {
  Bundle,
  CaseFileRef,
  DecisionTable,
  F,
  FormatError,
  Rule,
  RuleSet,
  UnsupportedError,
  canonicalBytes,
  decimal,
  digest,
  dump,
  dumps,
  load,
  loads,
  version,
} = root

const vip = (threshold = 5) =>
  new RuleSet([
    new Rule('vip', F.customer.years.gt(threshold), { discount: decimal('0.15') }),
    new Rule('loyal', F.customer.years.gt(2), { discount: decimal('0.05'), label: 'loyal' }),
  ])

const table = () =>
  new DecisionTable(
    ['RULE', 'customer.years', 'THEN discount', 'THEN label'],
    [
      ['vip', '> 5', decimal('0.15'), 'VIP'],
      ['loyal', '> 2', decimal('0.05'), 'loyal'],
    ],
  )

const make = (rules = vip(), options = {}) => new Bundle(rules, { id: 'acme/discounts', revision: '1', ...options })

/** A bundle document's JSON with `change` applied to its `bundle` content. */
function edited(bundle, change) {
  const document = JSON.parse(dumps(bundle))
  change(document.bundle)
  return loads(JSON.stringify(document))
}

describe('making a bundle', () => {
  test('of a rule set: its identifier, revision, digest, engine, contract and cases', () => {
    const rules = vip()
    const cases = [
      { path: 'cases/vip.json', content: '[{"customer": {"years": 9}}]' },
      { path: 'cases/empty.json', content: new Uint8Array() },
    ]
    const bundle = make(rules, { revision: '2026-11-16.1', cases })
    assert.equal(bundle.id, 'acme/discounts')
    assert.equal(bundle.revision, '2026-11-16.1')
    assert.equal(bundle.rulesetDigest, digest(rules))
    assert.ok(bundle.rules instanceof RuleSet)
    assert.equal(bundle.ruleset, bundle.rules)
    assert.deepEqual(bundle.ruleset.rules.map((rule) => rule.name), ['vip', 'loyal'])
    assert.deepEqual(bundle.engine, {
      name: 'cleanrules',
      version,
      limits: {
        maxSize: 16777216,
        maxNesting: 1024,
        maxDepth: 256,
        maxPathSegments: 256,
        maxTableSize: 2097152,
        maxValues: 1000000,
      },
      isCurrent: true,
    })
    assert.deepEqual(bundle.contract.output, { keys: ['discount', 'label'] })
    assert.equal(bundle.contract.input.type, 'object')
    assert.deepEqual(Object.keys(bundle.contract.input.properties), ['customer'])
    assert.deepEqual(
      bundle.cases.map((c) => c.path),
      ['cases/vip.json', 'cases/empty.json'],
    )
    // The SHA-256 of no bytes.
    assert.equal(bundle.cases[1].digest, 'sha256:e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855')
  })

  test('of a decision table: it holds the table, and gives the rule set it compiles to', () => {
    const held = table()
    const bundle = make(held)
    assert.ok(bundle.rules instanceof DecisionTable)
    assert.ok(bundle.ruleset instanceof RuleSet)
    assert.equal(bundle.rulesetDigest, digest(held.ruleset))
    assert.equal(dumps(bundle.rules), dumps(held))
    assert.ok('decision_table' in JSON.parse(dumps(bundle)).bundle)
    assert.equal(make(held.ruleset).rulesetDigest, bundle.rulesetDigest)
  })

  test('is reproducible: the same arguments give the same bytes and digest', () => {
    const cases = [{ path: 'cases/a.json', content: '[]' }]
    const a = make(vip(), { cases })
    const b = make(vip(), { cases: new Set(cases) })
    assert.deepEqual(canonicalBytes(a), canonicalBytes(b))
    assert.equal(digest(a), digest(b))
    assert.notEqual(digest(a), a.rulesetDigest)
    assert.match(digest(a), /^sha256:[0-9a-f]{64}$/)
  })

  test('records the limits it is given, the others at their defaults', () => {
    const bundle = make(vip(), { limits: { maxDepth: 64, maxValues: 10, maxSize: null } })
    assert.equal(bundle.engine.limits.maxDepth, 64)
    assert.equal(bundle.engine.limits.maxValues, 10)
    assert.equal(bundle.engine.limits.maxSize, 16777216)
    assert.deepEqual(make(vip(), { limits: bundle.engine.limits }).engine, bundle.engine)
  })

  test('getters are frozen and the same each time', () => {
    const bundle = make(vip(), { cases: [{ path: 'a.json', content: 'x' }] })
    for (const value of [bundle, bundle.engine, bundle.engine.limits, bundle.contract, bundle.contract.input, bundle.contract.output.keys, bundle.cases, bundle.cases[0]]) {
      assert.ok(Object.isFrozen(value))
    }
    assert.equal(bundle.engine, bundle.engine)
    assert.equal(bundle.contract, bundle.contract)
    assert.equal(bundle.cases, bundle.cases)
    assert.equal(bundle.rules, bundle.rules)
  })

  test('inspects as its identifier, revision, rules and engine', () => {
    assert.equal(
      make(vip(), { cases: [{ path: 'a.json', content: '' }] })[Symbol.for('nodejs.util.inspect.custom')](),
      `Bundle(acme/discounts 1: rule set, 1 case file, cleanrules ${version})`,
    )
    assert.equal(make(table())[Symbol.for('Deno.customInspect')](), `Bundle(acme/discounts 1: decision table, 0 case files, cleanrules ${version})`)
  })
})

describe('actions', () => {
  const notify = () => {}
  const coded = new RuleSet([new Rule('vip', F.x.gt(1), {}, { do: notify })])

  test('an action given as code is refused, as dumps refuses it', () => {
    assert.throws(
      () => make(coded),
      (error) => error instanceof UnsupportedError && error.context.pointer === '/bundle/ruleset/rules/0/do',
    )
  })

  test('omitActions makes the bundle of the rules without it', () => {
    const bundle = make(coded, { omitActions: true })
    assert.equal(bundle.rulesetDigest, digest(coded, { omitActions: true }))
    assert.equal(bundle.ruleset.rules[0].do, null)
  })

  test('a named action stays a name, unbound', () => {
    const named = new RuleSet([new Rule('vip', F.x.gt(1), {}, { do: 'notify' })]).bind({ notify })
    const bundle = make(named)
    assert.equal(bundle.ruleset.rules[0].actionName, 'notify')
    assert.equal(bundle.ruleset.rules[0].do, null)
    assert.equal(bundle.ruleset.bind({ notify }).rules[0].do, notify)
  })
})

describe('reading and writing', () => {
  test('dumps and loads: JSON, YAML and JSON.stringify give the same bundle', () => {
    const bundle = make(vip(), { cases: [{ path: 'cases/a.json', content: '[]' }] })
    for (const text of [dumps(bundle), JSON.stringify(bundle)]) {
      const again = loads(text)
      assert.ok(again instanceof Bundle)
      assert.equal(dumps(again), dumps(bundle))
    }
    const yaml = dumps(bundle, { format: 'yaml' })
    assert.match(yaml, /^version: 1\nbundle:\n {2}id: \"acme\/discounts\"\n/)
    const fromYaml = loads(yaml, { format: 'yaml' })
    assert.equal(digest(fromYaml), digest(bundle))
    assert.deepEqual(fromYaml.compare(bundle), { relation: 'same_rules_same_engine', identical: true, differences: [] })
  })

  test('the rule syntax and CSV cannot hold a bundle', () => {
    for (const format of ['text', 'csv']) {
      assert.throws(() => dumps(make(), { format }), UnsupportedError)
    }
  })

  test('load and dump a bundle file', async () => {
    const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'cleanrules-'))
    try {
      const bundle = make(table())
      for (const name of ['discounts.bundle.json', 'discounts.bundle.yaml']) {
        const file = path.join(dir, name)
        await dump(bundle, file)
        const again = await load(file)
        assert.ok(again instanceof Bundle)
        assert.equal(digest(again), digest(bundle))
      }
    } finally {
      fs.rmSync(dir, { recursive: true, force: true })
    }
  })

  test('a bundle of another engine is read as it is, and named as such', () => {
    const other = edited(make(), (bundle) => {
      bundle.engine.version = '1.0.0-other.1'
      bundle.contract.input = { type: 'object' }
    })
    assert.equal(other.engine.version, '1.0.0-other.1')
    assert.equal(other.engine.isCurrent, false)
    assert.deepEqual(other.contract.input, { type: 'object' })
  })

  test('a bundle of this engine with an edited contract is refused', () => {
    assert.throws(
      () =>
        edited(make(), (bundle) => {
          bundle.contract.output.keys = ['discount']
        }),
      (error) => error instanceof FormatError && error.code === 'decision_mismatch' && error.context.pointer === '/bundle/contract/output',
    )
  })

  test('a rule set digest that is not that of the rules is refused', () => {
    assert.throws(
      () =>
        edited(make(), (bundle) => {
          bundle.ruleset_digest = digest(vip(6))
        }),
      (error) => error.code === 'decision_mismatch' && error.context.pointer === '/bundle/ruleset_digest',
    )
  })

  test('RuleSet.fromDocument of a bundle document gives its rule set', () => {
    const bundle = make(table())
    const document = native.Document.read(dumps(bundle))
    assert.equal(document.kind, 'bundle')
    assert.equal(native.RuleSet.fromDocument(document).digest(), bundle.rulesetDigest)
  })
})

describe('case files', () => {
  const bundle = make(vip(), {
    cases: [
      { path: 'cases/a.json', content: 'héllo' },
      { path: 'cases/b.bin', content: new Uint8Array([0, 255]) },
    ],
  })

  test('caseDigestMatches checks the bytes against the recorded digest', () => {
    assert.equal(bundle.caseDigestMatches('cases/a.json', 'héllo'), true)
    assert.equal(bundle.caseDigestMatches('cases/a.json', new TextEncoder().encode('héllo')), true)
    assert.equal(bundle.caseDigestMatches('cases/a.json', 'hello'), false)
    assert.equal(bundle.caseDigestMatches('cases/b.bin', new Uint8Array([0, 255])), true)
    assert.equal(bundle.caseDigestMatches('cases/b.bin', new Uint8Array([0])), false)
  })

  test('a path the bundle does not refer to is a RangeError naming its paths', () => {
    assert.throws(() => bundle.caseDigestMatches('cases/c.json', ''), {
      name: 'RangeError',
      message: 'the bundle refers to no case file "cases/c.json"; its case files are "cases/a.json", "cases/b.bin"',
    })
    assert.throws(() => make().caseDigestMatches('a.json', ''), { name: 'RangeError', message: /it has none/ })
  })

  test('arguments of the wrong type are TypeErrors', () => {
    assert.throws(() => bundle.caseDigestMatches(1, ''), TypeError)
    assert.throws(() => bundle.caseDigestMatches('cases/a.json', 5), TypeError)
  })
})

describe('comparing', () => {
  const base = make(vip())
  const relation = (a, b) => {
    const forth = a.compare(b)
    assert.deepEqual(b.compare(a), forth, 'symmetric')
    return forth
  }

  test('the same rules on the same engine: identical, or with other cases', () => {
    assert.deepEqual(relation(base, make(vip())), { relation: 'same_rules_same_engine', identical: true, differences: [] })
    const withCases = make(vip(), { cases: [{ path: 'a.json', content: '' }] })
    assert.deepEqual(relation(base, withCases), { relation: 'same_rules_same_engine', identical: false, differences: ['cases'] })
    assert.deepEqual(relation(make(table()), make(table().ruleset)).differences, ['source'])
  })

  test('the same rules on another engine or configuration', () => {
    assert.deepEqual(relation(base, make(vip(), { limits: { maxValues: 10 } })), {
      relation: 'same_rules_other_engine',
      identical: false,
      differences: ['engine.limits.max_values'],
    })
    const older = edited(base, (bundle) => {
      bundle.engine.version = '0.0.1-old'
    })
    assert.deepEqual(relation(base, older).differences, ['engine.version'])
  })

  test('other rules on the same engine, and on another one', () => {
    const other = make(vip(6), { revision: '2' })
    assert.deepEqual(relation(base, other), {
      relation: 'other_rules_same_engine',
      identical: false,
      differences: ['revision', 'rules'],
    })
    assert.equal(relation(base, make(vip(6), { revision: '2', limits: { maxDepth: 64 } })).relation, 'other_rules_other_engine')
  })

  test('one revision naming two sets of rules is a conflict', () => {
    assert.deepEqual(relation(base, make(vip(6))), { relation: 'conflict', identical: false, differences: ['rules'] })
    assert.equal(relation(base, make(vip(6), { limits: { maxDepth: 64 } })).relation, 'conflict')
  })

  test('the comparison is frozen, and takes only a Bundle', () => {
    const comparison = base.compare(base)
    assert.ok(Object.isFrozen(comparison) && Object.isFrozen(comparison.differences))
    assert.throws(() => base.compare(vip()), { name: 'TypeError', message: /compare takes a Bundle, not a RuleSet/ })
  })

  test('the native layer compares two bundle documents, and refuses others', () => {
    const a = native.Document.read(dumps(base))
    const b = native.Document.read(dumps(make(vip(), { limits: { maxDepth: 64 } })))
    assert.deepEqual(JSON.parse(a.compareBundle(b)), {
      relation: 'same_rules_other_engine',
      identical: false,
      differences: ['engine.limits.max_depth'],
    })
    const rules = native.Document.read(dumps(vip()))
    assert.throws(() => a.compareBundle(rules), { name: 'TypeError', message: 'other must be a bundle document, not a ruleset document' })
    assert.throws(() => rules.compareBundle(a), { name: 'TypeError', message: 'this document must be a bundle document, not a ruleset document' })
    assert.throws(() => a.compareBundle({}), TypeError)
  })
})

describe('errors', () => {
  test('an identifier, revision or case path without its form is a FormatError', () => {
    for (const options of [
      { id: 'Acme' },
      { id: 'acme//x' },
      { revision: '' },
      { revision: 'a b' },
      { cases: [{ path: '../x.json', content: '' }] },
      { cases: [{ path: '/x.json', content: '' }] },
      {
        cases: [
          { path: 'a.json', content: '' },
          { path: 'a.json', content: 'x' },
        ],
      },
    ]) {
      assert.throws(() => make(vip(), options), (error) => error instanceof FormatError && error.code === 'format_error', JSON.stringify(options))
    }
  })

  test('arguments of the wrong type are TypeErrors', () => {
    const cases = [
      [vip(), undefined, /id must be a string, such as "acme\/discounts", not undefined/],
      [vip(), { id: 'a' }, /revision must be a string/],
      [vip(), { id: 'a', revision: 1 }, /revision must be a string, such as "2026-11-16.1", not a number/],
      [vip(), { id: 'a', revision: '1', cases: 'a.json' }, /cases must be an array of \{ path, content \}/],
      [vip(), { id: 'a', revision: '1', cases: ['a.json'] }, /cases\[0\] must be an object \{ path, content \}, not a string/],
      [vip(), { id: 'a', revision: '1', cases: [{ path: 1, content: '' }] }, /cases\[0\]\.path must be a string/],
      [vip(), { id: 'a', revision: '1', cases: [{ path: 'a.json' }] }, /cases\[0\]\.content must be a string or bytes/],
      [vip(), { id: 'a', revision: '1', omitActions: 'yes' }, /omitActions is a boolean/],
      [vip(), { id: 'a', revision: '1', limits: { maxYamlSize: 10 } }, /does not record limits\.maxYamlSize/],
      [vip(), { id: 'a', revision: '1', limits: 5 }, /limits must be an object/],
      [vip(), { id: 'a', revision: '1', version: '1' }, /unknown option "version"/],
      [new Rule('vip', F.x.gt(1)), { id: 'a', revision: '1' }, /a bundle holds a RuleSet or a DecisionTable, not a Rule; give a single rule as a rule set/],
    ]
    for (const [rules, options, message] of cases) {
      assert.throws(() => new Bundle(rules, options), { name: 'TypeError', message }, String(message))
    }
  })

  test('the native layer makes bundles of rule set and table documents only', () => {
    const rules = native.Document.read(dumps(vip()))
    const made = native.Document.makeBundle(rules, 'acme/a', '1', [{ path: 'a.json', content: 'x' }], { maxDepth: 64 })
    assert.equal(made.kind, 'bundle')
    assert.equal(loads(made.write()).engine.limits.maxDepth, 64)
    const condition = native.Document.read('{"version": 1, "condition": {"op": "const", "value": true}}')
    assert.throws(() => native.Document.makeBundle(condition, 'a', '1'), {
      name: 'TypeError',
      message: /a bundle holds a rule set or a decision table, not a condition document/,
    })
    assert.throws(() => native.Document.makeBundle({}, 'a', '1'), TypeError)
  })
})

describe('decisions with their rule set digest', () => {
  const rules = vip()
  const decision = rules.evaluate({ customer: { years: 9 } })

  test('dumps writes it on request, and readDecision verifies and keeps it', () => {
    assert.equal(JSON.parse(dumps(decision)).decision.ruleset_digest, undefined)
    assert.equal(decision.toJSON().decision.ruleset_digest, undefined)
    const logged = dumps(decision, { rulesetDigest: true })
    assert.equal(JSON.parse(logged).decision.ruleset_digest, digest(rules))
    assert.equal(JSON.parse(logged).decision.ruleset_digest, make(rules).rulesetDigest)
    const read = rules.readDecision(logged)
    assert.deepEqual(read.selected.map((r) => r.rule.name), ['vip'])
    // Read with one, it keeps it either way.
    assert.equal(JSON.parse(dumps(read)).decision.ruleset_digest, digest(rules))
    assert.equal(JSON.parse(dumps(read, { rulesetDigest: false })).decision.ruleset_digest, digest(rules))
    assert.equal(read.toJSON().decision.ruleset_digest, digest(rules))
    assert.throws(() => vip(6).readDecision(logged), (error) => error.code === 'decision_mismatch')
  })

  test('in YAML and through dump', async () => {
    const yaml = dumps(decision, { format: 'yaml', rulesetDigest: true })
    assert.match(yaml, /ruleset_digest: \"sha256:[0-9a-f]{64}\"/)
    assert.equal(dumps(rules.readDecision(yaml, { format: 'yaml' })), dumps(decision, { rulesetDigest: true }))
    const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'cleanrules-'))
    try {
      const file = path.join(dir, 'decision.json')
      await dump(decision, file, { rulesetDigest: true })
      assert.equal(fs.readFileSync(file, 'utf8'), dumps(decision, { rulesetDigest: true }))
    } finally {
      fs.rmSync(dir, { recursive: true, force: true })
    }
  })

  test('the native layer writes it too', () => {
    const layer = native.RuleSet.read(dumps(rules)).evaluate({ customer: { years: 9 } })
    assert.equal(JSON.parse(layer.write('json', { rulesetDigest: true })).decision.ruleset_digest, digest(rules))
    assert.equal(JSON.parse(layer.write()).decision.ruleset_digest, undefined)
    assert.equal(JSON.parse(layer.write('json', null)).decision.ruleset_digest, undefined)
    assert.equal(JSON.parse(layer.write('json', {})).decision.ruleset_digest, undefined)
    assert.throws(() => layer.write('json', { rulesetDigest: 'yes' }), {
      name: 'TypeError',
      message: 'options.rulesetDigest is a boolean, not a string',
    })
    assert.throws(() => layer.write('json', true), {
      name: 'TypeError',
      message: 'options must be an object, such as { rulesetDigest: true }, not a boolean',
    })
  })

  test('rulesetDigest is an option of decisions only; decisions have no canonical bytes here', () => {
    assert.throws(() => dumps(rules, { rulesetDigest: true }), { name: 'TypeError', message: /rulesetDigest is an option of decisions/ })
    assert.throws(() => dumps(decision, { rulesetDigest: 1 }), TypeError)
    assert.throws(() => digest(decision), TypeError)
    assert.throws(() => dumps(decision, { format: 'csv' }), UnsupportedError)
  })
})

describe('review fixes', () => {
  test('a limit of 0 is not recorded', () => {
    assert.throws(() => make(vip(), { limits: { maxDepth: 0 } }), UnsupportedError)
  })

  test('a limit above 2^53 - 1 is not recorded either; one that is not a whole number from 0 is a RangeError', () => {
    const max = Number.MAX_SAFE_INTEGER
    assert.equal(make(vip(), { limits: { maxValues: max } }).engine.limits.maxValues, max)
    for (const value of [max + 1, 2 ** 64, 2 ** 80]) {
      assert.throws(() => make(vip(), { limits: { maxValues: value } }), (error) => error instanceof UnsupportedError && error.code === 'unsupported')
    }
    for (const value of [-1, 1.5, Number.NaN, Number.POSITIVE_INFINITY]) {
      assert.throws(() => make(vip(), { limits: { maxDepth: value } }), { name: 'RangeError', message: /limits\.maxDepth is .*; a limit is a whole number from 0/ })
    }
    assert.throws(() => make(vip(), { limits: { maxDepth: '64' } }), { name: 'TypeError', message: 'limits.maxDepth is a number, not a string' })
    assert.throws(() => make(vip(), { limits: { maxTextSize: 10 } }), { name: 'TypeError', message: /does not record limits\.maxTextSize/ })
  })

  test('the case files of a bundle are CaseFileRef objects, which check what they are made of', () => {
    const bundle = make(vip(), { cases: [{ path: 'cases/a.json', content: '[]' }] })
    const [first] = bundle.cases
    assert.ok(first instanceof CaseFileRef)
    assert.equal(first.path, 'cases/a.json')
    assert.equal(first.digestMatches('[]'), true)
    assert.equal(first.digestMatches(new TextEncoder().encode('[]')), true)
    assert.equal(first.digestMatches('[ ]'), false)
    const again = new CaseFileRef(first.path, first.digest)
    assert.deepEqual(again, first)
    assert.ok(Object.isFrozen(again))
    assert.deepEqual(JSON.parse(JSON.stringify(again)), { path: 'cases/a.json', digest: first.digest })
    assert.equal(again[Symbol.for('nodejs.util.inspect.custom')](), `CaseFileRef(cases/a.json ${first.digest})`)
    for (const [path, digest] of [
      ['Cases/A.json', first.digest],
      ['../a.json', first.digest],
      ['cases/a.json', 'sha256:00'],
      ['cases/a.json', first.digest.toUpperCase()],
    ]) {
      assert.throws(() => new CaseFileRef(path, digest), FormatError, `${path} ${digest}`)
    }
    assert.throws(() => new CaseFileRef(1, first.digest), { name: 'TypeError', message: /path must be a string/ })
    assert.throws(() => new CaseFileRef('a.json'), { name: 'TypeError', message: /digest must be a string/ })
    assert.throws(() => first.digestMatches(5), TypeError)
  })

  test('the comparison is symmetric', () => {
    const a = make(vip(), { limits: { maxDepth: 64 } })
    const b = make(vip(6), { revision: '2', cases: [{ path: 'a.json', content: '' }] })
    assert.deepEqual(a.compare(b), b.compare(a))
  })

  test('case paths are lowercase and do not end with a dot', () => {
    for (const path of ['cases/A.json', 'cases/a.']) {
      assert.throws(() => make(vip(), { cases: [{ path, content: '' }] }), FormatError)
    }
  })

  test('loads does not bind a bundle to actions', () => {
    assert.throws(() => loads(dumps(make()), { actions: {} }), /bundle\.ruleset\.bind/)
  })
})
