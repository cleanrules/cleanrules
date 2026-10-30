# Specification

RulePy has several implementations of the same semantics:

- the **Python reference evaluator**, which is the oracle;
- the **polars** backend for validation;
- the incremental **Rete** matcher;
- the optional **Rust** core (`rulepy-core`).

Users' rule files (JSON, YAML, text) also have to keep meaning the same thing
across versions. The documents in this directory are the contract that all of
them follow. When an implementation and a spec disagree, the spec wins, and the
disagreement is a bug in the implementation or in the spec. It is never
acceptable backend-specific behaviour.

## Documents

| Document | Status | Defines |
|---|---|---|
| `ast.md` | planned (task 3.1) | Every condition node: its serialized form, operator tag, typing and null semantics |
| `ast-schema.json` | planned (task 3.1) | JSON Schema for serialized rules and rule sets |

## Conventions that apply to every spec

- **Truth values.** Crisp logic is Kleene three-valued: `TRUE`, `FALSE` and
  `UNKNOWN`. A missing field or a null makes a comparison `UNKNOWN`, and a rule
  fires only when its condition is `TRUE`. This matches polars' null handling.
  Fuzzy logic maps truth to `[0, 1]` using the connectives configured in
  fuzzytool.
- **Operator tags.** Each node has a stable lowercase tag, such as `"and"`,
  `"between"` or `"matches"`. The tag is used in serialized rules, in
  `UnsupportedOperation.operation` and in every backend. It is never a Python
  class name.
- **Format version.** Serialized documents carry `"rulepy_ast": <n>`. Adding a
  new node or an optional field keeps `n`. Changing or removing anything bumps
  `n`, and readers reject versions they do not know with a clear error.
- **Positions.** Line and column numbers in errors are 1-based.
- **Regular expressions.** Only the subset supported by both Python's `re` and
  Rust's `regex` crate is allowed: no lookaround and no backreferences.
- **Determinism.** The same rules and inputs give the same firing order and the
  same explanation, on every backend.
