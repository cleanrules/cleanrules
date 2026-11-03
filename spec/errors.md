# Errors

**Status:** draft, normative for the core and every binding.

Every error of cleanrules has a stable **code**, the same in every language,
and a fixed set of **context fields**. Messages are written by the core, so
they are the same in every language: they name the rule or node that failed and
say how to fix it. Conformance vectors compare the code and the context fields
marked as compared below, never the message or the hint.

## Hierarchy

*Not checked by vectors: each binding's tests check its error types.*

```text
CleanRulesError
├── FormatError                 format_error, unsupported_version, limit_exceeded, decision_mismatch
├── ExpressionError             expression_error
├── RuleDefinitionError         rule_definition_error
├── RuleSyntaxError             rule_syntax_error
├── RegistryError               registry_error
├── RuleNotFoundError           rule_not_found
├── EvaluationError             evaluation_error
│   ├── MissingFieldError       missing_field
│   ├── TypeMismatchError       type_mismatch
│   └── HitPolicyError          hit_policy_error
├── ActionError                 action_error
└── UnsupportedError            unsupported
```

Every error also has a `message` and an optional `hint`. An error caused by
another one (a `read_error` in strict mode, an action that raised) keeps it as
its cause.

## In each language

*Not checked by vectors: each binding's tests check its error types.*

| Language | Form |
|---|---|
| Python | exception classes with the names above, under `cleanrules.CleanRulesError`. They also derive from Python's builtin exceptions, so `except` clauses written for those work: `ExpressionError` is also a `TypeError` and a `ValueError`; `RuleSyntaxError` and `RuleDefinitionError` are `ValueError`s; `RegistryError` is a `LookupError`; `RuleNotFoundError` a `KeyError`; `UnsupportedError` a `NotImplementedError`. The cause is `__cause__`. |
| Java, Scala | unchecked exceptions named with `Exception` instead of `Error` (`CleanRulesException`, `MissingFieldException`, …), with a `code()` method. The cause is `getCause()`. |
| JavaScript, TypeScript | `Error` subclasses with the names above and a `code` property. The cause is `cause`. |
| C++ | exceptions with the names above, deriving from `std::runtime_error`, with `code()`. |
| Go | one type, `*cleanrules.Error`, with a `Code` field and the context. Each parent of the hierarchy is a sentinel (`ErrEvaluation`, `ErrFormat`, …), and the type's `Is` method matches its own code's sentinel and its parent's, so `errors.Is(err, cleanrules.ErrEvaluation)` is true for a `missing_field`. The cause is returned by `Unwrap`. |
| C | functions return a `cr_status` (`int32_t`): `CR_OK` (0), the numeric value of an error code (*Numeric codes*, part of the ABI), or a negative status for a misuse that only C can make: `CR_INVALID_ARGUMENT` (-1: a null pointer where one is required, an unknown format or limit) and `CR_INTERNAL_ERROR` (-2: a bug in cleanrules, caught at the boundary). A fallible function takes a last argument `cr_error **error`, which may be `NULL` and otherwise points to `NULL`; on failure it receives a new `cr_error`, which the caller frees with `cr_error_free`, with the code, the message, the hint and the error's *JSON form*, and on success it is left alone. Out-parameters are `NULL` after a failure. `cr_error_parent(code)` gives the parent's code, 0 for a root. Texts are passed as bytes and a length: bytes that are not UTF-8 are a `format_error` at the line and column of the first invalid byte, in every format, as for JSON (`json-format.md`, *Errors and limits*). The cause's message is the `cause` of the JSON form. |
| Rust | a `#[non_exhaustive]` `Error` with `code()`, accessors for the context, and `source()` for the cause. |

## Codes

| Code | Raised when | Context (compared fields in **bold**) |
|---|---|---|
| `format_error` | a document's JSON or shape is invalid (`json-format.md`) | **`pointer`**, or **`line`** and **`column`** for a syntax error |
| `unsupported_version` | a document's `version` is missing or unknown | **`pointer`** |
| `limit_exceeded` | a document is too large or nested too deep | for node depth, **`pointer`** to the first node too deep, or in text **`line`** and **`column`** where it starts; for JSON nesting, **`line`** and **`column`** of the bracket that exceeds it, and in text of the token that nests too deep (`text.md`); for size, none |
| `decision_mismatch` | a decision does not belong to the rule set it is read with | **`pointer`** |
| `expression_error` | a condition node breaks a construction rule (`ast.md`) | `rule`, `node`; **`pointer`** when read from JSON; **`line`** and **`column`** when read from text (`text.md`) |
| `rule_definition_error` | a rule or rule set breaks a construction rule (`rules.md`, `json-format.md`) | **`rule`** when known; **`pointer`** when read from JSON; **`line`** and **`column`** when read from text |
| `rule_syntax_error` | a text in the rule syntax cannot be parsed (`text.md`) | **`line`**, **`column`** |
| `registry_error` | a name is not registered: a logic, or an action when a rule set's named actions are bound (`rules.md`, *Actions*) | **`name`**, `available`; for a logic, **`pointer`** when read from JSON, **`line`** and **`column`** when read from text; for an action, **`rule`** |
| `rule_not_found` | a rule is looked up by a name the rule set does not have | **`name`**, `available` |
| `evaluation_error` | strict mode met a value it cannot read (`read_error`), or a batch was given something that is not a collection of inputs (`rules.md`) | **`rule`**, **`path`**, `node`, **`row`** |
| `missing_field` | strict mode met a field whose path cannot be resolved | **`rule`**, **`path`**, `node`, **`row`**, `available` |
| `type_mismatch` | strict mode met values of incompatible groups or a value of the `other` group, ordered booleans, or a non-boolean used as a condition | **`rule`**, **`path`**, **`types`**, `node`, **`row`** |
| `hit_policy_error` | the hit policy cannot decide (below) | **`rules`**, **`row`** |
| `action_error` | an action raised during `run` | **`rule`**, **`executed`**, `decision` |
| `unsupported` | this build, this logic or this version of the format cannot do the operation (`json-format.md`, `explain.md`) | `operation`, `reason` |

