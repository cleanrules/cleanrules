// The error classes (spec/errors.md): the hierarchy, one real error of every
// code with its context fields, and the JSON form.

import assert from 'node:assert/strict'
import { describe, test } from 'node:test'

import cr, { build } from './support/cr.js'
import { rules } from './support/probe.js'

const HIERARCHY = [
  ['FormatError', 'CleanRulesError', 'format_error'],
  ['ExpressionError', 'CleanRulesError', 'expression_error'],
  ['RuleDefinitionError', 'CleanRulesError', 'rule_definition_error'],
  ['RuleSyntaxError', 'CleanRulesError', 'rule_syntax_error'],
  ['RegistryError', 'CleanRulesError', 'registry_error'],
  ['RuleNotFoundError', 'CleanRulesError', 'rule_not_found'],
  ['EvaluationError', 'CleanRulesError', 'evaluation_error'],
  ['MissingFieldError', 'EvaluationError', 'missing_field'],
  ['TypeMismatchError', 'EvaluationError', 'type_mismatch'],
  ['HitPolicyError', 'EvaluationError', 'hit_policy_error'],
  ['ActionError', 'CleanRulesError', 'action_error'],
  ['UnsupportedError', 'CleanRulesError', 'unsupported'],
]

/** The error `run` throws. */
function caught(run) {
  try {
    run()
  } catch (error) {
    return error
  }
  assert.fail('nothing was thrown')
}

const doc = (content) => JSON.stringify({ version: 1, ...content })

describe('the hierarchy', () => {
  for (const [name, parent, code] of HIERARCHY) {
    test(`${name} is a ${parent} with code ${code}`, () => {
      const Class = cr[name]
      const error = new Class('went wrong')
      assert.ok(error instanceof cr[parent])
      assert.ok(error instanceof cr.CleanRulesError)
      assert.ok(error instanceof Error)
      assert.equal(error.name, name)
      assert.equal(error.code, code)
      assert.equal(Class.defaultCode, code)
      assert.equal(error.message, 'went wrong')
      assert.equal(error.hint, null)
      assert.equal(error.decision, null)
      assert.deepEqual(error.context, { rules: [], types: [], available: [], executed: [] })
      assert.match(String(error.stack), new RegExp(`^${name}: went wrong`))
    })
  }

  test('the base class has no code of its own', () => {
    const error = new cr.CleanRulesError('m', { code: 'anything' })
    assert.equal(error.code, 'anything')
    assert.equal(error.name, 'CleanRulesError')
    assert.equal(new cr.CleanRulesError('m').code, '')
  })

  test('errors take a hint, context fields and a cause', () => {
    const cause = new Error('root')
    const error = new cr.MissingFieldError('missing field x', {
      hint: 'add it',
      context: { rule: 'r', path: ['x', 0] },
      cause,
    })
    assert.equal(error.hint, 'add it')
    assert.equal(error.context.rule, 'r')
    assert.deepEqual(error.context.path, ['x', 0])
    assert.equal(error.cause, cause)
    assert.ok(Object.isFrozen(error.context))
    assert.deepEqual(error.toJSON(), {
      code: 'missing_field',
      message: 'missing field x',
      hint: 'add it',
      rule: 'r',
      path: ['x', 0],
      cause: 'root',
    })
  })

  test('unknown context fields are refused', () => {
    assert.throws(() => new cr.FormatError('m', { context: { lines: 1 } }), TypeError)
  })
})

