# Specification

cleanrules has one engine, the Rust core, used from Rust, C/C++, Python,
JavaScript, Java/Scala and Go. Users' rule files (JSON, YAML, text) are read by
all of them and have to keep meaning the same thing across versions. The
documents in this directory are the contract they all follow, and the
conformance vectors in `conformance/` turn it into tests that every binding
runs. When the engine and a spec disagree, the spec wins, and the disagreement
is a bug in the engine or in the spec. It is never acceptable
language-specific behaviour.

The semantics come from the Python v1 (`rulepy`, kept in the git tag
`python-reference`). Where the spec departs from it on purpose, each document
lists the differences.

## Documents

| Document | Status | Defines |
|---|---|---|
| [`ast.md`](ast.md) | draft | Every condition node: operator tag, kind, construction rules, typing and null semantics |
| [`rules.md`](rules.md) | draft | Rules, hit policies, payloads, outputs, aggregates and actions |
| [`data-model.md`](data-model.md) | draft | Values (decimals of 38 digits, datetimes, opaque values, nulls with a reason) and how each language's data becomes values |
| [`errors.md`](errors.md) | draft | Error codes, hierarchy and compared context fields |
| [`explain.md`](explain.md) | draft | Explanations (status, requirements, simplification, counterfactuals, notes) and summaries |
| [`json-format.md`](json-format.md) | draft, format version 1 | Documents, values, conditions, rules, rule sets, decisions, explanations and summaries as JSON |
| [`schemas/v1/`](schemas/v1/) | draft | JSON Schemas (draft 2020-12) for values and for every document (condition, rule, rule set, decision, explanation, summary), with valid and invalid examples checked by `schemas/check.py` |

## Conventions that apply to every spec

- **Truth values.** Crisp logic is Kleene three-valued: `TRUE`, `FALSE` and
  `UNKNOWN`. A missing field or a null makes a comparison `UNKNOWN`, and a rule
  fires only when its condition is `TRUE`. This matches SQL's null handling.
  Fuzzy logic maps truth to degrees in `[0, 1]` using the connectives
  configured in fuzzytool; unknown is the interval `[0, 1]` (proposed, see
  `ast.md`).
- **Operator tags.** Each node has a stable lowercase tag, such as `"and"`,
  `"between"` or `"matches"`. The tag is used in serialized rules, in errors
  and in every language. It is never a class name.
- **Format version.** Serialized documents carry `"version": <n>`. Adding a
  new node or an optional field keeps `n`. Changing or removing anything bumps
  `n`, and readers reject versions they do not know with a clear error
  (`json-format.md`).
- **Positions.** Line and column numbers in errors are 1-based.
- **Regular expressions.** The syntax of Rust's `regex` crate: no lookaround
  and no backreferences (`ast.md`, *`matches`*).
- **Determinism.** The same rules and inputs give the same firing order and the
  same explanation, in every language.
