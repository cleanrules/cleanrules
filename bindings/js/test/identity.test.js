// Rule identity (task R.3, spec/rules.md, *Identity*): a rule's id, its
// effective id (the id, or the name without one), unique within a rule set,
// and ids in documents. The core checks and keeps them; the binding passes
// them through.

import assert from 'node:assert/strict'
import { describe, test } from 'node:test'

import { root } from './support/cr.js'

const { F, Rule, RuleDefinitionError, RuleSet, RuleSyntaxError, dumps, loads, ruleFromText } = root

/** Whether `call` throws `Class` with a message matching `pattern`. */
function throwsWith(call, Class, pattern, check = () => {}) {
  assert.throws(call, (error) => {
    assert.ok(error instanceof Class, `${error?.constructor?.name}: ${error?.message}`)
    assert.match(error.message, pattern)
    check(error)
    return true
  })
}

/** A rule named `name`, with `id` when given. */
function rule(name, id) {
  return new Rule(name, F.x.gt(0), { out: name }, id === undefined ? undefined : { id })
}

describe('Rule ids', () => {
  test('a rule keeps its id; its effective id is the id, or its name without one', () => {
    const renamed = new Rule('gold_customer', F.years.gt(5), { discount: 0.1 }, { id: 'vip', priority: 3 })
    assert.deepEqual([renamed.name, renamed.id, renamed.effectiveId], ['gold_customer', 'vip', 'vip'])
    assert.ok(Object.isFrozen(renamed))
    const bare = rule('loyal')
    assert.deepEqual([bare.id, bare.effectiveId], [null, 'loyal'])
    assert.deepEqual([rule('a', null).id, rule('a', null).effectiveId], [null, 'a'])
    // An id may equal the name, and is taken literally.
    assert.deepEqual([rule('a', 'a').id, rule('a', 'a').effectiveId], ['a', 'a'])
    assert.equal(rule('a', ' PRICING-12 ').effectiveId, ' PRICING-12 ')
  })

  test('the id changes nothing in evaluation, explanations or summaries', () => {
    const rules = new RuleSet([rule('a', 'ID-A'), rule('b')], { hitPolicy: 'collect' })
    const plain = new RuleSet([rule('a'), rule('b')], { hitPolicy: 'collect' })
    const decision = rules.evaluate({ x: 1 })
    assert.deepEqual(decision.fired, ['a', 'b'])
    // Rules are found, and named, by name.
    assert.equal(rules.rule('a').id, 'ID-A')
    assert.equal(rules.get('ID-A'), undefined)
    assert.equal(decision.explain().toString(), plain.evaluate({ x: 1 }).explain().toString())
    assert.equal(JSON.stringify(decision.explain()), JSON.stringify(plain.evaluate({ x: 1 }).explain()))
    assert.equal(JSON.stringify(rules.summary()), JSON.stringify(plain.summary()))
  })

  test('an id is a non-empty string', () => {
    throwsWith(() => rule('r', 5), RuleDefinitionError, /^id must be a string, not the number 5$/, (error) => {
      assert.equal(error.context.rule, 'r')
      assert.match(error.hint, /name/)
    })
    throwsWith(() => rule('r', ['vip']), RuleDefinitionError, /id must be a string/)
    // The core refuses an empty id.
    throwsWith(() => rule('r', ''), RuleDefinitionError, /^a rule id cannot be empty$/, (error) => {
      assert.equal(error.code, 'rule_definition_error')
      assert.equal(error.context.rule, 'r')
      assert.match(error.hint, /leave the id out/)
    })
  })
})

describe('effective ids in a rule set', () => {
  test('two rules with the same id: the error names the second', () => {
    throwsWith(() => new RuleSet([rule('a', 'x'), rule('b', 'x')]), RuleDefinitionError, /the id "x" of rule "b" repeats the id of rule "a"/, (error) => {
      assert.equal(error.code, 'rule_definition_error')
      assert.equal(error.context.rule, 'b')
    })
  })

  test("an id equal to the name of a rule without one, in either order", () => {
    throwsWith(() => new RuleSet([rule('x'), rule('b', 'x')]), RuleDefinitionError, /repeats the name of rule "x", which has no id/, (error) => {
      assert.equal(error.context.rule, 'b')
    })
    throwsWith(() => new RuleSet([rule('a', 'x'), rule('x')]), RuleDefinitionError, /rule "x", which has no id, repeats the id of rule "a"/, (error) => {
      assert.equal(error.context.rule, 'x')
      assert.match(error.hint, /give the rule an id of its own/)
    })
  })

  test('swapped ids are unique; a repeated name is found first', () => {
    const swapped = new RuleSet([rule('a', 'b'), rule('b', 'a')])
    assert.deepEqual(swapped.rules.map((r) => r.effectiveId), ['b', 'a'])
    assert.equal(swapped.rule('a').id, 'b')
    new RuleSet([rule('a', 'a'), rule('b')])
    throwsWith(() => new RuleSet([rule('a', 'x'), rule('b', 'x'), rule('a', 'y')]), RuleDefinitionError, /^two rules are named "a"$/)
  })
})