describe('every code, from the core', () => {
  test('format_error, with a JSON Pointer', () => {
    const error = caught(() => cr.Document.read(doc({ condition: { op: 'nope' } })))
    assert.ok(error instanceof cr.FormatError)
    assert.equal(error.code, 'format_error')
    assert.equal(error.context.pointer, '/condition/op')
    assert.equal(typeof error.message, 'string')
  })

  test('format_error, with the line and column of bytes that are not UTF-8', () => {
    const bytes = new Uint8Array([...Buffer.from('{\n  "a": "'), 0xff, ...Buffer.from('"}')])
    const error = caught(() => cr.Document.read(bytes))
    assert.equal(error.code, 'format_error')
    assert.equal(error.context.line, 2)
    assert.equal(error.context.column, 9)
  })

  test('unsupported_version is a FormatError', () => {
    const error = caught(() => cr.Document.read('{"version": 2, "condition": {"op": "const", "value": true}}'))
    assert.ok(error instanceof cr.FormatError)
    assert.equal(error.code, 'unsupported_version')
    assert.equal(error.context.pointer, '/version')
  })

  test('limit_exceeded is a FormatError', () => {
    const error = caught(() => cr.Document.read(doc({ condition: { op: 'const', value: true } }), { format: 'json', limits: { maxSize: 10 } }))
    assert.ok(error instanceof cr.FormatError)
    assert.equal(error.code, 'limit_exceeded')
  })

  test('decision_mismatch is a FormatError', () => {
    const a = rules('RULE a\nIF x > 1')
    const b = rules('RULE b\nIF x > 1')
    const error = caught(() => b.readDecision(a.evaluate({ x: 2 }).write()))
    assert.ok(error instanceof cr.FormatError)
    assert.equal(error.code, 'decision_mismatch')
    assert.equal(typeof error.context.pointer, 'string')
  })

  test('expression_error, at the line and column of the node', () => {
    const error = caught(() => cr.Node.read("x IN (1, 'a')", { format: 'text' }))
    assert.ok(error instanceof cr.ExpressionError)
    assert.equal(error.context.line, 1)
    assert.equal(error.context.column, 1)
  })

  test('rule_definition_error names the rule', () => {
    const error = caught(() => rules('RULE a\nIF x > 1\n\nRULE a\nIF x > 2'))
    assert.ok(error instanceof cr.RuleDefinitionError)
    assert.equal(error.context.rule, 'a')
  })

  test('rule_syntax_error, at the line and column', () => {
    const error = caught(() => rules('RULE a\nIF x >'))
    assert.ok(error instanceof cr.RuleSyntaxError)
    assert.equal(error.context.line, 2)
    assert.equal(typeof error.context.column, 'number')
  })

  test('registry_error for an unknown logic names it and the available ones', () => {
    const error = caught(() => rules('LOGIC nope\n\nRULE a\nIF x > 1'))
    assert.ok(error instanceof cr.RegistryError)
    assert.equal(error.context.name, 'nope')
    assert.ok(error.context.available.includes('boolean'))
  })

  test('evaluation_error for a batch that is not an array', () => {
    const error = caught(() => rules('RULE a\nIF x > 1').evaluateMany({ x: 1 }))
    assert.ok(error instanceof cr.EvaluationError)
    assert.equal(error.code, 'evaluation_error')
  })

  test('missing_field names the rule, the path and the node', () => {
    const error = caught(() => rules('STRICT\n\nRULE a\nIF customer.age > 1').evaluate({}))
    assert.ok(error instanceof cr.MissingFieldError)
    assert.equal(error.context.rule, 'a')
    assert.deepEqual(error.context.path, ['customer', 'age'])
    assert.deepEqual(error.context.node, { op: 'field', path: ['customer', 'age'] })
  })

  test('type_mismatch names the types', () => {
    const error = caught(() => rules('STRICT\n\nRULE a\nIF x > 1').evaluate({ x: 'one' }))
    assert.ok(error instanceof cr.TypeMismatchError)
    assert.deepEqual(error.context.types, ['string', 'number'])
  })

  test('hit_policy_error names the rules', () => {
    const error = caught(() => rules('HIT POLICY unique\n\nRULE a\nIF x > 1\n\nRULE b\nIF x > 0').evaluate({ x: 2 }))
    assert.ok(error instanceof cr.HitPolicyError)
    assert.deepEqual(error.context.rules, ['a', 'b'])
  })

  test('hit_policy_error for the single output of a multi-hit decision', () => {
    const decision = rules('HIT POLICY collect\n\nRULE a\nIF x > 1\nTHEN v = 1').evaluate({ x: 2 })
    assert.throws(() => decision.outputJson(), cr.HitPolicyError)
  })

  test('action_error names the rule, the actions run, the cause and the decision', () => {
    const ruleset = rules('HIT POLICY rule_order\n\nRULE a\nIF TRUE\nDO first\n\nRULE b\nIF TRUE\nDO second')
    const cause = new Error('boom')
    const error = caught(() =>
      ruleset.run(
        {},
        {
          first: () => {},
          second: () => {
            throw cause
          },
        },
      ),
    )
    assert.ok(error instanceof cr.ActionError)
    assert.equal(error.context.rule, 'b')
    assert.deepEqual(error.context.executed, ['a'])
    assert.equal(error.cause, cause)
    assert.ok(error.decision instanceof cr.Decision)
    assert.equal(error.toJSON().cause, 'Error: boom')
  })

  test('unsupported names the operation and why', () => {
    const error = caught(() => cr.Node.read('x > 1', { format: 'csv' }))
    assert.ok(error instanceof cr.UnsupportedError)
    assert.equal(error.context.operation, 'csv')
    assert.equal(typeof error.context.reason, 'string')
    const decision = rules('RULE a\nIF x > 1').evaluate({ x: 2 })
    assert.throws(() => decision.write('text'), cr.UnsupportedError)
  })
})

