'use strict'

// cleanrules for Node: what is final today, the errors, the marker values and
// the versions. The native classes are in `cleanrules/native` (`native.js`);
// the idiomatic TypeScript API arrives here in phase F.2.

const native = require('./lib/load.js')
const errors = require('./lib/errors.js')
const values = require('./lib/values.js')

// One assignment per name, so that `import { MissingFieldError } from
// 'cleanrules'` finds each (Node detects the names of a CommonJS module by
// these assignments).
exports.FORMAT_VERSION = native.FORMAT_VERSION
exports.version = native.version()
// Values JavaScript has no type for.
exports.Decimal = values.Decimal
exports.Float = values.Float
exports.LocalDate = values.LocalDate
exports.NaiveDateTime = values.NaiveDateTime
exports.decimal = values.decimal
exports.float = values.float
exports.date = values.date
exports.datetime = values.datetime
// Errors.
exports.CleanRulesError = errors.CleanRulesError
exports.FormatError = errors.FormatError
exports.ExpressionError = errors.ExpressionError
exports.RuleDefinitionError = errors.RuleDefinitionError
exports.RuleSyntaxError = errors.RuleSyntaxError
exports.RegistryError = errors.RegistryError
exports.RuleNotFoundError = errors.RuleNotFoundError
exports.EvaluationError = errors.EvaluationError
exports.MissingFieldError = errors.MissingFieldError
exports.TypeMismatchError = errors.TypeMismatchError
exports.HitPolicyError = errors.HitPolicyError
exports.ActionError = errors.ActionError
exports.UnsupportedError = errors.UnsupportedError
