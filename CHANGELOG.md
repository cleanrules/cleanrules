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
- JSON format: rules (`name`, `when`, payload `then`, `priority`, `tags`,
  `description`, `metadata`) and rule sets (`hit_policy`, `aggregate`, `logic`,
  `strict`, `rules`), with JSON Schemas and the error codes
  `rule_definition_error`, `registry_error` and `unsupported`. Payload member
  order is significant. Actions are not serialized yet (writers take
  `omit_actions`), and rule metadata values must be data values.
- JSON format: decisions, with every rule's result tree (`truth` for
  conditions, `value` for values), structured reasons and the selected rules;
  outputs are derived from the payload trees. A decision is read with its rule
  set and checked against it node by node (`decision_mismatch`).
- Error catalogue (`spec/errors.md`): stable codes, the hierarchy under
  `CleanRulesError`, and the context fields that conformance vectors compare.
- Conformance vectors (`conformance/`): the vector format and its JSON Schema,
  a checker, 125 hand-picked and 1,821 generated vectors exported from the Python v1,
  and 187 vectors for
  reading documents, written from the spec.
- Data model (`spec/data-model.md`): values, decimals of 38 digits (as Arrow
  `Decimal128`), microsecond datetimes, opaque values for native data with no
  equivalent, nulls with a reason (`int_overflow`, the new `decimal_overflow`,
  `read_error`), and how each language's data is read and converted.
- Specification of explanations and summaries (`spec/explain.md`): rule
  status, requirements with `all`/`any` groups, simplification, the check that
  each way to fire really works, counterfactuals and notes. Their JSON forms
  are output-only documents.
- Specification: `matches` patterns use the syntax of Rust's `regex` crate, and
  a null constant is only allowed as the operand of `is_null` (also under
  `and`, `or` and `not`).
- `cleanrules::explain`: explanations (status, `all`/`any` requirements,
  simplification, options checked to make the rule fire, causes with the values
  read, counterfactuals with exact gaps, notes) and summaries, with their JSON
  forms. The conformance runner passes all 317 explanation and 3 summary vectors.
- `cleanrules::format::read_decision_document`: decisions read with their rule
  set, rebuilt into a `Decision` and checked as `spec/json-format.md` requires
  (`decision_mismatch`).
- `cleanrules::rules`: rules, rule sets, the six hit policies, payloads,
  aggregates, batches and decisions; rule and rule set documents, and decisions
  written in JSON. The conformance runner passes all 302 evaluation vectors.
- `cleanrules::eval`: the evaluator, generic over the logic, with result
  trees, structured reasons, exact comparisons, 64-bit overflow, and strict
  mode; result trees in JSON (`format::write_result`). The core's conformance
  runner passes all 1,326 condition and value vectors.
- `cleanrules::logic`: the `Logic` trait, Kleene's three-valued logic
  registered as `"boolean"`, and lookup by name with `registry_error`.
- `cleanrules::build`: idiomatic Rust construction of conditions, with
  methods for comparisons and tests, operators for connectives and arithmetic,
  and the first construction error reported by `.build()`.
- `cleanrules::ast`: the condition AST, valid by construction, with the
  construction rules of `spec/ast.md`, structural identity and field paths.
- `cleanrules::format::read_document` and `write_document` for condition
  documents, with the error order and locations of the spec.
- `cleanrules::format::value`: values in JSON, read as constants, data or
  recorded values and written in canonical form, including datetimes with
  fixed offsets or IANA time zones checked against the bundled database.
- `cleanrules::json`: a strict JSON reader and writer that keeps member order
  and number literals, rejects duplicate keys, lone surrogates and byte order
  marks, reports errors by line and column in code points, enforces size and
  nesting limits, and never overflows the stack.
- Core value model: `Value` (null with a reason, booleans, ints, floats,
  38-digit decimals, strings, dates, naive and aware datetimes, lists, maps,
  opaque values), exact comparison across ints, floats and decimals, type
  groups, equality under `ANY`, and the core `Error` with the stable codes of
  `spec/errors.md`.
- Rust workspace with the core crate `cleanrules` (edition 2024, MSRV 1.95):
  strict lints (no `unsafe`, clippy pedantic, no `unwrap` or printing in
  library code), rustfmt, cargo-deny, and CI on Linux, macOS and Windows.
- Repository skeleton: README, MIT license, changelog, `.gitignore`.
- Development roadmap (`docs/PLAN.md`) and Claude Code project configuration
  (`CLAUDE.md`, `.claude/`).

### Removed

- The Python v1 (`rulepy`), its tests, its documentation site and the
  temporary vector exporter (task A.4). Its semantics live on in `spec/` and
  `conformance/`, and the code in the git tag `python-reference`; the section
  below lists what it contained. The Python package comes back as a binding of
  the Rust core in phase C, under the name `cleanrules`.

## [python-reference] - not released

The Python v1, kept in the git tag `python-reference`.

### Added

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
- Condition AST specification (`spec/ast.md`, draft): node catalogue,
  Kleene three-valued semantics, null and type-mismatch rules.
- The package ships a `py.typed` marker.
- Documentation site skeleton (MkDocs Material + mkdocstrings) with the API
  reference for `rulepy.errors` and the conventions shared by every backend
  (`docs/spec/README.md`).
- Development tooling: `dev` dependency group (pytest, pytest-cov, hypothesis,
  ruff, pyright) with strict pytest, branch coverage (>= 90%), strict pyright
  and ruff (Google docstrings) configured in `pyproject.toml`.
