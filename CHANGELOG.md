# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Repository skeleton: README, MIT license, changelog, `.gitignore`.
- Package metadata (`pyproject.toml`, hatchling, `src/` layout): Python >= 3.11,
  no runtime dependencies, optional extras `fuzzy`, `polars`, `pandas`, `yaml`,
  `audit`, `ml`, `fast`, `bench` and `docs` reserved for upcoming features.
- Development tooling: `dev` dependency group (pytest, pytest-cov, hypothesis,
  ruff, pyright) with strict pytest, branch coverage (>= 90%), strict pyright
  and ruff (Google docstrings) configured in `pyproject.toml`.
- Development roadmap (`docs/PLAN.md`) and Claude Code project configuration
  (`CLAUDE.md`, `.claude/`).
