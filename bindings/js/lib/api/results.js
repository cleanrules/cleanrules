// What decisions, explanations, summaries, result trees and decision tables
// give back, as frozen objects of these classes (spec/explain.md; the same
// classes as Python's `cleanrules._rules`, `_trees` and `_tables`). The core
// builds every explanation, summary and tree; this module only turns its
// encoding into objects. The classes are the same for both builds; only the
// engine makes their objects, except `TableOutput`, which a table's header
// may also hold.

import { RuleNotFoundError, fromNative } from '../errors.js'
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
    /** The payload keys, sorted by Unicode code point. */
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

// --- Audits --------------------------------------------------------------

/** `value` with the arrays and plain objects it holds frozen, deeply (the engine's values are frozen already). */
function frozenData(value) {
  if (Array.isArray(value) || (value !== null && typeof value === 'object' && Object.getPrototypeOf(value) === Object.prototype)) {
    for (const item of Object.values(value)) frozenData(item)
    Object.freeze(value)
  }
  return value
}

/** A rule of what was audited: diagnostics name rules by `name`. */
class AuditedRule {
  constructor(token, rule) {
    check(token, 'AuditedRule')
    /** The rule's name. */
    this.name = rule.name
    /** Its own id, or `null` (`Rule.id`). */
    this.id = rule.id ?? null
    /** Its row's position in a decision table, from 0; `null` for a rule set. */
    this.row = rule.row ?? null
    Object.freeze(this)
  }
}

/** A check of an audit and its status. */
class CheckResult {
  constructor(token, result) {
    check(token, 'CheckResult')
    /** The check: `"types"`, `"fields"`, `"unsatisfiable"`, `"shadowed"`, `"overlap"`, `"gap"`, or one a later version adds. */
    this.check = result.check
    /** `"complete"`, `"partial"` or `"not_applicable"`. */
    this.status = result.status
    Object.freeze(this)
  }
}

/** A test the audit does not model, or a node read as one because of a limit. */
class NotModelled {
  constructor(token, item) {
    check(token, 'NotModelled')
    /** The rule, by name. */
    this.rule = item.rule
    /** The node's tag, such as `"matches"`. */
    this.op = item.op
    /** Where it is, as a JSON Pointer into the audited document. */
    this.pointer = item.pointer
    /** The limit that made it one (`"max_terms"`), or `null`. */
    this.limit = item.limit ?? null
    Object.freeze(this)
  }
}

/** An item a check left undecided, or the items it did not reach. */
class Undecided {
  constructor(token, item) {
    check(token, 'Undecided')
    /** The check. */
    this.check = item.check
    /** The rules involved, by name; none for the gap check and for the items `maxSteps` did not reach. */
    this.rules = frozen(item.rules)
    /** Why: `"not_modelled"`, `"no_witness"`, `"limit"`, or one a later version adds. */
    this.reason = item.reason
    /** The limit, when `reason` is `"limit"`; otherwise `null`. */
    this.limit = item.limit ?? null
    /** How many items `maxSteps` left unreached, for that entry; otherwise `null`. */
    this.remaining = item.remaining ?? null
    Object.freeze(this)
  }
}

/** One finding of an audit (`spec/audit.md`, *Checks*). */
class Diagnostic {
  #text

  constructor(token, item, witness, text) {
    check(token, 'Diagnostic')
    /** What was found: a `DiagnosticCode`, or a code a later version adds. */
    this.code = item.code
    /** `"error"`, `"warning"` or `"info"`. */
    this.severity = item.severity
    /** The rules involved, by name: names in `Audit.rules`. */
    this.rules = frozen(item.rules)
    /** What was found and how to fix it, in words. */
    this.message = item.message
    /** The test or cell, as a JSON Pointer into the audited document, or `null`. */
    this.pointer = item.pointer ?? null
    /** The field, for `mixed_types` and `unlisted_field`; otherwise `null`. */
    this.path = item.path === undefined ? null : path(item.path)
    /** The type groups a field is used with, for `mixed_types`. */
    this.groups = frozen(item.groups ?? [])
    /**
     * The input that shows it, for `overlap` and `gap`, as native data
     * (frozen) that the audited rules evaluate; otherwise `null`.
     */
    this.witness = witness === null ? null : frozenData(decode(witness, []))
    this.#text = text
    Object.freeze(this)
  }

