// What the root entry exports besides the versions and the API of its build
// (`lib/api/index.js`), the same in every build: the errors, the marker
// values, and the classes of what decisions, explanations, summaries, audits,
// result trees and decision tables give back.

export {
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
} from './errors.js'
export {
  ActionContext,
  Audit,
  AuditedRule,
  Cause,
  CauseGroup,
  CheckResult,
  Counterfactual,
  Diagnostic,
  Explanation,
  FieldUsage,
  Note,
  NotModelled,
  Reason,
  ResultTree,
  RuleExplanation,
  RuleResult,
  RuleSummary,
  Summary,
  TableOutput,
  TableRow,
  Undecided,
} from './api/results.js'
export {
  Decimal,
  Float,
  LocalDate,
  NaiveDateTime,
  ZonedDateTime,
  date,
  datetime,
  decimal,
  float,
} from './values.js'
