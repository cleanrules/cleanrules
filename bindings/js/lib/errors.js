'use strict'

// The errors of cleanrules (spec/errors.md). Every error derives from
// `CleanRulesError`, so one `instanceof CleanRulesError` catches them all.
// Each has a stable `code`, the same in every language, a `message` written by
// the core, a `hint` on how to fix the problem (or `null`), the `context`
// fields that apply (`rule`, `path`, `pointer`, ...: an object, because an
// error's `name` is its class) and, when another error caused it, a `cause`.
//
//   try {
//     ruleset.evaluate(data)
//   } catch (error) {
//     if (error instanceof MissingFieldError) console.log(error.context.path)
//   }

/** The context fields of an error, in the order of its JSON form. */
const CONTEXT_FIELDS = [
  'pointer',
  'line',
  'column',
  'rule',
  'rules',
  'path',
  'types',
  'row',
  'name',
  'available',
  'executed',
  'node',
  'operation',
  'reason',
]

/** The base class of every error raised by cleanrules. */
class CleanRulesError extends Error {
  /**
   * @param {string} message What went wrong.
   * @param {object} [options] `code` (defaults to the class's), `hint`,
   *   `context` (the context fields that apply), `cause`, and `decision` for
   *   an `ActionError`.
   */
  constructor(message, options = {}) {
    super(message, 'cause' in options ? { cause: options.cause } : undefined)
    const { code, hint = null, context = {}, decision = null, json = null } = options
    const unknown = Object.keys(context).filter((field) => !CONTEXT_FIELDS.includes(field))
    if (unknown.length > 0) {
      throw new TypeError(`unknown error context fields: ${unknown.sort().join(', ')}`)
    }
    /** The stable error code, such as `"missing_field"`. */
    this.code = code ?? new.target.defaultCode
    /** How to fix it, or `null`. */
    this.hint = hint
    /** The context fields that apply (`spec/errors.md`, *Codes*). */
    this.context = Object.freeze({ rules: [], types: [], available: [], executed: [], ...context })
    /** For an `ActionError`, the decision whose actions were running. */
    this.decision = decision
    Object.defineProperty(this, '_json', { value: json, enumerable: false })
  }

  /** The error's JSON form (`spec/errors.md`, *JSON form*). */
  toJSON() {
    if (this._json !== null) {
      return JSON.parse(this._json)
    }
    const form = { code: this.code, message: this.message }
    if (this.hint !== null) {
      form.hint = this.hint
    }
    for (const field of CONTEXT_FIELDS) {
      const value = this.context[field]
      if (value !== undefined && !(Array.isArray(value) && value.length === 0)) {
        form[field] = value
      }
    }
    if (this.cause !== undefined) {
      form.cause = this.cause instanceof Error ? this.cause.message : String(this.cause)
    }
    return form
  }
}
CleanRulesError.defaultCode = ''

function define(name, parent, defaultCode) {
  const Class = {
    [name]: class extends parent {},
  }[name]
  Class.defaultCode = defaultCode
  Object.defineProperty(Class.prototype, 'name', { value: name, writable: true, configurable: true })
  return Class
}

Object.defineProperty(CleanRulesError.prototype, 'name', {
  value: 'CleanRulesError',
  writable: true,
  configurable: true,
})

/** A document is invalid (`format_error`, `unsupported_version`, `limit_exceeded`, `decision_mismatch`). */
const FormatError = define('FormatError', CleanRulesError, 'format_error')
/** A condition node breaks a construction rule (`expression_error`). */
const ExpressionError = define('ExpressionError', CleanRulesError, 'expression_error')
/** A rule or rule set breaks a construction rule (`rule_definition_error`). */
const RuleDefinitionError = define('RuleDefinitionError', CleanRulesError, 'rule_definition_error')
/** The text syntax cannot be parsed (`rule_syntax_error`). */
const RuleSyntaxError = define('RuleSyntaxError', CleanRulesError, 'rule_syntax_error')
/** A name is not registered (`registry_error`). */
const RegistryError = define('RegistryError', CleanRulesError, 'registry_error')
/** A rule set has no rule by that name (`rule_not_found`). */
const RuleNotFoundError = define('RuleNotFoundError', CleanRulesError, 'rule_not_found')
/** Strict evaluation cannot go on (`evaluation_error` and its children). */
const EvaluationError = define('EvaluationError', CleanRulesError, 'evaluation_error')
/** Strict mode met a field that cannot be resolved (`missing_field`). */
const MissingFieldError = define('MissingFieldError', EvaluationError, 'missing_field')
/** Strict mode met values of incompatible types (`type_mismatch`). */
const TypeMismatchError = define('TypeMismatchError', EvaluationError, 'type_mismatch')
/** The hit policy cannot decide (`hit_policy_error`). */
const HitPolicyError = define('HitPolicyError', EvaluationError, 'hit_policy_error')
/** An action threw while running a decision (`action_error`). */
const ActionError = define('ActionError', CleanRulesError, 'action_error')
/** This build, logic or format version cannot do the operation (`unsupported`). */
const UnsupportedError = define('UnsupportedError', CleanRulesError, 'unsupported')

/** The class of each code (`spec/errors.md`, *Hierarchy*). */
const CLASSES = {
  format_error: FormatError,
  unsupported_version: FormatError,
  limit_exceeded: FormatError,
  decision_mismatch: FormatError,
  expression_error: ExpressionError,
  rule_definition_error: RuleDefinitionError,
  rule_syntax_error: RuleSyntaxError,
  registry_error: RegistryError,
  rule_not_found: RuleNotFoundError,
  evaluation_error: EvaluationError,
  missing_field: MissingFieldError,
  type_mismatch: TypeMismatchError,
  hit_policy_error: HitPolicyError,
  action_error: ActionError,
  unsupported: UnsupportedError,
}

/**
 * The error of a core error's JSON form, for the native layer: `cause` is a
 * JavaScript value that caused it (or `null`), `decision` the decision of an
 * `ActionError` (or `null`).
 */
function fromNative(json, cause, decision) {
  const form = JSON.parse(json)
  const { code, message, hint = null } = form
  // Fields a newer core adds stay in the JSON form (`toJSON()`).
  const context = {}
  for (const field of CONTEXT_FIELDS) {
    if (field in form) {
      context[field] = form[field]
    }
  }
  // A new code of a newer core is still an error.
  const Class = Object.hasOwn(CLASSES, code) ? CLASSES[code] : CleanRulesError
  const options = { code, hint, context, decision: decision ?? null, json }
  if (cause !== null && cause !== undefined) {
    options.cause = cause
  }
  return new Class(message, options)
}

module.exports = {
  ActionError,
  CleanRulesError,
  EvaluationError,
  ExpressionError,
  FormatError,
  HitPolicyError,
  MissingFieldError,
  RegistryError,
  RuleDefinitionError,
  RuleNotFoundError,
  RuleSyntaxError,
  TypeMismatchError,
  UnsupportedError,
  fromNative,
}
