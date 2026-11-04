'use strict'

// Actions by name as JavaScript callbacks (spec/rules.md, *Actions*).

const assert = require('node:assert/strict')
const { test } = require('node:test')

const cr = require('./support/cr.js')
const { rules } = require('./support/probe.js')

const RULES = `HIT POLICY rule_order

RULE a
IF x > 0
THEN level = 1
DO first

RULE b
IF x > 10
DO second

RULE c
IF x > 1
DO first`

test('run calls the actions of the selected rules, in rule order', () => {
  const ruleset = rules(RULES)
  const calls = []
  const input = { x: 5 }
  const record = (call) => calls.push(call)
  const decision = ruleset.run(input, { first: record, second: record })
  assert.deepEqual(
    calls.map((c) => [c.ruleName, c.position, c.truth, c.outputJson]),
    [
      ['a', 0, 'true', '{"level":1}'],
      ['c', 2, 'true', '{}'],
    ],
  )
  for (const call of calls) {
    assert.equal(call.input, input)
    // The decision the actions see is the one run returns.
    assert.equal(call.decision, decision)
  }
  assert.equal(calls[0].decision.ruleOutputJson(0), '{"level":1}')
  assert.deepEqual(decision.selected(), [0, 2])
})

test('a Map of actions works too, and extra actions are fine', () => {
  const ruleset = rules(RULES)
  let count = 0
  const bump = () => {
    count += 1
  }
  ruleset.run({ x: 20 }, new Map([['first', bump], ['second', bump], ['unused', bump]]))
  assert.equal(count, 3)
})

test('evaluate never runs actions', () => {
  const ruleset = rules(RULES)
  assert.deepEqual(ruleset.evaluate({ x: 20 }).selected(), [0, 1, 2])
})

test('a missing action is a RegistryError before anything is read', () => {
  const ruleset = rules(RULES)
  let read = false
  const data = {
    get x() {
      read = true
      return 1
    },
  }
  assert.throws(
    () => ruleset.run(data, { first: () => {} }),
    (error) =>
      error instanceof cr.RegistryError && error.context.name === 'second' && error.context.rule === 'b',
  )
  assert.equal(read, false)
  assert.throws(() => ruleset.checkActions(['first']), cr.RegistryError)
  ruleset.checkActions(['first', 'second'])
})

test('actions must be functions in an object or a Map', () => {
  const ruleset = rules(RULES)
  assert.throws(() => ruleset.run({ x: 1 }, { first: 'no', second: () => {} }), TypeError)
  assert.throws(() => ruleset.run({ x: 1 }, null), TypeError)
  assert.throws(() => ruleset.run({ x: 1 }, new Map([[1, () => {}]])), TypeError)
})

test('an action that throws stops the run: ActionError, caused by what it threw', () => {
  const ruleset = rules(RULES)
  const ran = []
  let seen
  const thrown = new TypeError('boom')
  const error = (() => {
    try {
      ruleset.run(
        { x: 20 },
        {
          first: (call) => {
            ran.push(call.ruleName)
          },
          second: (call) => {
            seen = call.decision
            throw thrown
          },
        },
      )
    } catch (caught) {
      return caught
    }
    return undefined
  })()
  assert.ok(error instanceof cr.ActionError)
  assert.equal(error.code, 'action_error')
  assert.equal(error.cause, thrown)
  assert.equal(error.context.rule, 'b')
  assert.deepEqual(error.context.executed, ['a'])
  assert.equal(error.decision, seen)
  assert.deepEqual(ran, ['a'])
  assert.match(error.message, /second.*TypeError: boom/)
  assert.equal(typeof error.hint, 'string')
})

test('an action may throw something that is not an Error', () => {
  const ruleset = rules('RULE a\nIF TRUE\nDO act')
  assert.throws(
    () =>
      ruleset.run(
        {},
        {
          act: () => {
            throw 'plain'
          },
        },
      ),
    (error) => error instanceof cr.ActionError && error.cause === 'plain' && /plain/.test(error.message),
  )
})

test('an action may run rule sets itself', () => {
  const inner = rules('RULE inner\nIF y > 1\nTHEN ok = TRUE')
  const outer = rules('RULE outer\nIF TRUE\nDO nested')
  let output
  outer.run(
    {},
    {
      nested: () => {
        output = inner.evaluate({ y: 2 }).outputJson()
      },
    },
  )
  assert.equal(output, '{"ok":true}')
})

test('strict errors in run are the same as in evaluate', () => {
  const ruleset = rules('STRICT\n\nRULE a\nIF x > 1\nDO act')
  assert.throws(() => ruleset.run({}, { act: () => {} }), cr.MissingFieldError)
})

test('an action that returns a promise fails: actions are synchronous', async () => {
  const ruleset = rules(RULES)
  let unhandled = 0
  const count = () => {
    unhandled += 1
  }
  process.on('unhandledRejection', count)
  try {
    assert.throws(
      () =>
        ruleset.run(
          { x: 5 },
          {
            first: async () => {
              throw new Error('later')
            },
            second: () => {},
          },
        ),
      (error) =>
        error instanceof cr.ActionError &&
        /returned a Promise; actions are synchronous/.test(error.message) &&
        error.context.rule === 'a' &&
        error.context.executed.length === 0 &&
        error.decision instanceof cr.Decision,
    )
    // Any thenable counts.
    assert.throws(() => ruleset.run({ x: 5 }, { first: () => ({ then() {} }), second: () => {} }), cr.ActionError)
    await new Promise((resolve) => setImmediate(resolve))
    assert.equal(unhandled, 0)
  } finally {
    process.off('unhandledRejection', count)
  }
})
