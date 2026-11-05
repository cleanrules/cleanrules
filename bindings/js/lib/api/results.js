// What decisions, explanations, summaries, result trees and decision tables
// give back, as frozen objects of these classes (spec/explain.md; the same
// classes as Python's `cleanrules._rules`, `_trees` and `_tables`). The core
// builds every explanation, summary and tree; this module only turns its
// encoding into objects. The classes are the same for both builds; only the
// engine makes their objects, except `TableOutput`, which a table's header
// may also hold.

import { RuleNotFoundError } from '../errors.js'
import { decode, decodeGap, jsonInt, plainObject } from '../output.js'
import { registerOpaque } from '../read.js'

/** Only the binding makes these objects. */
const INTERNAL = Symbol('cleanrules.internal')

const INSPECT = Symbol.for('nodejs.util.inspect.custom')
const DENO_INSPECT = Symbol.for('Deno.customInspect')

function check(token, name) {
  if (token !== INTERNAL) {
    throw new TypeError(`${name} has no public constructor; the engine makes these objects`)
  }
}

/** A frozen array of `items`. */
const frozen = (items) => Object.freeze([...items])

/** A path, frozen. */
const path = (segments) => frozen(segments)

/** One rule's result in a decision. */
class RuleResult {
  constructor(token, rule, fired, truth, output) {
    check(token, 'RuleResult')
    /** The rule. */
    this.rule = rule
    /** Whether its condition was true. */
    this.fired = fired
    /** Its condition's truth: `"true"`, `"false"` or `"unknown"`. */
    this.truth = truth
    /** Its evaluated payload (frozen), when the hit policy needed it; else `null`. */
    this.output = output
    Object.freeze(this)
  }
}

/** What an action receives (`spec/rules.md`, *Actions*). */
class ActionContext {
  #tree
  #treeOf

  constructor(token, rule, data, result, output, decision, tree) {
    check(token, 'ActionContext')
    this.#treeOf = tree
    /** The rule whose action runs. */
    this.rule = rule
    /** The input, as given to `run`. */
    this.data = data
    /** The rule's result in the decision. */
    this.result = result
    /** The rule's evaluated payload (empty when it has none). */
    this.output = output
    /** The whole decision. */
    this.decision = decision
    Object.freeze(this)
  }

  /**
   * The result tree of the rule's condition: each node's truth or value, and
   * why it is unknown (made when first read, from the input already read).
   */
  get tree() {
    this.#tree ??= this.#treeOf()
    return this.#tree
  }
}

// --- Result trees --------------------------------------------------------

/** A value of a result tree; the lists and maps it makes are frozen (opaque objects are not). */
function treeValue(form, objects) {
  const value = decode(form, objects)
  return Array.isArray(form) && (form[0] === 'l' || form[0] === 'O') ? Object.freeze(value) : value
}

/**
 * The result of evaluating a node and each of its children (`spec/ast.md`,
 * *Evaluating one node*; `spec/json-format.md`, *Result trees*).
 */
class ResultTree {
  #native
  #node
  #wrap
  #json

  /**
   * `form` is the tree's encoding, `json` its JSON form, `cursor` the native
   * nodes in pre-order with the next one to take, `objects` the opaque
   * values' objects, and `wrap` makes an `Expr` of a native node.
   */
  constructor(token, form, json, cursor, objects, wrap) {
    check(token, 'ResultTree')
    this.#native = cursor.nodes[cursor.next]
    cursor.next += 1
    this.#wrap = wrap
    this.#json = json
    const [tag, outcome, reason, children] = form
    /** Its truth value (`"true"`, `"false"` or `"unknown"`) for a condition; `null` for a value. */
    this.truth = tag === 't' ? outcome : null
    /** Its value, for a value node (`null` when it is null); `null` for a condition. */
    this.value = tag === 'v' ? treeValue(outcome, objects) : null
    /** Why it is unknown or null, if it is; otherwise `null`. */
    this.reason = reason === null ? null : new Reason(INTERNAL, reason)
    /** The children's results, in order. */
    this.children = frozen(
      children.map((child, i) => new ResultTree(INTERNAL, child, json.children[i], cursor, objects, wrap)),
    )
    Object.freeze(this)
  }

