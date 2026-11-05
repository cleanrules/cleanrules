// Rules, rule sets and decisions built in code (spec/rules.md), the same
// concepts and checks as Python's `cleanrules._rules`:
//
//   const rules = new RuleSet([
//     new Rule('vip', F.customer.years.gt(5).and(F.order.total.gt(1000)), { discount: 0.15 }),
//     new Rule('loyal', F.customer.years.gt(2), { discount: 0.05 }),
//   ], { hitPolicy: 'first' })
//   const decision = rules.evaluate({ customer: { years: 3 }, order: { total: 1200 } })
//   decision.output  // { discount: 0.05 }
//   decision.whyNot('vip').status  // 'not_fired'
//
// The core builds, evaluates and explains; this module checks the arguments
// JavaScript gives, converts values, and calls the actions of the rules the
// core selected. It is made once per build (`rules(native, expressions,
// trees, build)`). Rules and rule sets read from documents (`documents.js`)
// are made of the core's own (`fromNativeRule`, `fromNativeRuleSet`).

import { call, registry } from '../actions.js'
import { CleanRulesError, RuleDefinitionError, UnsupportedError, fromNative } from '../errors.js'
import { decode, int } from '../output.js'
import { capture, encodeData, notData, registerOpaque } from '../read.js'
import { ActionContext, Explanation, INTERNAL, RuleResult, Summary, notFound } from './results.js'
import { brand, options, sameBuild, show } from './shared.js'

const INSPECT = Symbol.for('nodejs.util.inspect.custom')
const DENO_INSPECT = Symbol.for('Deno.customInspect')

/** Only the binding makes decisions, and copies of rules. */
const PRIVATE = Symbol('cleanrules.rules')
/** Rules and rule sets made of the core's own, read from a document. */
const NATIVE = Symbol('cleanrules.native')

const RULE_OPTIONS = ['priority', 'tags', 'description', 'metadata', 'do']
const RULESET_OPTIONS = ['hitPolicy', 'aggregate', 'logic', 'strict']

const EMPTY = Object.freeze({})
const I64_MIN = -(2n ** 63n)
const I64_MAX = 2n ** 63n - 1n

/** The `[key, value]` entries of an object or a `Map`, or `null` for anything else. */
function entriesOf(value) {
  if (value instanceof Map) return [...value.entries()]
  if (value !== null && typeof value === 'object' && !Array.isArray(value)) {
    return Object.keys(value).map((key) => [key, value[key]])
  }
  return null
}

/**
 * Refuse to write a rule whose action is code without a name, unless
 * `omitActions` (`spec/json-format.md`, *Writing errors*): a document holds a
 * named action as its name, but never code. `pointer` locates the rule, with
 * `{}` for its position.
 */
function checkActions(ruleList, pointer, omitActions) {
  if (omitActions) return
  for (const [position, rule] of ruleList.entries()) {
    if (rule.do !== null && rule.actionName === null) {
      throw new UnsupportedError(`the action of rule ${JSON.stringify(rule.name)} cannot be written: it is code without a name`, {
        hint: "name it ({ do: 'notify' }) and bind it with RuleSet.bind, or pass { omitActions: true } to write the rule without it",
        context: { pointer: `${pointer.replace('{}', String(position))}/do`, rule: rule.name },
      })
    }
  }
}