describe('ids in documents', () => {
  const rules = () => new RuleSet([rule('gold_customer', 'vip'), rule('loyal'), rule('legacy', 'PRICING 12')])

  for (const format of ['json', 'yaml', 'text']) {
    test(`rule sets keep their ids in ${format}`, () => {
      const back = loads(dumps(rules(), { format }), { format })
      assert.deepEqual(
        back.rules.map((r) => [r.name, r.id, r.effectiveId]),
        [
          ['gold_customer', 'vip', 'vip'],
          ['loyal', null, 'loyal'],
          ['legacy', 'PRICING 12', 'PRICING 12'],
        ],
      )
      assert.equal(dumps(back), dumps(rules()))
    })
  }

  test('the JSON document writes the id right after the name, and only when there is one', () => {
    const json = dumps(rule('gold_customer', 'vip'))
    assert.match(json, /^\{"version":1,"rule":\{"name":"gold_customer","id":"vip","when":/)
    assert.deepEqual(rule('gold_customer', 'vip').toJSON().rule.id, 'vip')
    assert.equal('id' in rule('loyal').toJSON().rule, false)
    const read = loads('{"version":1,"rule":{"name":"a","id":"x","when":{"op":"field","path":["p"]}}}')
    assert.deepEqual([read.id, read.effectiveId], ['x', 'x'])
  })

  test('the text syntax: ID after the rule name, printed before PRIORITY', () => {
    const text = 'RULE gold_customer ID vip PRIORITY 2\nIF customer.years > 5'
    const read = ruleFromText(text)
    assert.deepEqual([read.name, read.id, read.effectiveId, read.priority], ['gold_customer', 'vip', 'vip', 2])
    assert.equal(dumps(read, { format: 'text' }), text)
    assert.equal(dumps(rule('legacy', 'PRICING 12'), { format: 'text' }).split('\n')[0], 'RULE legacy ID "PRICING 12"')
    // Elsewhere, `id` is a name.
    assert.equal(ruleFromText('IF id > 5').id, null)
  })

  test('documents with a repeated effective id or an empty id are refused', () => {
    const doc = (ids) =>
      JSON.stringify({
        version: 1,
        ruleset: { rules: ids.map(([name, id]) => ({ name, ...(id === undefined ? {} : { id }), when: { op: 'field', path: ['p'] } })) },
      })
    throwsWith(() => loads(doc([['a', 'x'], ['x']])), RuleDefinitionError, /effective ids must be unique/, (error) => {
      assert.equal(error.context.pointer, '/ruleset/rules/1/name')
    })
    throwsWith(() => loads(doc([['a', 'x'], ['b', 'x']])), RuleDefinitionError, /repeats the id of rule "a"/, (error) => {
      assert.equal(error.context.pointer, '/ruleset/rules/1/id')
    })
    throwsWith(() => loads(doc([['a', '']])), RuleDefinitionError, /^a rule id cannot be empty$/, (error) => {
      assert.equal(error.context.pointer, '/ruleset/rules/0/id')
    })
    throwsWith(() => loads('RULE a ID x IF p\nRULE x IF q', { format: 'text' }), RuleDefinitionError, /effective ids must be unique/, (error) => {
      assert.deepEqual([error.context.line, error.context.column], [2, 1])
    })
    throwsWith(() => ruleFromText('RULE a ID "" IF x'), RuleDefinitionError, /^a rule id cannot be empty$/, (error) => {
      assert.deepEqual([error.context.line, error.context.column], [1, 11])
    })
    throwsWith(() => ruleFromText('RULE a ID IF x'), RuleSyntaxError, /.+/)
  })
})
