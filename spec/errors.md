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
| Python | exception classes with the names above, under `cleanrules.CleanRulesError`. They keep the builtin bases of the v1, so existing `except` clauses still work: `ExpressionError` is also a `TypeError` and a `ValueError`; `RuleSyntaxError` and `RuleDefinitionError` are `ValueError`s; `RegistryError` is a `LookupError`; `RuleNotFoundError` a `KeyError`; `UnsupportedError` a `NotImplementedError`. The cause is `__cause__`. |
| Java, Scala | unchecked exceptions named with `Exception` instead of `Error` (`CleanRulesException`, `MissingFieldException`, …), with a `code()` method. The cause is `getCause()`. |
| JavaScript, TypeScript | `Error` subclasses with the names above and a `code` property. The cause is `cause`. |
| C++ | exceptions with the names above, deriving from `std::runtime_error`, with `code()`. |
| Go | one type, `*cleanrules.Error`, with a `Code` field and the context. Each parent of the hierarchy is a sentinel (`ErrEvaluation`, `ErrFormat`, …), and the type's `Is` method matches its own code's sentinel and its parent's, so `errors.Is(err, cleanrules.ErrEvaluation)` is true for a `missing_field`. The cause is returned by `Unwrap`. |
| C | functions return a `cr_status`: `CR_OK` or a `cr_error_code`, an enum with one stable numeric value per code (part of the ABI). `cr_error_parent(code)` gives the parent's code. The message, the context and the cause's message are read from the last error of the handle. |
| Rust | a `#[non_exhaustive]` `Error` with `code()`, accessors for the context, and `source()` for the cause. |

## Codes

| Code | Raised when | Context (compared fields in **bold**) |
|---|---|---|
| `format_error` | a document's JSON or shape is invalid (`json-format.md`) | **`pointer`**, or **`line`** and **`column`** for a syntax error |
| `unsupported_version` | a document's `version` is missing or unknown | **`pointer`** |
| `limit_exceeded` | a document is too large or nested too deep | for node depth, **`pointer`** to the first node too deep; for JSON nesting, **`line`** and **`column`** of the bracket that exceeds it; for size, none |
| `decision_mismatch` | a decision does not belong to the rule set it is read with | **`pointer`** |
| `expression_error` | a condition node breaks a construction rule (`ast.md`) | `rule`, `node`; **`pointer`** when read from JSON |
| `rule_definition_error` | a rule or rule set breaks a construction rule (`rules.md`, `json-format.md`) | **`rule`** when known; **`pointer`** when read from JSON |
| `rule_syntax_error` | the text DSL cannot be parsed (phase D) | **`line`**, **`column`** |
| `registry_error` | a name is not registered (a logic; later, an action) | **`name`**, `available`; **`pointer`** when read from JSON |
| `rule_not_found` | a rule is looked up by a name the rule set does not have | **`name`**, `available` |
| `evaluation_error` | strict mode met a value it cannot read (`read_error`), or a batch was given something that is not a collection of inputs (`rules.md`) | **`rule`**, **`path`**, `node`, **`row`** |
| `missing_field` | strict mode met a field whose path cannot be resolved | **`rule`**, **`path`**, `node`, **`row`**, `available` |
| `type_mismatch` | strict mode met values of incompatible groups or a value of the `other` group, ordered booleans, or a non-boolean used as a condition | **`rule`**, **`path`**, **`types`**, `node`, **`row`** |
| `hit_policy_error` | the hit policy cannot decide (below) | **`rules`**, **`row`** |
| `action_error` | an action raised during `run` (task D.4) | **`rule`**, **`executed`**, `decision` |
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

## Differences from the Python v1

- The base class is `CleanRulesError` (the v1's was `RulePyError`), and the
  format errors gain a class, `FormatError`.
- `UnsupportedOperation` becomes `UnsupportedError`, and names the operation,
  not a backend: there is one engine.
- `TypeMismatchError.types` are written in snake case (`naive_datetime`); the
  v1 wrote `naive datetime`.
