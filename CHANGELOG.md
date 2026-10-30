# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

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
  `Or(*conditions)`, and `Not`. Trees are checked when built, can be walked,
  compared with `same_as()` and keyed with `structural_key()`, and pickle.
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