Context fields:

- `pointer`: a JSON Pointer into the document as written (`json-format.md`,
  *Errors and limits*);
- `line`, `column`: 1-based, counting Unicode code points, not bytes or UTF-16
  units;
- `rule`: a rule name; `rules`, `executed`: rule names in rule order;
- `path`: the **full path of the field** being read, as an array of strings and
  integers (a reason, instead, gives the path up to where reading failed;
  `ast.md`, *Results and reasons*); absent when the value is a constant;
- `types`: type groups, in the order of `ast.md` (*Results and reasons*);
- `row`: the 0-based position of the input in a batch;
- `node`: the failing node, as a condition node in JSON form, when it can be
  written (a NaN constant cannot);
- `name`, `available`: the name looked up, and the names that exist, sorted;
- `operation`, `reason`: what was not supported, and why, in words.

**`hit_policy_error`** is raised when `UNIQUE` sees more than one rule fire
(`rules` lists every rule that fired), when `ANY` sees fired rules with
different outputs (every rule that fired), when an aggregate meets a value that
is not a number (the rules whose payloads hold one), and when a multi-hit
decision is asked for its single `output` (`rules` is empty).

### Numeric codes

*Not checked by vectors: the C ABI's tests check them.*

The C ABI gives each code a stable number, which never changes and is never
reused; a new code takes the next one.

| Code | C constant | Value |
|---|---|---|
| `format_error` | `CR_FORMAT_ERROR` | 1 |
| `unsupported_version` | `CR_UNSUPPORTED_VERSION` | 2 |
| `limit_exceeded` | `CR_LIMIT_EXCEEDED` | 3 |
| `decision_mismatch` | `CR_DECISION_MISMATCH` | 4 |
| `expression_error` | `CR_EXPRESSION_ERROR` | 5 |
| `rule_definition_error` | `CR_RULE_DEFINITION_ERROR` | 6 |
| `rule_syntax_error` | `CR_RULE_SYNTAX_ERROR` | 7 |
| `registry_error` | `CR_REGISTRY_ERROR` | 8 |
| `rule_not_found` | `CR_RULE_NOT_FOUND` | 9 |
| `evaluation_error` | `CR_EVALUATION_ERROR` | 10 |
| `missing_field` | `CR_MISSING_FIELD` | 11 |
| `type_mismatch` | `CR_TYPE_MISMATCH` | 12 |
| `hit_policy_error` | `CR_HIT_POLICY_ERROR` | 13 |
| `action_error` | `CR_ACTION_ERROR` | 14 |
| `unsupported` | `CR_UNSUPPORTED` | 15 |

### JSON form

*Not checked by vectors: the core's tests check it.*

For the C ABI, logs and agents, the core writes an error as a JSON object with
these members, in this order, each only when it applies: `code`, `message`,
`hint`; then the context fields `pointer`, `line`, `column`, `rule`, `rules`,
`path` (a path as in `json-format.md`), `types`, `row`, `name`, `available`,
`executed`, `node` (a node in JSON form, as in `json-format.md`, even a null constant), `operation` and `reason`,
leaving out empty arrays; and `cause`, the message of the error that caused it.
The C ABI's own statuses are written with the codes `invalid_argument` and
`internal_error`. JSON Schema: `schemas/v1/error.schema.json`.

The member names and what they hold are stable, like the format's
(`json-format.md`, *Versioning*): a new version may add members, never change or
remove one. Bindings built on the C ABI (Go, the JVM) and agents read the code
and the context from it. `message`, `hint` and `cause` are free text for people,
whose wording may change. Conformance vectors still compare only the fields of
*Comparing errors*.

### Comparing errors

A conformance vector's `error` holds the code and the compared fields that
apply. A runner keeps, from the actual error, the code and the compared fields
only, and compares the two objects without regard to member order:

- a compared field that applies must be present and equal;
- one that does not apply must be absent: `rule` when a bare condition or value
  is evaluated, `row` outside a batch, `path` for a constant;
- fields that are not compared, the message and the hint are dropped.

## Which error comes first

- Reading a document: `json-format.md`, *Errors and limits*.
- Evaluating in strict mode: `ast.md` (*Results and reasons*) for conditions,
  and `rules.md` (*Strict mode and errors*) for rule sets, payloads and hit
  policies. Strict mode raises only for `missing_field`, `type_mismatch`,
  `unorderable` (as `type_mismatch`) and `read_error`; nulls, division by zero,
  overflows and NaN results stay nulls with their reasons.
- In a batch, the first input in order that fails; its `row` says which.