/** The rules, rule sets and decisions of one build. */
function rules(native, expressions, trees, build) {
  const { Expr, constant, nodeOf, wrap, wrapJson } = expressions
  // The native decision and the objects of its input, of each `Decision`.
  let decisionParts
  // The native rule of each `Rule`, and the native rule set of each `RuleSet`.
  const natives = new WeakMap()

  function definition(message, rule, hint) {
    const options = rule === null ? {} : { context: { rule } }
    if (hint !== undefined) options.hint = hint
    return new RuleDefinitionError(message, options)
  }

  /** The payload: each value a node, or a constant made from it. */
  function payload(rule, then) {
    if (then === undefined || then === null) return []
    const entries = entriesOf(then)
    sameBuild(then, build)
    if (entries === null || then instanceof Expr) {
      throw definition(`then must be an object of outputs, not ${show(then)}`, rule, 'e.g. { discount: 0.1 }')
    }
    return entries.map(([key, value]) => {
      if (typeof key !== 'string') {
        throw definition(`payload keys must be strings, not ${show(key)}`, rule)
      }
      if (value instanceof Expr) return [key, value]
      sameBuild(value, build)
      try {
        return [key, constant(value)]
      } catch (error) {
        if (!(error instanceof CleanRulesError)) throw error
        throw new error.constructor(`then[${JSON.stringify(key)}]: ${error.message}`, {
          hint: error.hint,
          context: { rule },
          cause: error,
        })
      }
    })
  }

  /** Metadata in the reader's encoding, as text; `null` for none. */
  function metadataWire(rule, metadata) {
    if (metadata === undefined || metadata === null) return null
    const entries = entriesOf(metadata)
    if (entries === null) {
      throw definition(`metadata must be an object, not ${show(metadata)}`, rule)
    }
    if (entries.some(([key]) => typeof key !== 'string')) {
      throw definition('metadata keys must be strings', rule)
    }
    const [form, thrown] = encodeData(metadata)
    const problem = notData(form, 'metadata')
    if (problem !== null) {
      const [bad, at] = problem
      const messages = {
        NaN: `${at} is NaN, which is not data`,
        X: `${at} is not a data value: a ${bad[1]}`,
        IO: `${at} is an integer that does not fit in 64 bits`,
        E: `reading ${at} threw ${bad[1]}`,
      }
      const error = definition(
        messages[bad[0]],
        rule,
        'metadata holds data: null, booleans, numbers, strings, dates, arrays and objects',
      )
      if (bad[0] === 'E') throw new RuleDefinitionError(error.message, { hint: error.hint, context: { rule }, cause: thrown[bad[2]] })
      throw error
    }
    return JSON.stringify(form)
  }

  /** A named condition and its payload; immutable. */
  class Rule {
    #when
    #then
    #priority
    #description
    #do
    #details

    /**
     * @param name Unique within a rule set.
     * @param when The condition: the rule fires when it is true.
     * @param then The payload: keys whose values are constants or value expressions.
     * @param options `priority`, `tags`, `description`, `metadata` and `do` (an
     *   action: a function, or the name of one that `RuleSet.bind` resolves).
     */
    constructor(name, when, then, ruleOptions) {
      if (name === NATIVE) {
        // A rule of the core (`when`), read from a document.
        const made = when
        const [ruleName, priority, description, keys] = JSON.parse(native.ruleParts(made))
        const nodes = native.ruleNodes(made).map(wrap)
        this.name = ruleName
        this.#when = nodes[0]
        this.#then = Object.freeze(Object.fromEntries(keys.map((key, i) => [key, nodes[i + 1]])))
        this.#priority = decode(priority, [])
        this.#description = description
        this.#do = null
        natives.set(this, made)
        Object.freeze(this)
        return
      }
      if (name === PRIVATE) {
        // A copy of the rule `when` with the action `then` (`bind`).
        const [source, action] = [when, then]
        this.name = source.name
        this.#when = source.#when
        this.#then = source.#then
        this.#priority = source.#priority
        this.#description = source.#description
        this.#details = source.#details
        this.#do = action
        natives.set(this, natives.get(source))
        Object.freeze(this)
        return
      }
      if (typeof name !== 'string' || name === '') {
        throw definition(`a rule name must be a non-empty string, not ${show(name)}`, null)
      }
      if (!(when instanceof Expr)) {
        sameBuild(when, build)
        throw definition(`when must be a condition node, not ${show(when)}`, name, 'build conditions with F, e.g. F.total.gt(100)')
      }
      const given = options(ruleOptions, RULE_OPTIONS, '{ priority: 10 }')
      const { priority = 0, tags = [], description = null, metadata = null, do: action = null } = given
      let digits
      if (typeof priority === 'bigint') {
        if (priority < I64_MIN || priority > I64_MAX) {
          throw definition(`priority ${priority} does not fit in 64 bits`, name)
        }
        digits = priority.toString()
      } else if (Number.isSafeInteger(priority)) {
        digits = String(priority)
      } else {
        throw definition(`priority must be an integer (a number or a bigint), not ${show(priority)}`, name)
      }
      if (typeof tags === 'string') {
        throw definition(`tags must be a collection of strings, not the string ${JSON.stringify(tags)}`, name, `use tags: [${JSON.stringify(tags)}]`)
      }
      if (tags === null || typeof tags !== 'object' || typeof tags[Symbol.iterator] !== 'function') {
        throw definition(`tags must be a collection of strings, not ${show(tags)}`, name)
      }
      const tagList = [...tags]
      if (!tagList.every((tag) => typeof tag === 'string')) {
        throw definition('tags must be strings', name)
      }
      if (description !== null && typeof description !== 'string') {
        throw definition(`description must be a string, not ${show(description)}`, name)
      }
      let actionName = null
      let code = null
      if (typeof action === 'string') {
        actionName = action
      } else if (typeof action === 'function') {
        code = action
      } else if (action !== null) {
        throw definition(
          `do must be a function or an action's name, not ${show(action)}`,
          name,
          'pass a function that takes an ActionContext, or a name that RuleSet.bind resolves',
        )
      }
      const items = payload(name, then)
      const wire = metadataWire(name, metadata)
      const list = new native.NodeList()
      let made
      try {
        for (const [key, value] of items) list.push(nodeOf(value), key)
        made = native.ruleNew(name, nodeOf(when), list, digits, tagList, description, wire, actionName)
      } finally {
        list.free?.()
      }
      /** The rule's name. */
      this.name = name
      this.#when = when
      // Own properties, `__proto__` included (`Object.fromEntries` defines them).
      this.#then = Object.freeze(Object.fromEntries(items))
      this.#priority = typeof priority === 'bigint' ? int(digits) : priority
      this.#description = description
      this.#do = code
      natives.set(this, made)
      Object.freeze(this)
    }

    #read() {
      if (this.#details === undefined) {
        const [tags, metadata, action] = JSON.parse(native.ruleDetails(natives.get(this)))
        this.#details = [Object.freeze(tags), Object.freeze(decode(metadata, [])), action]
      }
      return this.#details
    }

    /** The condition. */
    get when() {
      return this.#when
    }

    /** The payload, in order: each key with its node (frozen; empty without one). */
    get then() {
      return this.#then
    }

    /** The priority, for the `priority` hit policy. */
    get priority() {
      return this.#priority
    }

    /** The tags, sorted and without repeats, as the core keeps them. */
    get tags() {
      return this.#read()[0]
    }

    /** The description, or `null`. */
    get description() {
      return this.#description
    }

    /** The metadata, as the core keeps it (frozen). */
    get metadata() {
      return this.#read()[1]
    }

    /** The action's code: given, or bound to its name; `null` if none. */
    get do() {
      return this.#do
    }

    /** The name of the rule's action, if it names one (bound or not); otherwise `null`. */
    get actionName() {
      return this.#read()[2]
    }

    /**
     * The rule document of the JSON format (`spec/json-format.md`, *Rules*),
     * as `dumps(rule)` writes it; an `UnsupportedError` for an action that is
     * code without a name.
     */
    toJSON() {
      checkActions([this], '/rule', false)
      return JSON.parse(native.writeRule(natives.get(this), 'json'))
    }

    [INSPECT]() {
      return `Rule(${this.name}: ${this.#when.toString()})`
    }

    [DENO_INSPECT]() {
      return this[INSPECT]()
    }
  }

  /** Rules evaluated together under a hit policy; immutable, iterable and indexed by name. */
  class RuleSet {
    #rules
    #byName

    /**
     * @param ruleList The rules, in order; names are unique.
     * @param options `hitPolicy` (`'first'` by default), `aggregate` (with
     *   `'collect'`), `logic` (`'boolean'`) and `strict` (`false`).
     */
    constructor(ruleList, setOptions) {
      if (ruleList === NATIVE) {
        // A rule set of the core (`setOptions`), read from a document.
        const made = setOptions
        const items = native.ruleSetRules(made).map((rule) => new Rule(NATIVE, rule))
        natives.set(this, made)
        this.#rules = Object.freeze(items)
        this.#byName = new Map(items.map((rule) => [rule.name, rule]))
        Object.freeze(this)
        return
      }
      if (ruleList === null || typeof ruleList !== 'object' || typeof ruleList[Symbol.iterator] !== 'function') {
        throw definition(`RuleSet takes an iterable of rules, not ${show(ruleList)}`, null)
      }
      const items = [...ruleList]
      for (const rule of items) {
        if (!(rule instanceof Rule)) {
          sameBuild(rule, build)
          throw definition(`RuleSet takes Rule objects, not ${show(rule)}`, null)
        }
      }
      const { hitPolicy, aggregate, logic, strict } = options(setOptions, RULESET_OPTIONS, "{ hitPolicy: 'priority' }")
      const list = new native.RuleList()
      try {
        for (const rule of items) list.push(natives.get(rule))
        natives.set(this, native.ruleSetNew(list, hitPolicy, aggregate, logic, strict))
      } finally {
        list.free?.()
      }
      this.#rules = Object.freeze(items)
      this.#byName = new Map(items.map((rule) => [rule.name, rule]))
      Object.freeze(this)
    }

    /** The rules, in order (frozen). */
    get rules() {
      return this.#rules
    }

    /** How many rules there are. */
    get length() {
      return this.#rules.length
    }

    [Symbol.iterator]() {
      return this.#rules[Symbol.iterator]()
    }

    /** The rule named `name`, or `undefined` (like `Map.prototype.get`); `rule(name)` throws instead. */
    get(name) {
      return this.#byName.get(name)
    }

    /** The rule named `name`; a `RuleNotFoundError` listing the rules when there is none. */
    rule(name) {
      const rule = this.#byName.get(name)
      if (rule === undefined) throw notFound(name, this.#byName.keys())
      return rule
    }

    /** Whether there is a rule named `name`. */
    has(name) {
      return this.#byName.has(name)
    }

    /** The hit policy's name. */
    get hitPolicy() {
      return natives.get(this).hitPolicy
    }

    /** The `collect` aggregate's name, or `null`. */
    get aggregate() {
      return natives.get(this).aggregate
    }

    /** The logic's name. */
    get logic() {
      return natives.get(this).logic
    }

    /** Whether evaluation throws instead of giving unknown (`spec/rules.md`, *Strict mode*). */
    get strict() {
      return natives.get(this).strict
    }

    /** The field paths the rules read, in order of first appearance; native data is read along these only. */
    fieldPaths() {
      return Object.freeze(natives.get(this).fieldPaths().map((path) => Object.freeze(path)))
    }

    /** Evaluate the rule set on one input (`spec/data-model.md`, *JavaScript and TypeScript*). */
    evaluate(data) {
      const [decision, objects] = capture(() => natives.get(this).evaluate(data))
      return new Decision(PRIVATE, this, decision, objects[0] ?? [])
    }

    /** Evaluate each input of a batch, in order; an error's `row` is the input's position. */
    evaluateMany(rows) {
      const iterable = rows !== null && typeof rows === 'object' && typeof rows[Symbol.iterator] === 'function'
      const batch = Array.isArray(rows) || !iterable || rows instanceof Map ? rows : [...rows]
      const [decisions, objects] = capture(() => natives.get(this).evaluateMany(batch))
      return decisions.map((decision, i) => new Decision(PRIVATE, this, decision, objects[i] ?? []))
    }

    /**
     * Read a decision this rule set made, recorded as JSON or YAML (`{ format,
     * limits }`), checked against the rule set. Its opaque values come back as
     * `null`: a recorded decision keeps only their type's name.
     */
    readDecision(text, readOptions) {
      const { format, limits } = options(readOptions, ['format', 'limits'], "{ format: 'yaml' }")
      return new Decision(PRIVATE, this, natives.get(this).readDecision(text, { format, limits }), [])
    }

    /**
     * This rule set with its named actions resolved in `actions` (an object or
     * a `Map` of functions); every named action is checked, whether or not its
     * rule fires. Rules whose action is a function keep it.
     */
    bind(actions) {
      const [names, functions] = registry(actions)
      natives.get(this).checkActions(names)
      const table = new Map(names.map((name, i) => [name, functions[i]]))
      const bound = this.#rules.map((rule) =>
        rule.actionName === null ? rule : new Rule(PRIVATE, rule, table.get(rule.actionName)),
      )
      return new RuleSet(bound, {
        hitPolicy: this.hitPolicy,
        aggregate: this.aggregate,
        logic: this.logic,
        strict: this.strict,
      })
    }

    /**
     * Evaluate like `evaluate`, then call the actions of the selected rules,
     * in rule order, each with an `ActionContext`. Actions are synchronous.
     */
    run(data) {
      // Named actions must be bound: the core names the first that is not.
      const bound = this.#rules.filter((rule) => rule.actionName !== null && rule.do !== null).map((rule) => rule.actionName)
      natives.get(this).checkActions(bound)
      const decision = this.evaluate(data)
      const executed = []
      for (const result of decision.selected) {
        const action = result.rule.do
        if (action === null) continue
        const position = decision.results.indexOf(result)
        const tree = () => {
          const [made, objects] = decisionParts(decision)
          return trees.resultTree(native.ruleTree(made, position), objects)
        }
        const context = new ActionContext(INTERNAL, result.rule, data, result, result.output ?? EMPTY, decision, tree)
        const failure = call(action, context)
        if (failure !== undefined) {
          const [kind, thrown, detail] = failure
          const json = native.actionErrorData(result.rule.name, result.rule.actionName, kind, detail, executed)
          throw fromNative(json, thrown, decision)
        }
        executed.push(result.rule.name)
      }
      return decision
    }

    /** Describe the rule set without evaluating it: rules, fields read, outputs, tags. */
    summary() {
      const ruleset = natives.get(this)
      const actions = this.#rules.map((rule) => rule.do !== null)
      return new Summary(INTERNAL, native.summaryWith(ruleset, actions, 'json'), () =>
        native.summaryWith(ruleset, actions, 'text'),
      )
    }

    /**
     * The rule set document of the JSON format (`spec/json-format.md`, *Rule
     * sets*), as `dumps(ruleset)` writes it; an `UnsupportedError` for an
     * action that is code without a name.
     */
    toJSON() {
      checkActions(this.#rules, '/ruleset/rules/{}', false)
      return JSON.parse(natives.get(this).write('json'))
    }

    [INSPECT]() {
      return `RuleSet(${this.#rules.map((rule) => rule.name).join(', ')}; hit policy ${this.hitPolicy})`
    }

    [DENO_INSPECT]() {
      return this[INSPECT]()
    }
  }

  /** The outcome of evaluating a rule set on one input; immutable. */
  class Decision {
    #native
    #objects
    #output
    #failed
    #explanation

    static {
      decisionParts = (decision) => [decision.#native, decision.#objects]
    }

    constructor(token, ruleset, decision, objects) {
      if (token !== PRIVATE) {
        throw new TypeError('Decision has no public constructor; decisions come from RuleSet.evaluate')
      }
      this.#native = decision
      this.#objects = objects
      const [results, selected, output, failed] = JSON.parse(native.decisionData(decision))
      const frozen = (form) => (form === null ? null : Object.freeze(decode(form, objects)))
      /** The rule set that decided. */
      this.ruleset = ruleset
      /** Every rule's result, in rule order. */
      this.results = Object.freeze(
        results.map(([fired, truth, payload], i) => new RuleResult(INTERNAL, ruleset.rules[i], fired, truth, frozen(payload))),
      )
      /** The results the hit policy selected, in rule order. */
      this.selected = Object.freeze(selected.map((i) => this.results[i]))
      this.#output = frozen(output)
      this.#failed = failed
      Object.freeze(this)
    }

    /** The hit policy that decided. */
    get hitPolicy() {
      return this.ruleset.hitPolicy
    }

    /** The names of the rules that fired, in rule order (selected or not). */
    get fired() {
      return Object.freeze(this.results.filter((result) => result.fired).map((result) => result.rule.name))
    }

    /** The payloads of the selected rules, in rule order. */
    get outputs() {
      return Object.freeze(this.selected.map((result) => result.output ?? EMPTY))
    }

    /**
     * The single output: the winner's payload or the aggregate; `null` when
     * nothing was selected. A `HitPolicyError` for `rule_order`, and `collect`
     * without an aggregate, which have several (read `outputs`).
     */
    get output() {
      if (this.#failed) {
        // The core's error, made again.
        this.#native.outputJson()
      }
      return this.#output
    }

    /** With `collect` and an aggregate, its values (`null` when nothing was selected); otherwise `null`. */
    get aggregated() {
      return this.ruleset.aggregate === null ? null : this.output
    }

    /** Explain every rule: selected, fired but not selected, not fired, or unknown. */
    explain() {
      if (this.#explanation === undefined) {
        const decision = this.#native
        this.#explanation = new Explanation(
          INTERNAL,
          JSON.parse(native.explanationData(decision)),
          this.#objects,
          wrapJson,
          () => JSON.parse(decision.explain('json')),
        )
      }
      return this.#explanation
    }

    /** One rule's explanation, typically of a rule that did not fire; a `RuleNotFoundError` for an unknown name. */
    whyNot(name) {
      return this.explain().forRule(name)
    }

    /** The decision document of the JSON format (`spec/json-format.md`, *Decisions*). */
    toJSON() {
      return JSON.parse(this.#native.write('json'))
    }

    [INSPECT]() {
      return `Decision(${this.hitPolicy}: selected [${this.selected.map((result) => result.rule.name).join(', ')}])`
    }

    [DENO_INSPECT]() {
      return this[INSPECT]()
    }
  }

  registerOpaque(Rule, RuleSet, Decision)
  brand(build, Rule, RuleSet, Decision)

  return {
    Decision,
    Rule,
    RuleSet,
    checkActions,
    /** The `Rule` of a rule of the core. */
    fromNativeRule: (made) => new Rule(NATIVE, made),
    /** The `RuleSet` of a rule set of the core. */
    fromNativeRuleSet: (made) => new RuleSet(NATIVE, made),
    /** The rule of the core of a `Rule`. */
    ruleNative: (rule) => natives.get(rule),
    /** The rule set of the core of a `RuleSet`. */
    ruleSetNative: (ruleset) => natives.get(ruleset),
    /** A copy of `rule` with the action `action` (a function). */
    withAction: (rule, action) => new Rule(PRIVATE, rule, action),
  }
}

export { rules }