  /** The diagnostic as the audit's text writes it (the core's): its line, then its indented lines. */
  toString() {
    return this.#text
  }
}

/** The report of an audit (`spec/audit.md`): `RuleSet.audit()` and `DecisionTable.audit()` make it. */
class Audit {
  #document
  #text
  #atLeast

  constructor(token, data) {
    check(token, 'Audit')
    const [document, witnesses, maxSeverity, text, texts, atLeast] = JSON.parse(data)
    this.#document = JSON.stringify(document)
    this.#text = text
    this.#atLeast = atLeast
    const audit = document.audit
    /** The kind of rules audited: `"ruleset"` or `"decision_table"` (a later version may add sources). */
    this.source = audit.source
    /**
     * `"bundle"` or `"rule"` when the core audited the rules as that document
     * holds them (`Document.audit` of the native layer, the command line),
     * with pointers into it; `null` for a rule set or a table audited
     * directly, as `RuleSet.audit` and `DecisionTable.audit` do.
     */
    this.document = audit.document ?? null
    /** The hit policy: a rule set's, as the table policy of the same name. */
    this.hitPolicy = audit.hit_policy
    /** The `collect` aggregate, or `null`. */
    this.aggregate = audit.aggregate ?? null
    /** The logic's name. */
    this.logic = audit.logic
    /** Whether evaluation is strict; witnesses are evaluated so. */
    this.strict = audit.strict === true
    /** The rule set digest of what was audited, when asked for (`rulesetDigest`); otherwise `null`. */
    this.rulesetDigest = audit.ruleset_digest ?? null
    /** The limits it ran with; one a later version adds appears in `toJSON()` until this object gains it. */
    this.limits = Object.freeze({ maxTerms: jsonInt(audit.limits.max_terms), maxSteps: jsonInt(audit.limits.max_steps) })
    /** The rules, in rule order. */
    this.rules = frozen(audit.rules.map((rule) => new AuditedRule(INTERNAL, rule)))
    /** Each check's status, in check order. */
    this.checks = frozen(audit.checks.map((result) => new CheckResult(INTERNAL, result)))
    /** The tests not modelled, in rule order and depth-first pre-order. */
    this.notModelled = frozen(audit.not_modelled.map((item) => new NotModelled(INTERNAL, item)))
    /** The items left undecided, in check order. */
    this.undecided = frozen(audit.undecided.map((item) => new Undecided(INTERNAL, item)))
    /** The findings, by code and then in their check's order. */
    this.diagnostics = frozen(audit.diagnostics.map((item, i) => new Diagnostic(INTERNAL, item, witnesses[i], texts[i])))
    /** The most serious severity among the diagnostics, or `null` without findings. */
    this.maxSeverity = maxSeverity
    Object.freeze(this)
  }

