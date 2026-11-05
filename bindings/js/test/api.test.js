// Rules, rule sets, decisions and actions built in code (task F.2a), with the
// behaviour of Python's `cleanrules._rules`.

import assert from 'node:assert/strict'
import { describe, test } from 'node:test'

import { root } from './support/cr.js'

const {
  ActionContext,
  ActionError,
  CleanRulesError,
  Decision,
  ExpressionError,
  F,
  HitPolicyError,
  MissingFieldError,
  RegistryError,
  Rule,
  RuleDefinitionError,
  RuleNotFoundError,
  RuleResult,
  RuleSet,
  UnsupportedError,
  constant,
  decimal,
  float,
} = root

/** Whether `call` throws `Class` with a message matching `pattern`. */
function throwsWith(call, Class, pattern, check = () => {}) {
  assert.throws(call, (error) => {
    assert.ok(error instanceof Class, `${error?.constructor?.name}: ${error?.message}`)
    assert.match(error.message, pattern)
    check(error)
    return true
  })
}

describe('Rule', () => {
  test('holds what it was given, and what the core keeps', () => {
    const notify = () => {}
    const when = F.years.gt(5)
    const rule = new Rule('vip', when, { discount: decimal('0.15'), base: F.price.mul(2), ['__proto__']: 1 }, {
      priority: 10,
      tags: ['b', 'a', 'b'],
      description: 'long-standing customers',
      metadata: { owner: 'sales', n: [1, 2.5], big: 2n ** 60n, when: new Date(0) },
      do: notify,
    })
    assert.ok(Object.isFrozen(rule))
    assert.equal(rule.name, 'vip')
    assert.equal(rule.when, when)
    assert.deepEqual(Object.keys(rule.then), ['discount', 'base', '__proto__'])
    assert.equal(rule.then.discount.value.text, '0.15')
    assert.ok(rule.then.base.sameAs(F.price.mul(2)))
    assert.ok(Object.isFrozen(rule.then))
    assert.equal(rule.priority, 10)
    assert.deepEqual(rule.tags, ['a', 'b'])
    assert.equal(rule.description, 'long-standing customers')
    assert.deepEqual(rule.metadata.n, [1, 2.5])
    assert.equal(rule.metadata.big, 2n ** 60n)
    assert.equal(String(rule.metadata.when), '1970-01-01T00:00:00Z')
    assert.ok(Object.isFrozen(rule.metadata))
    assert.equal(rule.do, notify)
    assert.equal(rule.actionName, null)

    const named = new Rule('loyal', F.years.gt(2), new Map([['discount', 0.05]]), { do: 'notify', priority: 3n })
    assert.equal(named.actionName, 'notify')
    assert.equal(named.do, null)
    assert.equal(named.priority, 3)
    assert.deepEqual(Object.keys(named.then), ['discount'])
    const bare = new Rule('bare', F.x)
    assert.deepEqual(
      [bare.then, bare.priority, bare.tags, bare.description, bare.metadata, bare.do],
      [{}, 0, [], null, {}, null],
    )
    assert.equal(new Rule('huge', F.x, null, { priority: 2n ** 62n }).priority, 2n ** 62n)
  })

  test('arguments the binding refuses', () => {
    throwsWith(() => new Rule('', F.x), RuleDefinitionError, /non-empty string/)
    throwsWith(() => new Rule(5, F.x), RuleDefinitionError, /non-empty string, not the number 5/)
    throwsWith(() => new Rule('r', 'x > 1'), RuleDefinitionError, /when must be a condition node/, (error) => {
      assert.equal(error.context.rule, 'r')
      assert.match(error.hint, /F\.total\.gt\(100\)/)
    })
    throwsWith(() => new Rule('r', F.x, null, { priority: 1.5 }), RuleDefinitionError, /priority must be an integer/)
    throwsWith(() => new Rule('r', F.x, null, { priority: '1' }), RuleDefinitionError, /priority must be an integer/)
    throwsWith(() => new Rule('r', F.x, null, { priority: 2n ** 63n }), RuleDefinitionError, /does not fit in 64 bits/)
    throwsWith(() => new Rule('r', F.x, null, { tags: 'gold' }), RuleDefinitionError, /not the string "gold"/, (error) => {
      assert.equal(error.hint, 'use tags: ["gold"]')
    })
    throwsWith(() => new Rule('r', F.x, null, { tags: [1] }), RuleDefinitionError, /tags must be strings/)
    throwsWith(() => new Rule('r', F.x, null, { description: 5 }), RuleDefinitionError, /description must be a string/)
    throwsWith(() => new Rule('r', F.x, null, { do: 5 }), RuleDefinitionError, /do must be a function or an action's name/)
    throwsWith(() => new Rule('r', F.x, [1]), RuleDefinitionError, /then must be an object/)
    throwsWith(() => new Rule('r', F.x, F.y), RuleDefinitionError, /then must be an object/)
    throwsWith(() => new Rule('r', F.x, new Map([[1, 2]])), RuleDefinitionError, /payload keys must be strings/)
    throwsWith(() => new Rule('r', F.x, { v: NaN }), ExpressionError, /^then\["v"\]: a constant cannot be NaN/, (error) => {
      assert.equal(error.context.rule, 'r')
      assert.ok(error.cause instanceof ExpressionError)
    })
    throwsWith(() => new Rule('r', F.x, { v: undefined }), ExpressionError, /then\["v"\]: undefined is not a constant/)
    throwsWith(() => new Rule('r', F.x, null, { metadata: { a: [1, NaN] } }), RuleDefinitionError, /metadata\["a"\]\[1\] is NaN/)
    throwsWith(() => new Rule('r', F.x, null, { metadata: { s: new Set() } }), RuleDefinitionError, /metadata\["s"\] is not a data value: a Set/)
    throwsWith(() => new Rule('r', F.x, null, { metadata: { n: 2n ** 64n } }), RuleDefinitionError, /does not fit in 64 bits/)
    throwsWith(() => new Rule('r', F.x, null, { metadata: new Map([[1, 2]]) }), RuleDefinitionError, /keys must be strings/)
    throwsWith(() => new Rule('r', F.x, null, { metadata: 'x' }), RuleDefinitionError, /metadata must be an object/)
    throwsWith(() => new Rule('r', F.x, null, { priorty: 1 }), TypeError, /unknown option "priorty"/)
    throwsWith(() => new Rule('r', F.x, null, 'first'), TypeError, /options must be an object/)
  })

  test('rules the core refuses', () => {
    throwsWith(() => new Rule('r', F.x.add(1)), RuleDefinitionError, /is a value/, (error) => assert.equal(error.context.rule, 'r'))
    throwsWith(() => new Rule('r', constant(null)), ExpressionError, /null constant/)
    throwsWith(() => new Rule('r', F.x, { v: F.y.gt(1) }), RuleDefinitionError, /payload value "v" is a condition/)
    throwsWith(() => new Rule('r', F.x, null, { do: '' }), RuleDefinitionError, /action name cannot be empty/)
    throwsWith(() => new Rule('r', F.x, null, { metadata: { d: decimal('1e40') } }), RuleDefinitionError, /metadata\["d"\] is out of range/)
  })
})

describe('RuleSet', () => {
  const vip = new Rule('vip', F.years.gt(5), { discount: 0.15 }, { priority: 10 })
  const loyal = new Rule('loyal', F.years.gt(2), { discount: 0.05 }, { priority: 5 })

  test('an immutable sequence of rules, indexed by name', () => {
    const rules = new RuleSet([vip, loyal])
    assert.ok(Object.isFrozen(rules))
    assert.equal(rules.length, 2)
    assert.deepEqual([...rules], [vip, loyal])
    assert.deepEqual(rules.rules, [vip, loyal])
    assert.ok(Object.isFrozen(rules.rules))
    assert.equal(rules.get('loyal'), loyal)
    assert.equal(rules.has('vip'), true)
    assert.equal(rules.has('nope'), false)
    assert.deepEqual([rules.hitPolicy, rules.aggregate, rules.logic, rules.strict], ['first', null, 'boolean', false])
    assert.deepEqual(rules.fieldPaths(), [['years']])
    const collect = new RuleSet(new Set([vip, loyal]), { hitPolicy: 'COLLECT', aggregate: 'Sum', strict: true })
    assert.deepEqual([collect.hitPolicy, collect.aggregate, collect.strict], ['collect', 'sum', true])
  })

  test('get gives undefined for an unknown name; rule throws RuleNotFoundError listing the rules', () => {
    const rules = new RuleSet([vip, loyal])
    assert.equal(rules.get('vp'), undefined)
    assert.equal(rules.rule('vip'), vip)
    throwsWith(() => rules.rule('vp'), RuleNotFoundError, /^no rule named "vp"$/, (error) => {
      assert.equal(error.hint, 'the rules are: loyal, vip')
      assert.deepEqual(error.context.available, ['loyal', 'vip'])
      assert.equal(error.context.name, 'vp')
    })
    throwsWith(() => new RuleSet([]).rule('x'), RuleNotFoundError, /no rule/, (error) => assert.equal(error.hint, null))
  })

  test('field paths are frozen; rules and rule sets are their JSON documents, actions as names', () => {
    const rules = new RuleSet([vip, loyal])
    const paths = rules.fieldPaths()
    assert.ok(Object.isFrozen(paths) && Object.isFrozen(paths[0]))
    assert.equal(vip.toJSON().version, 1)
    assert.equal(vip.toJSON().rule.name, 'vip')
    assert.deepEqual(rules.toJSON().ruleset.rules.map((rule) => rule.name), ['vip', 'loyal'])
    assert.equal(JSON.parse(JSON.stringify({ rules })).rules.ruleset.hit_policy, 'first')
    // A named action is written as its name; code is refused, with where it is.
    assert.equal(new Rule('n', F.x, null, { do: 'notify' }).toJSON().rule.do, 'notify')
    const coded = new Rule('c', F.x, null, { do: () => {} })
    throwsWith(() => coded.toJSON(), UnsupportedError, /action of rule "c" cannot be written: it is code without a name/, (error) => {
      assert.equal(error.code, 'unsupported')
      assert.equal(error.context.pointer, '/rule/do')
      assert.equal(error.context.rule, 'c')
      assert.match(error.hint, /omitActions/)
    })
    throwsWith(() => new RuleSet([vip, coded]).toJSON(), UnsupportedError, /cannot be written/, (error) => {
      assert.equal(error.context.pointer, '/ruleset/rules/1/do')
    })
  })

  test('arguments and configurations it refuses', () => {
    throwsWith(() => new RuleSet(vip), RuleDefinitionError, /iterable of rules/)
    throwsWith(() => new RuleSet([vip, 'loyal']), RuleDefinitionError, /takes Rule objects, not the string "loyal"/)
    throwsWith(() => new RuleSet([vip, vip]), RuleDefinitionError, /vip/)
    throwsWith(() => new RuleSet([vip], { hitPolicy: 'last' }), RuleDefinitionError, /unknown hit policy "last"/, (error) => {
      assert.match(error.hint, /first, priority, unique, any, rule_order, collect/)
    })
    throwsWith(() => new RuleSet([vip], { hitPolicy: 'collect', aggregate: 'avg' }), RuleDefinitionError, /unknown aggregate "avg"/)
    throwsWith(() => new RuleSet([vip], { aggregate: 'sum' }), RuleDefinitionError, /collect/)
    throwsWith(() => new RuleSet([vip], { strict: 'yes' }), RuleDefinitionError, /strict must be a boolean/)
    throwsWith(() => new RuleSet([vip], { hitPolicy: 1 }), RuleDefinitionError, /hitPolicy is a string/)
    throwsWith(() => new RuleSet([vip], { logic: 'fuzzy' }), RegistryError, /fuzzy/)
    throwsWith(() => new RuleSet([vip], { policy: 'first' }), TypeError, /unknown option "policy"/)
  })
})

describe('Decision', () => {
  const rules = new RuleSet(
    [
      new Rule('vip', F.years.gt(5), { discount: decimal('0.15') }),
      new Rule('loyal', F.years.gt(2), { discount: decimal('0.05'), years: F.years }),
      new Rule('any', F.years.ge(0)),
    ],
    { hitPolicy: 'first' },
  )

  test('results, selection and outputs as native values', () => {
    const decision = rules.evaluate({ years: 3 })
    assert.ok(decision instanceof Decision)
    assert.ok(Object.isFrozen(decision))
    assert.equal(decision.ruleset, rules)
    assert.equal(decision.hitPolicy, 'first')
    assert.deepEqual(decision.fired, ['loyal', 'any'])
    assert.equal(decision.results.length, 3)
    for (const [i, result] of decision.results.entries()) {
      assert.ok(result instanceof RuleResult)
      assert.equal(result.rule, rules.rules[i])
    }
    assert.deepEqual(
      decision.results.map((result) => [result.fired, result.truth]),
      [
        [false, 'false'],
        [true, 'true'],
        [true, 'true'],
      ],
    )
    assert.deepEqual(decision.selected, [decision.results[1]])
    assert.equal(decision.output.discount.text, '0.05')
    assert.equal(decision.output.years, 3)
    assert.ok(Object.isFrozen(decision.output))
    assert.deepEqual(decision.outputs.length, 1)
    assert.equal(decision.outputs[0], decision.selected[0].output)
    assert.equal(decision.aggregated, null)
    assert.equal(rules.evaluate({ years: -1 }).output, null)
    assert.equal(rules.evaluate({}).results[0].truth, 'unknown')
    assert.equal(decision.toJSON().version, 1)
    assert.deepEqual(decision.toJSON().decision.selected, ['loyal'])
    assert.throws(() => new Decision(), TypeError)
  })

  test('multi-hit: outputs, the aggregate, and HitPolicyError for output', () => {
    const items = [new Rule('a', F.x.gt(1), { n: 1 }), new Rule('b', F.x.gt(2), { n: float(2.5) }), new Rule('c', F.x.gt(9), { n: 4 })]
    const order = new RuleSet(items, { hitPolicy: 'rule_order' }).evaluate({ x: 5 })
    assert.deepEqual(order.outputs, [{ n: 1 }, { n: 2.5 }])
    throwsWith(() => order.output, HitPolicyError, /.+/, (error) => assert.equal(error.code, 'hit_policy_error'))
    assert.equal(order.aggregated, null)
    const sum = new RuleSet(items, { hitPolicy: 'collect', aggregate: 'sum' })
    assert.deepEqual(sum.evaluate({ x: 5 }).aggregated, { n: 3.5 })
    assert.deepEqual(sum.evaluate({ x: 5 }).output, { n: 3.5 })
    assert.equal(sum.evaluate({ x: 0 }).aggregated, null)
  })

  test('evaluateMany: arrays and other iterables; errors name the row', () => {
    const decisions = rules.evaluateMany([{ years: 9 }, { years: 3 }])
    assert.deepEqual(
      decisions.map((decision) => decision.selected[0].rule.name),
      ['vip', 'loyal'],
    )
    const set = new Set([1])
    const echo = new RuleSet([new Rule('r', F.go, { s: F.s })])
    const fromIterable = echo.evaluateMany(new Set([{ go: true, s: set }, { go: true, s: 2 }]))
    assert.equal(fromIterable[0].output.s, set)
    assert.equal(fromIterable[1].output.s, 2)
    const strict = new RuleSet([new Rule('r', F.x.gt(1))], { strict: true })
    throwsWith(() => strict.evaluateMany([{ x: 2 }, {}]), MissingFieldError, /x/, (error) => assert.equal(error.context.row, 1))
    throwsWith(() => strict.evaluateMany('rows'), CleanRulesError, /a batch is a list of inputs/)
    throwsWith(() => strict.evaluate({}), MissingFieldError, /x/, (error) => assert.equal(error.context.rule, 'r'))
    assert.throws(() => strict.evaluate(undefined), TypeError)
  })

  test('readDecision: a recorded decision, explained like an evaluated one', () => {
    const decision = rules.evaluate({ years: 9 })
    const recorded = rules.readDecision(JSON.stringify(decision.toJSON()))
    assert.deepEqual(recorded.fired, decision.fired)
    assert.equal(recorded.output.discount.text, '0.15')
    assert.equal(String(recorded.explain()), String(decision.explain()))
    throwsWith(() => rules.readDecision('{}', { fromat: 'yaml' }), TypeError, /unknown option "fromat"/)
    throwsWith(() => rules.readDecision('{}', 'yaml'), TypeError, /options must be an object/)
    // JSON is YAML too.
    const yaml = rules.readDecision(JSON.stringify(decision.toJSON()), { format: 'yaml' })
    assert.deepEqual(yaml.fired, decision.fired)
    const other = new RuleSet([new Rule('vip', F.y.gt(5))])
    throwsWith(() => other.readDecision(JSON.stringify(decision.toJSON())), CleanRulesError, /.+/, (error) => {
      assert.equal(error.code, 'decision_mismatch')
    })
  })

  test('getters that evaluate inside data keep each decision its own objects', () => {
    const echo = new RuleSet([new Rule('r', F.go, { s: F.s })])
    const inner = new Set(['inner'])
    const outer = new Set(['outer'])
    let nested
    const data = {
      go: true,
      get s() {
        nested = echo.evaluate({ go: true, s: inner })
        return outer
      },
    }
    const [first, second] = echo.evaluateMany([data, { go: true, s: inner }])
    assert.equal(first.output.s, outer)
    assert.equal(second.output.s, inner)
    assert.equal(nested.output.s, inner)
  })
})

describe('actions', () => {
  const calls = []
  const record = (context) => calls.push([context.rule.name, { ...context.output }])
  const fail = (context) => {
    throw new RangeError(`cannot handle ${context.rule.name}`)
  }
  const shop = (doVip = record, doLoyal = record, options = {}) =>
    new RuleSet(
      [
        new Rule('vip', F.years.gt(5), { discount: decimal('0.15') }, { do: doVip }),
        new Rule('loyal', F.years.gt(2), { discount: decimal('0.05') }, { do: doLoyal }),
        new Rule('quiet', F.years.gt(0)),
      ],
      options,
    )
  const names = () => calls.map(([name]) => name)

  test('evaluate never runs them; run calls the selected rules\' actions in order', () => {
    calls.length = 0
    shop().evaluate({ years: 9 })
    assert.deepEqual(calls, [])
    const decision = shop(record, record, { hitPolicy: 'rule_order' }).run({ years: 9 })
    assert.deepEqual(decision.fired, ['vip', 'loyal', 'quiet'])
    assert.deepEqual(names(), ['vip', 'loyal'])
    assert.equal(calls[0][1].discount.text, '0.15')
    calls.length = 0
    shop().run({ years: 9 })
    assert.deepEqual(names(), ['vip'])
  })

  test('an action receives its ActionContext', () => {
    const seen = []
    const data = { years: 3 }
    const rules = shop(record, (context) => seen.push(context))
    const decision = rules.run(data)
    const [context] = seen
    assert.ok(context instanceof ActionContext)
    assert.ok(Object.isFrozen(context))
    assert.equal(context.rule, rules.get('loyal'))
    assert.equal(context.data, data)
    assert.equal(context.result, decision.selected[0])
    assert.equal(context.result.truth, 'true')
    assert.equal(context.output, decision.selected[0].output)
    assert.equal(context.decision, decision)
  })

  test('an action that throws stops the run: ActionError with the cause, executed and decision', () => {
    calls.length = 0
    throwsWith(
      () => shop(record, fail, { hitPolicy: 'rule_order' }).run({ years: 9 }),
      ActionError,
      /^the action of rule "loyal" threw RangeError: cannot handle loyal$/,
      (error) => {
        assert.equal(error.code, 'action_error')
        assert.equal(error.context.rule, 'loyal')
        assert.deepEqual(error.context.executed, ['vip'])
        assert.ok(error.cause instanceof RangeError)
        assert.ok(error.decision instanceof Decision)
        assert.deepEqual(error.decision.fired, ['vip', 'loyal', 'quiet'])
        assert.match(error.hint, /later actions did not run/)
        assert.equal(error.toJSON().code, 'action_error')
        assert.deepEqual(error.toJSON().executed, ['vip'])
      },
    )
    assert.deepEqual(names(), ['vip'])
  })

  test('actions are synchronous: a promise is an ActionError, its rejection handled', async () => {
    let unhandled = 0
    const onUnhandled = () => {
      unhandled += 1
    }
    const isNode = typeof globalThis.process?.on === 'function'
    if (isNode) globalThis.process.on('unhandledRejection', onUnhandled)
    try {
      throwsWith(
        () => shop(() => Promise.reject(new Error('later'))).run({ years: 9 }),
        ActionError,
        /^the action of rule "vip" returned a Promise; actions are synchronous$/,
        (error) => assert.equal(error.cause, undefined),
      )
      throwsWith(() => shop(async () => {}).run({ years: 9 }), ActionError, /returned a Promise/)
      await new Promise((resolve) => setTimeout(resolve, 10))
    } finally {
      if (isNode) globalThis.process.off('unhandledRejection', onUnhandled)
    }
    assert.equal(unhandled, 0)
  })

  test('named actions: bind resolves them in a registry, checked whether or not they fire', () => {
    calls.length = 0
    const rules = new RuleSet([
      new Rule('vip', F.years.gt(5), { discount: 0.15 }, { do: 'notify' }),
      new Rule('loyal', F.years.gt(2), { discount: 0.05 }),
      new Rule('audit', F.years.gt(0), null, { do: 'log' }),
    ], { hitPolicy: 'rule_order' })
    // Unbound, they evaluate and summarise.
    assert.deepEqual(rules.evaluate({ years: 9 }).fired, ['vip', 'loyal', 'audit'])
    assert.deepEqual(
      rules.summary().rules.map((rule) => [rule.action, rule.actionName]),
      [
        [true, 'notify'],
        [false, null],
        [true, 'log'],
      ],
    )
    const bound = rules.bind({ notify: record, log: record, unused: fail })
    assert.notEqual(bound, rules)
    assert.equal(rules.get('vip').do, null)
    assert.deepEqual(bound.rules.map((rule) => rule.do), [record, null, record])
    assert.deepEqual(bound.rules.map((rule) => rule.actionName), ['notify', null, 'log'])
    assert.equal(bound.get('loyal'), rules.get('loyal'))
    bound.run({ years: 9 })
    assert.deepEqual(names(), ['vip', 'audit'])
    assert.deepEqual(calls[1][1], {})
    const viaMap = rules.bind(new Map([['notify', fail], ['log', record]]))
    throwsWith(() => viaMap.run({ years: 9 }), ActionError, /^the action "notify" of rule "vip" threw RangeError/)

    throwsWith(() => rules.bind({ notify: record }), RegistryError, /^the action "log" of rule "audit" is not registered/, (error) => {
      assert.equal(error.context.name, 'log')
      assert.equal(error.context.rule, 'audit')
      assert.deepEqual(error.context.available, ['notify'])
    })
    throwsWith(() => rules.bind({ notify: record, log: 'record' }), TypeError, /is not a function/)
    throwsWith(() => rules.bind([['notify', record]]), TypeError, /.+/)
    throwsWith(() => rules.bind('notify'), TypeError, /an object or a Map/)
    // Code actions are kept; binding again replaces named ones.
    const mixed = new RuleSet([new Rule('a', F.x.gt(1), null, { do: fail }), new Rule('b', F.x.gt(1), null, { do: 'log' })])
    const rebound = mixed.bind({ log: record }).bind({ log: fail })
    assert.deepEqual(rebound.rules.map((rule) => rule.do), [fail, fail])
  })

  test('run refuses unbound names before evaluating', () => {
    calls.length = 0
    const strict = new RuleSet([new Rule('vip', F.years.gt(5), null, { do: 'notify' })], { strict: true })
    throwsWith(() => strict.run({}), RegistryError, /notify/, (error) => {
      assert.deepEqual([error.context.name, error.context.rule], ['notify', 'vip'])
    })
    throwsWith(() => strict.bind({ notify: record }).run({}), MissingFieldError, /years/)
    assert.deepEqual(calls, [])
  })

  test('the summary marks actions given as code', () => {
    const summary = shop(record, null).summary()
    assert.deepEqual(
      summary.rules.map((rule) => [rule.action, rule.actionName]),
      [
        [true, null],
        [false, null],
        [false, null],
      ],
    )
  })

  test('the native layer and the idiomatic API share the error classes', () => {
    assert.ok(new UnsupportedError('x') instanceof CleanRulesError)
  })
})
