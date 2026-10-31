# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- JSON format, version 1 (`spec/json-format.md`), the contract between
  languages: documents with `"version": 1`, the encoding of values (exact
  64-bit integers, floats with `{"$float": "inf"}`, `{"$decimal": ...}`,
  `{"$date": ...}` and `{"$datetime": ...}` with fixed offsets or RFC 9557 time
  zones, lists and maps), every condition node, canonical writing, error codes
  and limits. JSON Schemas in `spec/schemas/v1/`, checked against valid and
  invalid examples. The specification moved from `docs/spec/` to `spec/`.
- Specification: `matches` patterns use the syntax of Rust's `regex` crate, and
  a null constant is only allowed as the operand of `is_null` (also under
  `and`, `or` and `not`).
- Repository skeleton: README, MIT license, changelog, `.gitignore`.
- Package metadata (`pyproject.toml`, hatchling, `src/` layout): Python >= 3.11,
  no runtime dependencies, optional extras `fuzzy`, `polars`, `pandas`, `yaml`,
  `audit`, `ml`, `fast` and `bench` reserved for upcoming features.
- `rulepy.errors` (also exported from `rulepy`): `RulePyError` base class with an
  optional `hint`; `UnsupportedOperation` (also a `NotImplementedError`) naming
  the operation, backend, rule and node; `RuleSyntaxError` (also a `ValueError`)
  reporting 1-based line/column with a caret under the offending line;
  `EvaluationError` naming the rule and node. All errors pickle with their
  attributes intact.
- `ExpressionError` (also a `TypeError` and `ValueError`) for malformed
  condition trees, with optional rule and node context.
- `rulepy.core.expr`: immutable condition AST with `Field("a", 0, "b")`,
  `Const` (normalizes numpy/Enum/subclass scalars; rejects NaN and integers
  beyond 64 bits), `Compare`, n-ary flattened `And(*conditions)` and
  `Or(*conditions)`, and `Not`. `Const` also keeps `Decimal` values exact (for
  money and rates), rejecting `Decimal` NaN. Trees are checked when built, can
  be walked, compared with `same_as()` and keyed with `structural_key()`, and
  pickle.
- More condition nodes: `In` (canonical sorted set of same-type values),
  `Between` (polars-style `closed`), `IsNull`, `Matches` (regex search),
  `StringTest` (`starts_with` / `ends_with` / `contains`, literal text) and
  arithmetic (`Arith` with `add` / `sub` / `mul` / `div`, `Abs`, `Neg`), with
  the type aliases `StringOp`, `ArithOp` and `Closed`.
- `F` (exported from `rulepy`) for writing conditions in Python:
  `(F.customer.age >= 18) & F.country.is_in({"ES", "PT"}) & ~F.blocked`,
  `F["key with spaces"]`, `F.orders[0].total`. Comparison, logical (`&`, `|`,
  `~`) and arithmetic operators build nodes, as do the methods `is_in`,
  `between`, `is_null`, `matches`, `starts_with`, `ends_with`, `contains` and
  `abs`.
- Nodes guard against Python pitfalls with a clear `ExpressionError`:
  `and`/`or`/`not`, `if node:`, chained comparisons, `== None`, Python bools
  next to `&`/`|`, iterating a node, numpy arrays and pandas objects as
  operands, polars-style namespaces, and calling a field path (with a "did you
  mean" hint). Nodes hash by identity.
- `repr()` of a condition is Python that rebuilds the same tree.
- "Common pitfalls" documentation page.
- `rulepy.core.logic`: `Truth` (ordered `FALSE < UNKNOWN < TRUE`, not usable as a
  Python bool), the `Logic` protocol (provisional until 1.0), the optional
  `GradedLogic` protocol, and `BooleanLogic` (Kleene three-valued logic), with
  a registry (`get_logic(name, **params)`, `register_logic`,
  `unregister_logic`, `logic_names`).
- `rulepy.core.evaluate`: the reference evaluator. `evaluate(condition, data)`
  returns a truth value; `evaluate_tree` returns a `NodeResult` tree with a
  structured `Reason` (code, field paths, type groups) wherever a value is
  `UNKNOWN` or null; `evaluate_value` evaluates fields and arithmetic. It reads
  mappings, sequences, records, pandas rows, dataclasses, Pydantic models and
  numpy arrays, never private attributes, and supports a `strict` mode.
- Rules and rule sets (`rulepy.Rule`, `rulepy.RuleSet`): a rule has a condition
  (`when`), a payload (`then`, constants or value expressions computed from the
  data) and an optional action (`do`). Rule sets apply DMN-style hit policies
  (`HitPolicy`: `FIRST`, `PRIORITY`, `UNIQUE`, `ANY`, `RULE_ORDER`, `COLLECT`
  with per-key `sum`/`min`/`max`/`count`) and return a `Decision` with every
  rule's result and tree, the selected rules, `outputs` and `output`.
  `evaluate()` and `evaluate_many()` are pure; `run()` also calls the actions.
  Rules and rule sets pickle and deep-copy. Decision outputs and aggregates are
  read-only, and aggregates follow the arithmetic rules (Decimal as float64,
  64-bit integer overflow and NaN are null). Semantics: `docs/spec/rules.md`.
- Errors: `RuleDefinitionError`, `HitPolicyError`, `ActionError` (with the
  decision and the actions already run), `RuleNotFoundError` (a `KeyError`);
  `EvaluationError.row` names the input in batches.
- `rulepy.core.evaluate.evaluate_value_tree` and `type_group`.
- Explanations (`rulepy.core.explain`): `Decision.explain()` reports, for every
  rule, whether it was selected, fired without being selected (and why), did not
  fire or is unknown, and its requirement: the conditions behind the outcome,
  grouped as `all` (each must change / each is needed) or `any` (one is enough),
  with the data values they read and structured counterfactuals
  (`short by 200`, `needs total <= 1000 (over by 300)`). The text report is
  ASCII. Suggested fixes are checked against the rule, and conflicting
  conditions are reported as such (`achievable`). `Decision.why_not(name)`
  explains one rule; `Decision.logic` records
  the logic used. `rulepy.core.expr.format_scalar` renders constants.
- `RuleSet.summary()`: a static overview of a rule set (configuration, one line
  per rule, the fields it reads and where, including which outputs depend on
  them, its outputs and tags).
- `Expr.field_paths()`: the fields a condition or value reads, in order.
- `rulepy.errors.format_name`: quotes rule names, tags and keys for reports.
- `rulepy.errors.format_path` renders field paths unambiguously
  (`orders[0].total`, `['x y']`).
- Errors for evaluation and registries: `TypeMismatchError`,
  `MissingFieldError` (both name the field path) and `RegistryError`.
- Condition AST specification (`docs/spec/ast.md`, draft): node catalogue,
  Kleene three-valued semantics, null and type-mismatch rules.
- The package ships a `py.typed` marker.
- Documentation site skeleton (MkDocs Material + mkdocstrings) with the API
  reference for `rulepy.errors` and the conventions shared by every backend
  (`docs/spec/README.md`).
- Development tooling: `dev` dependency group (pytest, pytest-cov, hypothesis,
  ruff, pyright) with strict pytest, branch coverage (>= 90%), strict pyright
  and ruff (Google docstrings) configured in `pyproject.toml`.
- Development roadmap (`docs/PLAN.md`) and Claude Code project configuration
  (`CLAUDE.md`, `.claude/`).