  /** The node evaluated, as an `Expr` (a `between` has two synthetic comparisons as children). */
  get node() {
    this.#node ??= this.#wrap(this.#native)
    return this.#node
  }

  /**
   * The result tree in the JSON format (`spec/json-format.md`, *Result
   * trees*), for `JSON.stringify` (`Expr.toJSON` says how); a copy.
   */
  toJSON() {
    return JSON.parse(JSON.stringify(this.#json))
  }

  [INSPECT]() {
    return `ResultTree(${this.node}: ${this.truth ?? (typeof this.value === 'string' ? JSON.stringify(this.value) : String(this.value))})`
  }

  [DENO_INSPECT]() {
    return this[INSPECT]()
  }
}

// --- Decision tables -----------------------------------------------------

/**
 * An output column of a decision table: its payload key and its output
 * values, highest priority first. In a header, `new TableOutput('risk',
 * ['high', 'low'])` is the column `THEN risk IN ('high', 'low')`: the values
 * are JavaScript constants, not text.
 */
class TableOutput {
  constructor(name, values = []) {
    if (typeof name !== 'string') {
      throw new TypeError(`an output's name is a string, not ${typeof name}`)
    }
    if (values === null || typeof values !== 'object' || typeof values[Symbol.iterator] !== 'function') {
      throw new TypeError("an output's values are an array of constants")
    }
    /** The payload key. */
    this.name = name
    /** The output values, highest priority first (frozen). */
    this.values = frozen(values)
    Object.freeze(this)
  }
}

/** A row of a decision table: one rule. */
class TableRow {
  constructor(token, name, priority, description, inputs, outputs) {
    check(token, 'TableRow')
    /** The rule's name, or `null` for `row_<n>`. */
    this.name = name
    /** The rule's own priority, or `null`. */
    this.priority = priority
    /** The rule's description, or `null`. */
    this.description = description
    /** One condition per input, testing that input; `null` for any value. */
    this.inputs = frozen(inputs)
    /** One value per output; `null` for no value. */
    this.outputs = frozen(outputs)
    Object.freeze(this)
  }
}

// --- Explanations --------------------------------------------------------

/** Why a result is unknown or null (`spec/ast.md`, *Results and reasons*). */
class Reason {
  constructor(token, [code, paths, types, detail]) {
    check(token, 'Reason')
    /** The reason, such as `"missing_field"` or `"type_mismatch"`. */
    this.code = code
    /** The field paths involved. */
    this.paths = frozen(paths.map(path))
    /** The type groups involved, for type mismatches. */
    this.types = frozen(types)
    /** Informative text, or `null`. */
    this.detail = detail
    Object.freeze(this)
  }
}

/** What a comparison needed: `path op target`, and how far the value was. */
class Counterfactual {
  constructor(token, [at, op, target, gap], objects) {
    check(token, 'Counterfactual')
    /** The field compared. */
    this.path = path(at)
    /** The comparison it needed, such as `"gt"`. */
    this.op = op
    /** The value it was compared with. */
    this.target = decode(target, objects)
    /** How far the value was: a number (or `Decimal`), `{ days }`, `{ microseconds }`, or `null`. */
    this.gap = gap === null ? null : decodeGap(gap)
    Object.freeze(this)
  }
}

/** A condition in a rule's requirement, with what was read (`spec/explain.md`). */
class Cause {
  #node
  #nodeText
  #wrap
  #text

  constructor(token, form, objects, wrap) {
    check(token, 'Cause')
    const [, node, truth, negated, holds, values, reason, counterfactual, text] = form
    this.#nodeText = node
    this.#wrap = wrap
    this.#text = text
    /** Its truth value. */
    this.truth = truth
    /** Whether the rule needs it false (it is under a negation). */
    this.negated = negated
    /** Whether it went the way the rule needed. */
    this.holds = holds
    /** The fields it read, with their values: `[path, value]` pairs, in order. */
    this.values = frozen(values.map(([at, value]) => frozen([path(at), decode(value, objects)])))
    /** Why it is unknown, or `null`. */
    this.reason = reason === null ? null : new Reason(INTERNAL, reason)
    /** For a comparison that did not hold, what it needed; or `null`. */
    this.counterfactual = counterfactual === null ? null : new Counterfactual(INTERNAL, counterfactual, objects)
    Object.freeze(this)
  }

  /** The condition, as an `Expr`. */
  get node() {
    this.#node ??= this.#wrap(this.#nodeText)
    return this.#node
  }

  /** The cause's line in the explanation: `x > 10 is false (x = 8); short by 2`. */
  toString() {
    return this.#text
  }

  [INSPECT]() {
    return `Cause(${this.#text})`
  }

  [DENO_INSPECT]() {
    return this[INSPECT]()
  }
}

/** Requirements that must all hold (`kind` `"all"`) or any of which suffices (`"any"`). */
class CauseGroup {
  constructor(token, kind, items) {
    check(token, 'CauseGroup')
    /** `"all"` or `"any"`. */
    this.kind = kind
    /** The requirements: causes and groups. */
    this.items = frozen(items)
    Object.freeze(this)
  }
}

/** Why a rule that fired was not selected. */
class Note {
  constructor(token, [kind, winner, priority, winnerPriority]) {
    check(token, 'Note')
    /** `"lower_priority"`, `"same_priority"`, `"any_agrees"` or `"earlier_rule"`. */
    this.kind = kind
    /** The rule selected instead. */
    this.winner = winner
    /** This rule's priority, under the `priority` policy; otherwise `null`. */
    this.priority = priority === null ? null : decode(priority, [])
    /** The winner's priority, under the `priority` policy; otherwise `null`. */
    this.winnerPriority = winnerPriority === null ? null : decode(winnerPriority, [])
    Object.freeze(this)
  }
}

function requirement(form, objects, wrap) {
  if (form[0] === 'c') {
    return new Cause(INTERNAL, form, objects, wrap)
  }
  return new CauseGroup(
    INTERNAL,
    form[0],
    form[1].map((item) => requirement(item, objects, wrap)),
  )
}

/** The explanation of one rule. */
class RuleExplanation {
  #text

  constructor(token, [rule, status, note, achievable, need, text], objects, wrap) {
    check(token, 'RuleExplanation')
    this.#text = text
    /** The rule's name. */
    this.rule = rule
    /** `"selected"`, `"fired"`, `"not_fired"` or `"unknown"`. */
    this.status = status
    /** What it needed: a cause, or a group of them. */
    this.requirement = requirement(need, objects, wrap)
    /** Why it was not selected, when it fired; otherwise `null`. */
    this.note = note === null ? null : new Note(INTERNAL, note)
    /** When it did not fire, whether changing the listed conditions can make it fire; otherwise `null`. */
    this.achievable = achievable
    Object.freeze(this)
  }

  /** The causes of the requirement, in order, without their grouping. */
  get causes() {
    const found = []
    const walk = (item) => {
      if (item instanceof Cause) found.push(item)
      else item.items.forEach(walk)
    }
    walk(this.requirement)
    return frozen(found)
  }

  /** The rule's part of the explanation's text (`spec/explain.md`, *Text*). */
  toString() {
    return this.#text
  }
}

/** The not-found error of a rule name, listing the rules there are. */
function notFound(name, names) {
  const available = [...names].sort()
  return new RuleNotFoundError(`no rule named ${JSON.stringify(name)}`, {
    hint: available.length > 0 ? `the rules are: ${available.join(', ')}` : null,
    context: { name, available },
  })
}

/** The explanation of a decision (`spec/explain.md`). */
class Explanation {
  #text
  #json

  constructor(token, data, objects, wrap, json) {
    check(token, 'Explanation')
    const [text, rules] = data
    this.#text = text
    this.#json = json
    /** Each rule's explanation, in rule order. */
    this.rules = frozen(rules.map((rule) => new RuleExplanation(INTERNAL, rule, objects, wrap)))
    /** The names of the selected rules, in rule order. */
    this.selected = frozen(this.rules.filter((rule) => rule.status === 'selected').map((rule) => rule.rule))
    Object.freeze(this)
  }

  /** One rule's explanation; a `RuleNotFoundError` when there is no such rule. */
  forRule(name) {
    const found = this.rules.find((rule) => rule.rule === name)
    if (found === undefined) {
      throw notFound(name, this.rules.map((rule) => rule.rule))
    }
    return found
  }

  /**
   * The explanation document of the JSON format (`spec/json-format.md`,
   * *Explanations*), for `JSON.stringify` (`Expr.toJSON` says how).
   */
  toJSON() {
    return this.#json()
  }

  /** The explanation as text, the same in every language (`spec/explain.md`, *Text*). */
  toString() {
    return this.#text
  }
}

// --- Summaries -----------------------------------------------------------

/** A rule in a summary: name, priority, payload keys, and whether and how it acts. */
class RuleSummary {
  constructor(token, rule) {
    check(token, 'RuleSummary')
    this.name = rule.name
    this.priority = jsonInt(rule.priority)
    /** The payload keys, in order. */
    this.outputs = frozen(rule.outputs)
    /** Whether the rule has an action (code, or a name). */
    this.action = rule.action === true
    /** The action's name, when the rule names it; otherwise `null`. */
    this.actionName = rule.do ?? null
    Object.freeze(this)
  }
}

/** A field a rule set reads, and where. */
class FieldUsage {
  constructor(token, usage) {
    check(token, 'FieldUsage')
    /** The field's path. */
    this.path = path(usage.path)
    /** The rules whose conditions read it, in rule order. */
    this.conditions = frozen(usage.conditions)
    /** The `[rule, key]` payload values that read it, in order. */
    this.payloads = frozen(usage.payloads.map((use) => frozen([use.rule, use.key])))
    Object.freeze(this)
  }
}

/** What a rule set reads and produces, without evaluating it (`spec/explain.md`, *Summaries*). */
class Summary {
  #document
  #text

  constructor(token, document, text) {
    check(token, 'Summary')
    this.#document = document
    this.#text = text
    const data = JSON.parse(document).summary
    /** The hit policy. */
    this.hitPolicy = data.hit_policy
    /** The `collect` aggregate, or `null`. */
    this.aggregate = data.aggregate ?? null
    /** The logic's name. */
    this.logic = data.logic
    /** Whether evaluation is strict. */
    this.strict = data.strict === true
    /** Each rule, in rule order. */
    this.rules = frozen(data.rules.map((rule) => new RuleSummary(INTERNAL, rule)))
    /** Every field read, in order of first appearance, with where it is read. */
    this.fields = frozen(data.fields.map((usage) => new FieldUsage(INTERNAL, usage)))
    /** The payload keys, in order of first appearance. */
    this.outputs = frozen(data.outputs)
    /** Each tag, sorted, with the names of its rules. */
    this.tags = Object.freeze(plainObject(data.tags.map((entry) => [entry.tag, frozen(entry.rules)])))
    Object.freeze(this)
  }

  /** The summary document of the JSON format (`spec/json-format.md`, *Summaries*). */
  toJSON() {
    return JSON.parse(this.#document)
  }

  /** The summary as text, the same in every language. */
  toString() {
    return this.#text()
  }
}

// The binding's own objects are opaque as data.
registerOpaque(
  RuleResult,
  ActionContext,
  ResultTree,
  TableOutput,
  TableRow,
  Reason,
  Counterfactual,
  Cause,
  CauseGroup,
  Note,
  RuleExplanation,
  Explanation,
  RuleSummary,
  FieldUsage,
  Summary,
)

export {
  ActionContext,
  Cause,
  CauseGroup,
  Counterfactual,
  Explanation,
  FieldUsage,
  INTERNAL,
  Note,
  Reason,
  RuleExplanation,
  RuleResult,
  ResultTree,
  RuleSummary,
  Summary,
  TableOutput,
  TableRow,
  notFound,
}
