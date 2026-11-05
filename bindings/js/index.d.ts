// Types of the `cleanrules` package (`index.js`, `index.wasm.js`, `index.deno.js`):
// the idiomatic API (conditions, rules, rule sets, decisions, explanations,
// summaries, result trees, decision tables and documents), the errors, the
// marker values and the versions. The native
// classes are in `cleanrules/native` (`native.d.ts`, over the `binding.d.ts`
// napi-rs writes).

import type { Decision as NativeDecision, Limits } from './binding.js'

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

/**
 * An aware datetime as the engine gives it back: an instant, to the
 * microsecond, with its local date and time in its IANA time zone or at a
 * fixed offset. The same in every runtime; it reads back as the same value.
 * Only the engine makes them: as input, pass a `Date` or a
 * `Temporal.ZonedDateTime`.
 */
export declare class ZonedDateTime {
  private constructor()
  readonly year: number
  /** From 1. */
  readonly month: number
  readonly day: number
  readonly hour: number
  readonly minute: number
  readonly second: number
  readonly microsecond: number
  /** The offset from UTC at this instant, in seconds. */
  readonly offsetSeconds: number
  /** The IANA time zone, such as `"Europe/Madrid"`, or `null` for a fixed offset. */
  readonly timeZone: string | null
  /** The instant, in microseconds since 1970-01-01T00:00:00Z. */
  readonly epochMicroseconds: bigint
  /** The instant as a `Date` (to the millisecond, rounded toward the past). */
  toDate(): Date
  /**
   * The same instant as a `Temporal.ZonedDateTime`, in its IANA zone or at its
   * offset; in a named zone its local fields follow the runtime's zone
   * database. A `TypeError` where the runtime has no `Temporal`, a
   * `RangeError` for an offset with seconds (Temporal's offset zones have
   * whole minutes). Typed `Temporal.ZonedDateTime` where TypeScript's
   * libraries declare Temporal, `unknown` otherwise.
   */
  toTemporal(): TemporalZonedDateTime
  /** RFC 9557, as the JSON format writes it: `2026-10-31T09:30:00+01:00[Europe/Madrid]`. */
  toString(): string
  /** `toString()`, for `JSON.stringify`. */
  toJSON(): string
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
  decision?: Decision | NativeDecision | null
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
  /**
   * For an `ActionError`, the decision whose actions were running: a
   * `Decision` from `RuleSet.run`, or a native one from the native layer's
   * `run`; otherwise `null`.
   */
  readonly decision: Decision | NativeDecision | null
  /**
   * The error's JSON form (`spec/errors.md`, *JSON form*), as an object for
   * `JSON.stringify`: a float that holds an integer is `{"$float": n}`, so the
   * text reads back as the same form, except that a float zero loses its sign
   * (`JSON.stringify(-0)` is `0`) and integer-like keys, such as a payload key
   * `"10"`, come first, as in any plain object. Its `node`, if any, is exact
   * in `dumps` of the node.
   */
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

// --- Values --------------------------------------------------------------

/** `Temporal.ZonedDateTime` where the TypeScript libraries in use declare Temporal; otherwise `unknown`. */
export type TemporalZonedDateTime = typeof globalThis extends {
  Temporal: { ZonedDateTime: abstract new (...args: never[]) => infer Zoned }
}
  ? Zoned
  : unknown

/** A Temporal value (`Temporal.PlainDate`, `Temporal.ZonedDateTime`...), or a polyfill's. */
export interface TemporalValue {
  readonly [Symbol.toStringTag]: `Temporal.${string}`
}

/**
 * A constant (`spec/data-model.md`, *Constants*): JavaScript values next to a
 * node become constants by themselves (`F.x.gt(5)`).
 */
export type Constant =
  | null
  | boolean
  | number
  | bigint
  | string
  | Decimal
  | Float
  | LocalDate
  | NaiveDateTime
  | ZonedDateTime
  | Date
  | TemporalValue

/**
 * An opaque value given back: the very object the engine read from the input
 * (or `null` in a recorded decision, which keeps only its type's name).
 */
export type Opaque = object

/** A value given back (`spec/data-model.md`, *Values given back*). */
export type Value =
  | null
  | boolean
  | number
  | bigint
  | string
  | Decimal
  | LocalDate
  | NaiveDateTime
  | ZonedDateTime
  | Value[]
  | { [key: string]: Value }
  | Opaque

/** A payload or an output: keys with their values, frozen. */
export type Output = Readonly<Record<string, Value>>

/** A field path: keys (strings) and indices (numbers), outermost first. */
export type FieldPath = ReadonlyArray<string | number>

// --- Names ---------------------------------------------------------------

/** How a rule set turns the rules that fired into a decision (`spec/rules.md`). */
export type HitPolicy = 'first' | 'priority' | 'unique' | 'any' | 'rule_order' | 'collect'
/** How `collect` combines the outputs of the rules that fired. */
export type Aggregate = 'sum' | 'min' | 'max' | 'count'
/** A truth value of Kleene's three-valued logic. */
export type Truth = 'true' | 'false' | 'unknown'
/** What happened to a rule in a decision (`spec/explain.md`). */
export type Status = 'selected' | 'fired' | 'not_fired' | 'unknown'
/** Where a node may appear: as a condition, as a value, or as either. */
export type Kind = 'condition' | 'value' | 'any'
/** Which ends of `between` are included. */
export type Closed = 'both' | 'left' | 'right' | 'none'
/** A comparison's tag. */
export type CompareOp = 'eq' | 'ne' | 'lt' | 'le' | 'gt' | 'ge'
/** Why a rule that fired was not selected. */
export type NoteKind = 'lower_priority' | 'same_priority' | 'any_agrees' | 'earlier_rule'
/** Whether each item of a group matters (`all`), or one is enough (`any`). */
export type GroupKind = 'all' | 'any'
/** A node's tag (`spec/ast.md`, *Catalogue*). */
export type Tag =
  | 'field'
  | 'const'
  | CompareOp
  | 'and'
  | 'or'
  | 'not'
  | 'in'
  | 'between'
  | 'is_null'
  | 'matches'
  | 'starts_with'
  | 'ends_with'
  | 'contains'
  | 'add'
  | 'sub'
  | 'mul'
  | 'div'
  | 'abs'
  | 'neg'
/** Why a result is unknown or null (`spec/ast.md`, *Results and reasons*). */
export type ReasonCode =
  | 'missing_field'
  | 'null'
  | 'type_mismatch'
  | 'unorderable'
  | 'division_by_zero'
  | 'int_overflow'
  | 'decimal_overflow'
  | 'nan_result'
  | 'read_error'

// --- Conditions ----------------------------------------------------------

/** An operand of a comparison or of arithmetic: a node or a constant. */
export type Operand = Expr | Constant

/**
 * A node of a condition tree, immutable (`spec/ast.md`). Build nodes with
 * `F`, `field`, `constant`, `and`, `or`, `not` and these methods; the core
 * checks each one as it is built (`ExpressionError`).
 */
export declare class Expr {
  protected constructor()
  /** The node's tag, such as `"gt"` or `"and"` (`spec/ast.md`, *Catalogue*). */
  readonly tag: Tag
  /** Whether the node is a condition, a value, or either. */
  readonly kind: Kind
  /** The direct child nodes, in order. */
  children(): readonly Expr[]
  /** This node and all its descendants, in pre-order (parents first). */
  walk(): Generator<Expr, void, undefined>
  /** The paths of every field the node reads, in order of first appearance (frozen). */
  fieldPaths(): readonly FieldPath[]
  /** Whether `other` is a node with the same structure, operators and constants. */
  sameAs(other: unknown): boolean
  /** The canonical text: `customer.age >= 18 AND NOT blocked`. */
  toString(): string
  /**
   * The node in the JSON format (`spec/json-format.md`, *Conditions*), as an
   * object for `JSON.stringify`: a float that holds an integer is `{"$float":
   * n}`, so the text reads back as the same node, except that a float zero
   * loses its sign (`JSON.stringify(-0)` is `0`) and integer-like keys, such
   * as a payload key `"10"`, come first, as in any plain object. `dumps(node)`
   * writes it exactly.
   */
  toJSON(): Record<string, unknown>
  /** `this = other`; comparing with `null` is refused (use `isNull()`). */
  eq(other: Operand): Expr
  /** `this != other`. */
  ne(other: Operand): Expr
  /** `this < other`. */
  lt(other: Operand): Expr
  /** `this <= other`. */
  le(other: Operand): Expr
  /** `this > other`. */
  gt(other: Operand): Expr
  /** `this >= other`. */
  ge(other: Operand): Expr
  /** `this AND others...` (fuzzytool's `&`). */
  and(...others: Expr[]): Expr
  /** `this OR others...` (fuzzytool's `|`). */
  or(...others: Expr[]): Expr
  /** `NOT this` (fuzzytool's `~`). */
  not(): Expr
  /** `this + other`. */
  add(other: Operand): Expr
  /** `this - other`. */
  sub(other: Operand): Expr
  /** `this * other`. */
  mul(other: Operand): Expr
  /** `this / other`; a division by zero is unknown. */
  div(other: Operand): Expr
  /** `-this`. */
  neg(): Expr
  /** The absolute value. */
  abs(): Expr
  /** Membership in constants of one type group: `F.country.isIn(['ES', 'PT'])` (an iterable object, never a string). */
  isIn(values: Iterable<Constant> & object): Expr
  /** A range test, both ends included unless `closed` says otherwise. */
  between(low: Operand, high: Operand, options?: { closed?: Closed }): Expr
  /** Whether the value is missing or null; never unknown. */
  isNull(): Expr
  /** A regular expression search (the Rust `regex` syntax). */
  matches(pattern: string): Expr
  startsWith(prefix: string): Expr
  endsWith(suffix: string): Expr
  /** Whether a string contains the literal `part`. */
  contains(part: string): Expr
}

/** A value looked up in the input by path. */
export declare class Field extends Expr {
  /** The keys and indices of the path, outermost first. */
  readonly path: FieldPath
  /** The field one or more segments further: `.at(0)` for an index, `.at('and')` for a key named like a method. */
  at(...segments: Array<string | number>): FieldRef
}

/** A constant. */
export declare class Const extends Expr {
  /** The constant, as a value given back. */
  readonly value: Value
}

/**
 * A field whose properties are fields one key further (`F.customer.age`),
 * except its own members; `then` is never a field, and an index is refused
 * (use `.at(i)`).
 */
export type FieldRef = Field &
  { readonly then: undefined } & { readonly [index: number]: never } & { readonly [name: string]: FieldRef }

/** The type of `F`: every property is a field, `then` excepted; an index is refused. */
export type FieldRoot = { readonly then: undefined } & { readonly [index: number]: never } & {
  readonly [name: string]: FieldRef
}

/**
 * The start of every field path. A property is a key; use `field(...)` or
 * `.at(...)` for indices and for keys named like a method. Under
 * `noUncheckedIndexedAccess`, TypeScript makes every key of an index
 * signature possibly `undefined`: type `F` after the input (`Fields<T>`) or
 * use `field(...)`.
 *
 * @example
 * F.customer.age.ge(18).and(F.country.isIn(['ES', 'PT']))
 * F.items.at(0).price.gt(10)
 */
export declare const F: FieldRoot

/** What a typed field path stops at: a value with no fields of its own. */
type FieldLeaf =
  | string
  | number
  | bigint
  | boolean
  | symbol
  | Date
  | Decimal
  | Float
  | LocalDate
  | NaiveDateTime
  | ZonedDateTime
  | TemporalValue

/** A field of a typed path: its members, and only its typed keys (`Fields<T>`). */
export type TypedField<T> = FieldsOf<NonNullable<T>> &
  Field & { readonly then: undefined } & { readonly [index: number]: never }

/** The fields of a value of type `T`: its keys, or for an array, its elements through `.at(i)`. */
export type FieldsOf<T> = T extends FieldLeaf
  ? unknown
  : T extends readonly (infer Element)[]
    ? { at(index: number): TypedField<Element> }
    : { readonly [K in keyof T & string]-?: TypedField<T[K]> }

/**
 * `F` typed after the shape of the input, for completion and checked keys,
 * with nothing possibly `undefined` (types only: at run time it is `F`).
 * Keys below the first are checked; an index goes through `.at(i)`.
 *
 * @example
 * interface Order { customer: { years: number }; items: { price: number }[] }
 * const O = F as Fields<Order>
 * O.customer.years.gt(5).and(O.items.at(0).price.gt(10))
 */
export type Fields<T> = FieldsOf<T> & FieldRoot

/** A field by its path: `field('items', 0, 'price')`. */
export declare function field(...path: Array<string | number>): FieldRef
/** A constant, where no node is involved: `constant(true)`. */
export declare function constant(value: Constant): Const
/** `a AND b AND ...`: at least two conditions. */
export declare function and(...operands: Expr[]): Expr
/** `a OR b OR ...`: at least two conditions. */
export declare function or(...operands: Expr[]): Expr
/** `NOT x`. */
export declare function not(operand: Expr): Expr

// --- Rules ---------------------------------------------------------------

/** An action: called by `RuleSet.run` when its rule is selected; synchronous, its result ignored. */
export type Action = (context: ActionContext) => void

/** A rule's payload: keys with nodes or constants, in order. */
export type Payload = Readonly<Record<string, Operand>> | ReadonlyMap<string, Operand>

/** Data for a rule's metadata: null, booleans, numbers, strings, dates, arrays and objects. */
export type Metadata = Readonly<Record<string, unknown>> | ReadonlyMap<string, unknown>

export interface RuleOptions {
  /** Used by the `priority` hit policy; higher wins. 0 by default. */
  priority?: number | bigint
  /** Labels; kept sorted and without repeats. */
  tags?: Iterable<string>
  description?: string | null
  /** Free-form data (owner, ticket...). */
  metadata?: Metadata | null
  /** The action: a function, or the name of one that `RuleSet.bind` resolves. */
  do?: Action | string | null
}

/**
 * A named condition and its payload; immutable. The third argument is the
 * payload: options such as `priority` go in the fourth.
 *
 * @example
 * new Rule('vip', F.customer.years.gt(5), { discount: decimal('0.15') }, { priority: 10, do: 'notify' })
 */
export declare class Rule {
  constructor(name: string, when: Expr, then?: Payload | null, options?: RuleOptions)
  /** The rule's name, unique within a rule set; decisions and errors name the rule by it. */
  readonly name: string
  readonly when: Expr
  /** The payload, in order: each key with its node. */
  readonly then: Readonly<Record<string, Expr>>
  readonly priority: number | bigint
  /** Sorted, without repeats. */
  readonly tags: readonly string[]
  readonly description: string | null
  /** As the core keeps it. */
  readonly metadata: Output
  /** The action's code: given, or bound to its name; `null` if none. */
  readonly do: Action | null
  /** The name of the rule's action, if it names one; otherwise `null`. */
  readonly actionName: string | null
  /**
   * The rule document of the JSON format (`{ version, rule }`), as an object
   * for `JSON.stringify`: a float that holds an integer is `{"$float": n}`, so
   * the text reads back as the same rule, except that a float zero loses its
   * sign (`JSON.stringify(-0)` is `0`) and integer-like keys, such as a
   * payload key `"10"`, come first, as in any plain object. `dumps(rule)`
   * writes it exactly. An `UnsupportedError` for an action that is code
   * without a name.
   */
  toJSON(): Record<string, unknown>
}

export interface RuleSetOptions {
  /** `'first'` by default; names in any case. */
  hitPolicy?: HitPolicy | Uppercase<HitPolicy>
  /** With `'collect'`. */
  aggregate?: Aggregate | Uppercase<Aggregate> | null
  /** `'boolean'` (Kleene's three-valued logic) by default. */
  logic?: string
  /** Throw instead of giving unknown for missing fields and type mismatches. */
  strict?: boolean
}

/** How a recorded decision is read. */
export interface ReadDecisionOptions {
  format?: 'json' | 'yaml' | null
  limits?: Limits | null
}

/**
 * Rules evaluated together under a hit policy; immutable, iterable and indexed by name.
 *
 * @example
 * const rules = new RuleSet([vip, loyal], { hitPolicy: 'priority' })
 * rules.evaluate({ customer: { years: 7 } }).output
 */
export declare class RuleSet implements Iterable<Rule> {
  constructor(rules: Iterable<Rule>, options?: RuleSetOptions)
  readonly rules: readonly Rule[]
  /** How many rules there are (a rule set is a sequence, as in Python). */
  readonly length: number
  [Symbol.iterator](): Iterator<Rule>
  /** The rule named `name`, or `undefined` (like `Map.prototype.get`). */
  get(name: string): Rule | undefined
  /** The rule named `name`; a `RuleNotFoundError` listing the rules when there is none. */
  rule(name: string): Rule
  has(name: string): boolean
  readonly hitPolicy: HitPolicy
  readonly aggregate: Aggregate | null
  readonly logic: string
  readonly strict: boolean
  /** The field paths the rules read, frozen; native data is read along these only. */
  fieldPaths(): readonly FieldPath[]
  /**
   * Evaluate the rule set on native data (`spec/data-model.md`, *JavaScript
   * and TypeScript*), read along `fieldPaths()` only. In strict mode, an
   * `EvaluationError` for a missing field or a type mismatch.
   */
  evaluate(data: unknown): Decision
  evaluateMany(rows: Iterable<unknown>): Decision[]
  /** A decision this rule set made, recorded as JSON or YAML. */
  readDecision(text: string | Uint8Array, options?: ReadDecisionOptions | null): Decision
  /** This rule set with its named actions resolved in `actions`. */
  bind(actions: Readonly<Record<string, Action>> | ReadonlyMap<string, Action>): RuleSet
  /** Evaluate, then call the selected rules' actions in rule order. */
  run(data: unknown): Decision
  summary(): Summary
  /**
   * The rule set document of the JSON format (`{ version, ruleset }`), as an
   * object for `JSON.stringify`: a float that holds an integer is `{"$float":
   * n}`, so the text reads back as the same rule set, except that a float zero
   * loses its sign (`JSON.stringify(-0)` is `0`) and integer-like keys, such
   * as a payload key `"10"`, come first, as in any plain object.
   * `dumps(ruleset)` writes it exactly. An `UnsupportedError` for an action
   * that is code without a name.
   */
  toJSON(): Record<string, unknown>
}

/** One rule's result in a decision. */
export declare class RuleResult {
  private constructor()
  readonly rule: Rule
  readonly fired: boolean
  readonly truth: Truth
  /** Its evaluated payload, when the hit policy needed it; otherwise `null`. */
  readonly output: Output | null
}

/** What an action receives (`spec/rules.md`, *Actions*). */
export declare class ActionContext {
  private constructor()
  readonly rule: Rule
  /** The input, as given to `run`. */
  readonly data: unknown
  readonly result: RuleResult
  /** The result tree of the rule's condition: each node's truth or value, and why it is unknown. */
  readonly tree: ResultTree
  /** The rule's evaluated payload (empty when it has none). */
  readonly output: Output
  readonly decision: Decision
}

/**
 * The outcome of evaluating a rule set on one input; immutable.
 *
 * @example
 * const decision = rules.evaluate(order)
 * decision.output?.discount
 * decision.whyNot('vip').causes
 */
export declare class Decision {
  private constructor()
  readonly ruleset: RuleSet
  /** Every rule's result, in rule order. */
  readonly results: readonly RuleResult[]
  /** The results the hit policy selected, in rule order. */
  readonly selected: readonly RuleResult[]
  readonly hitPolicy: HitPolicy
  /** The names of the rules that fired, in rule order. */
  readonly fired: readonly string[]
  /** The payloads of the selected rules, in rule order. */
  readonly outputs: readonly Output[]
  /**
   * The winner's payload or the aggregate; `null` when nothing was selected.
   * Reading it throws a `HitPolicyError` for `rule_order`, and `collect`
   * without an aggregate.
   */
  readonly output: Output | null
  /** With `collect` and an aggregate, its values; otherwise `null`. */
  readonly aggregated: Output | null
  explain(): Explanation
  /** One rule's explanation; a `RuleNotFoundError` for an unknown name. */
  whyNot(name: string): RuleExplanation
  /**
   * The decision document of the JSON format, as an object for
   * `JSON.stringify`: a float that holds an integer is `{"$float": n}`, so the
   * text reads back as the same decision (`RuleSet.readDecision` reads it),
   * except that a float zero loses its sign (`JSON.stringify(-0)` is `0`) and
   * integer-like keys, such as a payload key `"10"`, come first, as in any
   * plain object. The native layer's `Decision.write()` (`cleanrules/native`)
   * writes it exactly.
   */
  toJSON(): Record<string, unknown>
}

// --- Explanations and summaries ------------------------------------------

/** Why a result is unknown or null. */
export declare class Reason {
  private constructor()
  readonly code: ReasonCode
  readonly paths: readonly FieldPath[]
  readonly types: readonly string[]
  readonly detail: string | null
}

/**
 * A counterfactual's gap: a number, bigint or `Decimal` between numbers, a
 * frozen `{ days }` between dates and `{ microseconds }` between datetimes.
 */
export type Gap =
  | number
  | bigint
  | Decimal
  | { readonly days: number | bigint }
  | { readonly microseconds: number | bigint }

/** What a comparison needed: `path op target`, and how far the value was. */
export declare class Counterfactual {
  private constructor()
  readonly path: FieldPath
  readonly op: CompareOp
  readonly target: Value
  readonly gap: Gap | null
}

/** A condition in a rule's requirement, with what was read. */
export declare class Cause {
  private constructor()
  readonly node: Expr
  readonly truth: Truth
  /** Whether the rule needs it false. */
  readonly negated: boolean
  /** Whether it went the way the rule needed. */
  readonly holds: boolean
  /** The fields it read, with their values, in order. */
  readonly values: ReadonlyArray<readonly [FieldPath, Value]>
  readonly reason: Reason | null
  readonly counterfactual: Counterfactual | null
  /** Its line in the explanation. */
  toString(): string
}

/** Requirements that must all hold, or any of which suffices. */
export declare class CauseGroup {
  private constructor()
  readonly kind: GroupKind
  readonly items: ReadonlyArray<Cause | CauseGroup>
}

/** Why a rule that fired was not selected. */
export declare class Note {
  private constructor()
  readonly kind: NoteKind
  readonly winner: string
  readonly priority: number | bigint | null
  readonly winnerPriority: number | bigint | null
}

/** The explanation of one rule. */
export declare class RuleExplanation {
  private constructor()
  readonly rule: string
  readonly status: Status
  readonly requirement: Cause | CauseGroup
  readonly note: Note | null
  readonly achievable: boolean | null
  /** The causes of the requirement, in order, without their grouping. */
  readonly causes: readonly Cause[]
  /** The rule's part of the explanation's text. */
  toString(): string
}

/** The explanation of a decision (`spec/explain.md`). */
export declare class Explanation {
  private constructor()
  readonly rules: readonly RuleExplanation[]
  /** The names of the selected rules. */
  readonly selected: readonly string[]
  /** One rule's explanation; a `RuleNotFoundError` for an unknown name. */
  forRule(name: string): RuleExplanation
  /**
   * The explanation document of the JSON format, as an object for
   * `JSON.stringify`: a float that holds an integer is `{"$float": n}`, so the
   * text reads back as the same explanation, except that a float zero loses
   * its sign (`JSON.stringify(-0)` is `0`) and integer-like keys, such as a
   * payload key `"10"`, come first, as in any plain object. The native layer's
   * `Decision.explain('json')` writes it exactly.
   */
  toJSON(): Record<string, unknown>
  /** The explanation as text, the same in every language. */
  toString(): string
}

/** A rule in a summary. */
export declare class RuleSummary {
  private constructor()
  readonly name: string
  readonly priority: number | bigint
  readonly outputs: readonly string[]
  /** Whether the rule has an action (code, or a name). */
  readonly action: boolean
  readonly actionName: string | null
}

/** A field a rule set reads, and where. */
export declare class FieldUsage {
  private constructor()
  readonly path: FieldPath
  readonly conditions: readonly string[]
  /** The `[rule, key]` payload values that read it. */
  readonly payloads: ReadonlyArray<readonly [string, string]>
}

/** What a rule set reads and produces, without evaluating it. */
export declare class Summary {
  private constructor()
  readonly hitPolicy: HitPolicy
  readonly aggregate: Aggregate | null
  readonly logic: string
  readonly strict: boolean
  readonly rules: readonly RuleSummary[]
  readonly fields: readonly FieldUsage[]
  readonly outputs: readonly string[]
  /** Each tag with the names of its rules. */
  readonly tags: Readonly<Record<string, readonly string[]>>
  /**
   * The summary document of the JSON format, as an object for
   * `JSON.stringify`: a float that holds an integer is `{"$float": n}`, so the
   * text reads back as the same summary, except that a float zero loses its
   * sign (`JSON.stringify(-0)` is `0`) and integer-like keys, such as a
   * payload key `"10"`, come first, as in any plain object. Summaries hold no
   * floats.
   */
  toJSON(): Record<string, unknown>
  /** The summary as text, the same in every language. */
  toString(): string
}

// --- Result trees --------------------------------------------------------

/**
 * The result of evaluating a node and each of its children (`spec/ast.md`,
 * *Evaluating one node*).
 */
export declare class ResultTree {
  private constructor()
  /** The node evaluated (a `between` has its two half comparisons as children). */
  readonly node: Expr
  /** Its truth value, for a condition; `null` for a value. */
  readonly truth: Truth | null
  /** Its value, for a value node (`null` when it is null); `null` for a condition. */
  readonly value: Value
  /** Why it is unknown or null, if it is. */
  readonly reason: Reason | null
  /** The children's results, in order. */
  readonly children: readonly ResultTree[]
  /**
   * The result tree in the JSON format (`spec/json-format.md`, *Result
   * trees*), as an object for `JSON.stringify`: a float that holds an integer
   * is `{"$float": n}`, so the text reads back as the same tree, except that a
   * float zero loses its sign (`JSON.stringify(-0)` is `0`) and integer-like
   * keys, such as a payload key `"10"`, come first, as in any plain object.
   * The native layer's `Node.evaluateTree` writes it exactly.
   */
  toJSON(): Record<string, unknown>
}

export interface EvaluateTreeOptions {
  /** The logic, by name: `'boolean'` (the default). */
  logic?: string | null
  /** Throw an `EvaluationError` instead of giving unknown. */
  strict?: boolean | null
}

export interface EvaluateValueTreeOptions {
  /** Throw an `EvaluationError` instead of giving null. */
  strict?: boolean | null
}

/**
 * Evaluate a condition (or a field or boolean constant used as one) on
 * native data and return its result tree. An `ExpressionError` for a value
 * node; in strict mode, an `EvaluationError` for a missing field or a type
 * mismatch.
 *
 * @example
 * const tree = evaluateTree(F.age.ge(18).and(F.country.isIn(['ES'])), { age: 20 })
 * tree.truth                    // 'unknown'
 * tree.children[1]?.reason?.code // 'missing_field'
 */
export declare function evaluateTree(condition: Expr, data: unknown, options?: EvaluateTreeOptions | null): ResultTree

/**
 * Evaluate a value node on native data and return its result tree.
 *
 * @example
 * evaluateValueTree(F.price.mul(F.quantity), { price: 2.5, quantity: 4 }).value  // 10
 */
export declare function evaluateValueTree(value: Expr, data: unknown, options?: EvaluateValueTreeOptions | null): ResultTree

// --- Decision tables -----------------------------------------------------

/** A decision table's hit policy: a rule set's, or `output_order` (DMN's O). */
export type TablePolicy = HitPolicy | 'output_order'

/**
 * An output column: its payload key and its output values, highest priority
 * first. In a header, `new TableOutput('risk', ['high', 'low'])` is the column
 * `THEN risk IN ('high', 'low')`: the values are constants, not text.
 */
export declare class TableOutput {
  constructor(name: string, values?: Iterable<Constant>)
  readonly name: string
  /**
   * The output values, highest priority first: as given to the constructor
   * (constants), or as the engine gives them back for a table's `outputs`.
   */
  readonly values: ReadonlyArray<Value | Constant>
}

/** A row of a table: one rule. */
export declare class TableRow {
  private constructor()
  /** The rule's name, or `null` for `row_<n>`. */
  readonly name: string | null
  /** The rule's own priority, or `null`. */
  readonly priority: number | bigint | null
  readonly description: string | null
  /** One condition per input, testing that input; `null` for any value. */
  readonly inputs: ReadonlyArray<Expr | null>
  /** One value per output; `null` for no value. */
  readonly outputs: ReadonlyArray<Expr | null>
}

/** A column of a table's header: its CSV text, an input `Expr`, or a `TableOutput`. */
export type HeaderItem = string | Expr | TableOutput

/**
 * A cell of a row: text read as in the table's CSV (`'> 5'`, `'-'`,
 * `"'VIP'"` for a string), an input's condition or an output's value as an
 * `Expr`, `null` or `undefined` for an empty cell, or a constant.
 */
export type Cell = string | Expr | Constant | undefined

export interface DecisionTableOptions {
  /** `'first'` by default; names in any case. */
  hitPolicy?: TablePolicy | Uppercase<TablePolicy> | null
  /** With `'collect'`. */
  aggregate?: Aggregate | Uppercase<Aggregate> | null
  /** `'boolean'` by default. */
  logic?: string | null
  strict?: boolean | null
  /** `maxSize` (the longest text of a header or cell), `maxDepth` and `maxTableSize`. */
  limits?: Limits | null
}

/**
 * A decision table and the rule set it compiles into (`spec/tables.md`);
 * immutable. Read one with `load('table.csv')` or `loads(text, { format:
 * 'csv' })`, or build one from a header and rows, as a spreadsheet holds it.
 * Errors point into the grid: `/header/<c>` or `/rows/<r>/<c>`, from 0.
 *
 * @example
 * const table = new DecisionTable(
 *   ['RULE', 'customer.years', 'order.total', 'THEN discount'],
 *   [['vip', '> 5', '> 1000', decimal('0.15')], ['loyal', '> 2', '-', decimal('0.05')]],
 * )
 * table.evaluate({ customer: { years: 9 }, order: { total: 1500 } }).output
 */
export declare class DecisionTable {
  constructor(header: Iterable<HeaderItem>, rows: Iterable<Iterable<Cell>>, options?: DecisionTableOptions | null)
  readonly hitPolicy: TablePolicy
  readonly aggregate: Aggregate | null
  readonly logic: string
  readonly strict: boolean
  /** The input expressions, in column order. */
  readonly inputs: readonly Expr[]
  /** The output columns. */
  readonly outputs: readonly TableOutput[]
  /** The rows, in order. */
  readonly rows: readonly TableRow[]
  /** The rule set the table compiles into: one rule per row. */
  readonly ruleset: RuleSet
  evaluate(data: unknown): Decision
  evaluateMany(rows: Iterable<unknown>): Decision[]
  summary(): Summary
  /**
   * The decision table document of the JSON format, as an object for
   * `JSON.stringify`: a float that holds an integer is `{"$float": n}`, so the
   * text reads back as the same table, except that a float zero loses its sign
   * (`JSON.stringify(-0)` is `0`) and integer-like keys, such as a payload key
   * `"10"`, come first, as in any plain object. `dumps(table)` writes it
   * exactly.
   */
  toJSON(): Record<string, unknown>
}

// --- Documents -----------------------------------------------------------

/** A document format: JSON, YAML, the text syntax of rules, or CSV for decision tables. */
export type Format = 'json' | 'yaml' | 'text' | 'csv'

/** What a document holds. */
export type Loaded = Expr | Rule | RuleSet | DecisionTable

/** An action registry: each name with its function. */
export type Actions = Readonly<Record<string, Action>> | ReadonlyMap<string, Action>

export interface LoadOptions {
  /** `'json'` by default for `loads`; the file's extension for `load`. */
  format?: Format | null
  /** The limits; each one left out keeps the core's default. */
  limits?: Limits | null
  /** Bind a rule or rule set read to these actions (`RuleSet.bind`). */
  actions?: Actions | null
}

export interface DumpOptions {
  /** `'json'` by default for `dumps`; the file's extension for `dump`. */
  format?: Format | null
  /** Write rules without their actions given as code, instead of refusing; `null` is `false`. */
  omitActions?: boolean | null
}

export interface RuleFromTextOptions {
  /** The rule's action, a function; bound to the name the text gives (`DO notify`), if any. */
  do?: Action | null
  /** An action registry, as `loads` takes; not with `do`. */
  actions?: Actions | null
  limits?: Limits | null
}

/**
 * Read a condition, rule, rule set or decision table document (`string` or
 * UTF-8 bytes). A rule's action is the name the document gives it, unbound
 * unless `actions` is given. CSV holds a decision table; the text syntax a
 * condition, a rule or a rule set.
 *
 * @example
 * const rules = loads(text, { format: 'yaml' })
 */
export declare function loads(text: string | Uint8Array, options: LoadOptions & { format: 'csv' }): DecisionTable
export declare function loads(text: string | Uint8Array, options: LoadOptions & { format: 'text' }): Expr | Rule | RuleSet
export declare function loads(text: string | Uint8Array, options?: LoadOptions | null): Loaded

/**
 * Write a condition, rule, rule set or decision table in its canonical form.
 *
 * @example
 * const yaml = dumps(rules, { format: 'yaml' })
 * An `UnsupportedError` for an action given as code (unless `omitActions`),
 * for CSV of anything but a table, and for what the text syntax cannot hold.
 */
export declare function dumps(value: Loaded, options?: DumpOptions | null): string

/**
 * Read a document from a file (Node and Deno), its format from the extension
 * (`.json`, `.yaml`, `.yml`, `.rules`, `.csv`) unless given. Rejects with an
 * `UnsupportedError` in browsers (use `loads`).
 *
 * @example
 * const rules = await load('pricing.yaml', { actions: { notify } })
 */
export declare function load(path: string | URLLike, options: LoadOptions & { format: 'csv' }): Promise<DecisionTable>
export declare function load(path: string | URLLike, options: LoadOptions & { format: 'text' }): Promise<Expr | Rule | RuleSet>
export declare function load(path: string | URLLike, options?: LoadOptions | null): Promise<Loaded>

/**
 * Write a document to a file (Node and Deno), as `dumps` does; a rule text
 * ends with a line break. Rejects with an `UnsupportedError` in browsers (use
 * `dumps`).
 *
 * @example
 * await dump(rules, 'pricing.rules')
 */
export declare function dump(value: Loaded, path: string | URLLike, options?: DumpOptions | null): Promise<void>

/** A `file:` URL (`URL` where the TypeScript libraries in use declare it). */
export type URLLike = typeof globalThis extends { URL: abstract new (...args: never[]) => infer Url } ? Url : never

/**
 * Read one rule written in the text syntax (`spec/text.md`); without a
 * `RULE` line it is named `rule_1`. fuzzytool's verb.
 *
 * @example
 * const vip = ruleFromText('RULE vip IF customer.years > 5 THEN discount = 0.15')
 */
export declare function ruleFromText(text: string | Uint8Array, options?: RuleFromTextOptions | null): Rule