  /**
   * Whether some diagnostic is at least as serious as `severity`
   * (`'info'`, `'warning'` or `'error'`), as the core orders them: compare
   * severities this way, not as strings. A `RangeError` for another name.
   */
  atLeast(severity) {
    if (typeof severity !== 'string' || !Object.hasOwn(this.#atLeast, severity)) {
      throw new RangeError(`unknown severity ${typeof severity === 'string' ? JSON.stringify(severity) : String(severity)}; use 'info', 'warning' or 'error'`)
    }
    return this.#atLeast[severity]
  }

  /** The status of a check; `"not_applicable"` for one the report does not list. */
  status(name) {
    return this.checks.find((result) => result.check === name)?.status ?? 'not_applicable'
  }

  /**
   * The audit document of the JSON format (`spec/audit.md`, *JSON*), for
   * `JSON.stringify` (`Expr.toJSON` says how); a copy. Witnesses are in the
   * JSON encoding of values (a decimal is `{"$decimal": "0.15"}`).
   */
  toJSON() {
    return JSON.parse(this.#document)
  }

  /** The audit as text, the same in every language (`spec/audit.md`, *Text*). */
  toString() {
    return this.#text
  }

  [INSPECT]() {
    const what = this.source === 'ruleset' ? 'rule set' : 'decision table'
    const count = this.diagnostics.length
    const worst = this.maxSeverity === null ? '' : `, max severity ${this.maxSeverity}`
    return `Audit(${what}: ${count} diagnostic${count === 1 ? '' : 's'}${worst})`
  }

  [DENO_INSPECT]() {
    return this[INSPECT]()
  }
}

// --- Test runs ------------------------------------------------------------

/** The engine's limits as a test run's `engine` names them, by the binding's names. */
const ENGINE_LIMITS = {
  max_size: 'maxSize',
  max_nesting: 'maxNesting',
  max_depth: 'maxDepth',
  max_path_segments: 'maxPathSegments',
  max_table_size: 'maxTableSize',
  max_values: 'maxValues',
}

/** A key of an expectation that does not match (`spec/tests.md`, *Test runs*). */
class Mismatch {
  constructor(token, data, form) {
    check(token, 'Mismatch')
    const [key, expected, actual, given, known] = data
    /**
     * The key: `"selected"`, `"output"`, `"outputs"`, `"unknown"`,
     * `"error"`, or one a later version adds, whose `expected` and `actual`
     * are in their JSON form.
     */
    this.key = key
    let wanted = expected
    let got = actual
    if (!known) {
      wanted = form.expected
      got = form.actual
    } else if (key === 'output') {
      // What is expected reads as a case file writes it: a float stays one.
      wanted = decode(expected, [], true)
      got = given ? decode(actual, []) : undefined
    } else if (key === 'outputs') {
      wanted = expected.map((form) => decode(form, [], true))
      got = actual.map((form) => decode(form, []))
    } else if (key === 'error') {
      got = actual === null ? null : fromNative(actual, null, null)
    }
    /**
     * What the case expects: rule ids (`selected`, `unknown`, sorted), an
     * object or `null` for nothing selected (`output`), objects
     * (`outputs`), or an error code (`error`); frozen.
     */
    this.expected = frozenData(wanted)
    /**
     * What was given, in the same form; for `error`, the error raised (a
     * `CleanRulesError`) or `null`. `undefined` when nothing was given (a
     * decision without a single output), and then `reason` says why.
     */
    this.actual = got instanceof Error ? got : frozenData(got)
    /** Why nothing was given to compare, or `null`. */
    this.reason = form.reason ?? null
    Object.freeze(this)
  }
}

/** The result of one case, or of a case file that cannot be run (`spec/tests.md`, *Test runs*). */
class CaseResult {
  #form

  constructor(token, data) {
    check(token, 'CaseResult')
    const [form, mismatches, error] = JSON.parse(data)
    this.#form = JSON.stringify(form)
    /** The path of the case's file, or `null`. */
    this.path = form.path ?? null
    /** The case's id; `null` for a case file that cannot be run. */
    this.id = form.id ?? null
    /** `"passed"`, `"failed"` or `"error"`. */
    this.status = form.status
    /** For a failed case, the keys that do not match, in the order of the expectation's keys. */
    this.mismatches = frozen(mismatches.map((item, i) => new Mismatch(INTERNAL, item, form.mismatches[i])))
    /** For an error, the error (a `CleanRulesError`); otherwise `null`. */
    this.error = error === null ? null : fromNative(error, null, null)
    /** For a failed case explained, the text of its decision's explanation; otherwise `null`. */
    this.explanationText = form.explanation_text ?? null
    Object.freeze(this)
  }

  /**
   * The result's JSON form, as a test run document writes it, for
   * `JSON.stringify`; a copy.
   */
  toJSON() {
    return JSON.parse(this.#form)
  }

  [INSPECT]() {
    const where = [this.path, this.id === null ? null : JSON.stringify(this.id)].filter((part) => part !== null)
    return `CaseResult(${where.length === 0 ? '(a case file without a path)' : where.join(' ')}: ${this.status})`
  }

  [DENO_INSPECT]() {
    return this[INSPECT]()
  }
}

/**
 * A summary as the native layer gives it, the core's counts: `[passed,
 * failed, error, interrupted, total, partial]`.
 */
function testRunSummary([passed, failed, error, interrupted, total, partial]) {
  return Object.freeze({
    total,
    passed,
    failed,
    error,
    partial,
    interrupted: interrupted === null ? null : Object.freeze({ reason: interrupted[0], remaining: interrupted[1] }),
  })
}

/**
 * The results of a run and their summary (`spec/tests.md`, *Test runs*):
 * `RuleSet.test`, `DecisionTable.test`, `Bundle.test` and `CaseRun.testRun()`
 * make it.
 */
class TestRun {
  #document
  #text

  constructor(token, data) {
    check(token, 'TestRun')
    const [document, text, results, summary, allPassed, isCurrent] = JSON.parse(data)
    this.#document = JSON.stringify(document)
    this.#text = text
    const run = document.test_run
    /** What was tested: `"ruleset"` or `"decision_table"` (a later version may add sources). */
    this.source = run.source
    /** The hit policy: a rule set's, as the table policy of the same name, or a table's. */
    this.hitPolicy = run.hit_policy
    /** The `collect` aggregate, or `null`. */
    this.aggregate = run.aggregate ?? null
    /** The logic's name. */
    this.logic = run.logic
    /** Whether evaluation is strict. */
    this.strict = run.strict === true
    /** The rule set digest of the rules tested, when asked for (`rulesetDigest`); otherwise `null`. */
    this.rulesetDigest = run.ruleset_digest ?? null
    const limits = Object.fromEntries(
      Object.entries(run.engine.limits).map(([key, value]) => [ENGINE_LIMITS[key] ?? key, jsonInt(value)]),
    )
    /**
     * The engine that ran the cases (this one): `name`, `version`, the
     * `limits` its case files were read with (a bundle's own, for a
     * bundle's cases), by the binding's names, and `isCurrent`, whether it
     * is this engine (the same name and version), as a bundle's engine says.
     */
    this.engine = Object.freeze({ name: run.engine.name, version: run.engine.version, limits: Object.freeze(limits), isCurrent })
    /** The bundle's `id` and `revision`, for a bundle's cases; otherwise `null`. */
    this.bundle = run.bundle === undefined ? null : Object.freeze({ id: run.bundle.id, revision: run.bundle.revision })
    /** The case files given to the run, in order: each `{ path, digest }`, `null` for what it has not. */
    this.files = frozen(run.files.map((file) => Object.freeze({ path: file.path ?? null, digest: file.digest ?? null })))
    /** The limits it ran with: `maxExplained`, and `maxResults` and `maxSteps` (or `null`). */
    this.limits = Object.freeze({
      maxExplained: jsonInt(run.limits.max_explained),
      maxResults: run.limits.max_results === undefined ? null : jsonInt(run.limits.max_results),
      maxSteps: run.limits.max_steps === undefined ? null : jsonInt(run.limits.max_steps),
    })
    /**
     * The counts (`total`, `passed`, `failed`, `error`), `partial`, and
     * `interrupted` (`{ reason, remaining }`) for a partial run, or `null`.
     */
    this.summary = testRunSummary(summary)
    /** The results, in run order. */
    this.results = frozen(results.map((result) => new CaseResult(INTERNAL, result)))
    /** Whether every case ran and passed: no failure, no error, and not partial; true for a run of no cases. */
    this.allPassed = allPassed
    Object.freeze(this)
  }

  /**
   * The test run document of the JSON format (`spec/tests.md`, *Test
   * runs*), for `JSON.stringify` (`Expr.toJSON` says how); a copy. Values
   * are in the JSON encoding of values (a decimal is `{"$decimal": "0.15"}`).
   */
  toJSON() {
    return JSON.parse(this.#document)
  }

  /** The test run as text, the same in every language (`spec/tests.md`, *Text*). */
  toString() {
    return this.#text
  }

  [INSPECT]() {
    const what = this.source === 'ruleset' ? 'rule set' : 'decision table'
    const { passed, failed, error, partial } = this.summary
    return `TestRun(${what}: ${passed} passed, ${failed} failed, ${error} error${error === 1 ? '' : 's'}${partial ? ', partial' : ''})`
  }

  [DENO_INSPECT]() {
    return this[INSPECT]()
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
  AuditedRule,
  CheckResult,
  NotModelled,
  Undecided,
  Diagnostic,
  Audit,
  Mismatch,
  CaseResult,
  TestRun,
)

export {
  ActionContext,
  Audit,
  AuditedRule,
  CaseResult,
  Cause,
  CauseGroup,
  CheckResult,
  Counterfactual,
  Diagnostic,
  Explanation,
  FieldUsage,
  INTERNAL,
  Mismatch,
  Note,
  NotModelled,
  Reason,
  RuleExplanation,
  RuleResult,
  ResultTree,
  RuleSummary,
  Summary,
  TableOutput,
  TableRow,
  TestRun,
  Undecided,
  frozenData,
  notFound,
  testRunSummary,
}
