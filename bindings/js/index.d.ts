// Types of the `cleanrules` package (`index.js`): what is final today, the
// errors, the marker values and the versions. The native classes are in
// `cleanrules/native` (`native.d.ts`, over the `binding.d.ts` napi-rs writes).

import type { Decision } from './binding'

/** The version of the JSON format this build reads and writes. */
export declare const FORMAT_VERSION: number

/** The version of the package. */
export declare const version: string

/** An exact decimal, given by its literal text, such as `"0.10"`. */
export declare class Decimal {
  constructor(text: string)
  /** The literal, as given. */
  readonly text: string
  toString(): string
}

/** A float, for a `number` that would otherwise read as an int. */
export declare class Float {
  constructor(value: number)
  /** The number. */
  readonly value: number
  valueOf(): number
  toString(): string
}

/** A day of the proleptic Gregorian calendar, years 1 to 9999. */
export declare class LocalDate {
  constructor(year: number, month: number, day: number)
  readonly year: number
  readonly month: number
  readonly day: number
  toString(): string
}

/** A date and a time of day, to the microsecond, without a time zone. */
export declare class NaiveDateTime {
  constructor(
    year: number,
    month: number,
    day: number,
    hour?: number,
    minute?: number,
    second?: number,
    microsecond?: number,
  )
  readonly year: number
  readonly month: number
  readonly day: number
  readonly hour: number
  readonly minute: number
  readonly second: number
  readonly microsecond: number
  toString(): string
}

/** `decimal("0.10")`: an exact decimal (JavaScript has none). */
export declare function decimal(text: string): Decimal
/** `float(2)`: the number as a float, not an int. */
export declare function float(value: number): Float
/** `date(2026, 11, 4)`: a date (months count from 1). */
export declare function date(year: number, month: number, day: number): LocalDate
/** `datetime(2026, 11, 4, 9, 30)`: a naive datetime (no time zone). */
export declare function datetime(
  year: number,
  month: number,
  day: number,
  hour?: number,
  minute?: number,
  second?: number,
  microsecond?: number,
): NaiveDateTime

/** The context fields of an error (`spec/errors.md`, *Codes*); each only when it applies. */
export interface ErrorContext {
  /** A JSON Pointer into the document read. */
  readonly pointer?: string
  /** The 1-based line of a syntax error. */
  readonly line?: number
  /** The 1-based column, in code points. */
  readonly column?: number
  /** The rule concerned. */
  readonly rule?: string
  /** The rules concerned, in rule order. */
  readonly rules: readonly string[]
  /** The full path of the field being read. */
  readonly path?: ReadonlyArray<string | number>
  /** The type groups involved. */
  readonly types: readonly string[]
  /** The position of the input in a batch. */
  readonly row?: number
  /** The name looked up. */
  readonly name?: string
  /** The names that exist, sorted. */
  readonly available: readonly string[]
  /** The rules whose actions had run. */
  readonly executed: readonly string[]
  /** The failing node, in its JSON form. */
  readonly node?: unknown
  /** What was not supported. */
  readonly operation?: string
  /** Why it was not supported. */
  readonly reason?: string
}

export interface CleanRulesErrorOptions {
  /** The stable error code; defaults to the class's own. */
  code?: string
  /** How to fix it. */
  hint?: string | null
  /** The context fields that apply. */
  context?: Partial<ErrorContext>
  /** The error or value that caused it. */
  cause?: unknown
  /** For an `ActionError`, the decision whose actions were running. */
  decision?: Decision | null
}

/** The base class of every error raised by cleanrules. */
export declare class CleanRulesError extends Error {
  constructor(message: string, options?: CleanRulesErrorOptions)
  /** The code of the class (empty for the base class). */
  static readonly defaultCode: string
  /** The stable error code, such as `"missing_field"`. */
  readonly code: string
  /** How to fix it, or `null`. */
  readonly hint: string | null
  /** The context fields that apply. */
  readonly context: ErrorContext
  /** For an `ActionError`, the decision whose actions were running; otherwise `null`. */
  readonly decision: Decision | null
  /** The error's JSON form (`spec/errors.md`, *JSON form*). */
  toJSON(): Record<string, unknown>
}

/** A document is invalid (`format_error`, `unsupported_version`, `limit_exceeded`, `decision_mismatch`). */
export declare class FormatError extends CleanRulesError {}
/** A condition node breaks a construction rule (`expression_error`). */
export declare class ExpressionError extends CleanRulesError {}
/** A rule or rule set breaks a construction rule (`rule_definition_error`). */
export declare class RuleDefinitionError extends CleanRulesError {}
/** The text syntax cannot be parsed (`rule_syntax_error`). */
export declare class RuleSyntaxError extends CleanRulesError {}
/** A name is not registered (`registry_error`). */
export declare class RegistryError extends CleanRulesError {}
/** A rule set has no rule by that name (`rule_not_found`). */
export declare class RuleNotFoundError extends CleanRulesError {}
/** Strict evaluation cannot go on (`evaluation_error` and its children). */
export declare class EvaluationError extends CleanRulesError {}
/** Strict mode met a field that cannot be resolved (`missing_field`). */
export declare class MissingFieldError extends EvaluationError {}
/** Strict mode met values of incompatible types (`type_mismatch`). */
export declare class TypeMismatchError extends EvaluationError {}
/** The hit policy cannot decide (`hit_policy_error`). */
export declare class HitPolicyError extends EvaluationError {}
/** An action threw while running a decision (`action_error`). */
export declare class ActionError extends CleanRulesError {}
/** This build, logic or format version cannot do the operation (`unsupported`). */
export declare class UnsupportedError extends CleanRulesError {}
