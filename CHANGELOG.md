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