describe('the JSON form', () => {
  test('a core error keeps its JSON form, members in order', () => {
    const error = caught(() => rules('STRICT\n\nRULE a\nIF customer.age > 1').evaluate({}))
    const form = error.toJSON()
    assert.deepEqual(Object.keys(form).slice(0, 2), ['code', 'message'])
    assert.equal(form.code, 'missing_field')
    assert.equal(form.rule, 'a')
    assert.deepEqual(form.path, ['customer', 'age'])
    assert.equal(JSON.parse(JSON.stringify(error)).code, 'missing_field')
  })
})

describe('misuse', () => {
  test('an unknown format is a RangeError', () => {
    assert.throws(() => cr.Document.read('{}', { format: 'xml' }), RangeError)
    assert.throws(() => rules('RULE a\nIF x > 1').write('xml'), RangeError)
  })

  test('a limit that is not a whole number from 0 is a RangeError', () => {
    for (const bad of [-1, 1.5, Number.NaN, Infinity, 2 ** 60]) {
      assert.throws(() => cr.Document.read('{}', { format: 'json', limits: { maxSize: bad } }), RangeError, String(bad))
    }
  })

  test('a position out of the rule set is a RangeError', () => {
    const decision = rules('RULE a\nIF x > 1').evaluate({ x: 2 })
    assert.throws(() => decision.firedAt(1), RangeError)
    assert.throws(() => decision.ruleName(5), RangeError)
  })

  test('the native classes have no public constructor', () => {
    const check = (run, message) => assert.throws(run, (e) => e instanceof TypeError && e.message === message)
    check(() => new cr.RuleSet(), 'RuleSet has no public constructor; use RuleSet.read')
    check(() => new cr.Document(), 'Document has no public constructor; use Document.read')
    check(() => new cr.Node(), 'Node has no public constructor; use Node.read')
    check(() => new cr.Decision(), 'Decision has no public constructor; decisions come from RuleSet.evaluate')
  })

  test('arguments of the wrong kind are TypeErrors that say what was given', () => {
    const ruleset = rules('RULE a\nIF x > 1\nDO act')
    const decision = ruleset.evaluate({ x: 2 })
    const cases = [
      [() => cr.RuleSet.read(42), 'the text must be a string or bytes (a Uint8Array), not a number'],
      [() => cr.Document.read('{}', 'json'), "options must be an object, such as { format: 'yaml' }, not a string"],
      [() => cr.Node.read('x', { format: 7 }), 'a format is a string such as "json", not a number'],
      [() => decision.write(7), 'a format is a string such as "json", not a number'],
      [() => decision.firedAt('0'), "a rule's position is a number, not a string"],
      [() => ruleset.evaluateJson('{}', 'big'), 'limits must be an object, such as { maxSize: 1000000 }, not a string'],
      [() => cr.Document.read('{}', { limits: { maxDepth: '9' } }), 'limits.maxDepth is a number, not a string'],
      [() => ruleset.checkActions([1]), 'action names are strings, not a number'],
      [() => ruleset.checkActions('act'), 'names must be an array, not a string'],
      [() => cr.Node.read('x', { format: 'text' }).evaluateTree({}, { strict: 1 }), 'options.strict is a boolean, not a number'],
      [() => cr.RuleSet.fromDocument(ruleset), 'document must be a Document (from Document.read), not another object'],
      [() => cr.RuleSet.fromDocument(), 'document must be a Document (from Document.read), not undefined'],
      [() => cr.Document.read(new DataView(new ArrayBuffer(1))), 'the text must be a string or bytes (a Uint8Array), not a DataView'],
      [() => cr.Document.read(new Uint16Array(1)), 'the text must be a string or bytes (a Uint8Array), not a typed array'],
    ]
    for (const [run, message] of cases) {
      assert.throws(run, (e) => e instanceof TypeError && !(e instanceof cr.CleanRulesError) && e.message === message, message)
    }
  })

  test('engine objects are released by [Symbol.dispose](), once', () => {
    const ruleset = rules('RULE a\nIF TRUE')
    ruleset[Symbol.dispose]()
    ruleset[Symbol.dispose]()
    if (build === 'wasm') {
      // The module's objects are released; using one is a clear error.
      assert.throws(() => ruleset.ruleNames(), (e) => e instanceof TypeError && /was released/.test(e.message))
      assert.throws(() => ruleset.length, (e) => e instanceof TypeError && /was released/.test(e.message))
    } else {
      // The addon's need no release: they stay usable.
      assert.deepEqual(ruleset.ruleNames(), ['a'])
    }
  })
})
