# cleanrules command line

`cleanrules` audits, validates and draws rule sets and decision tables of the
[cleanrules](https://github.com/cleanrules/cleanrules) rules engine, with exit
codes made for CI. Every report, diagram and error it prints is written by the
engine's core, so it is the same text you get from Rust, Python or
JavaScript. The full contract is [`spec/cli.md`](https://github.com/cleanrules/cleanrules/blob/main/spec/cli.md).

> **Pre-alpha.** Prebuilt binaries are not released yet: build it from
> source, or take a binary from the artifacts of the `CLI binaries` workflow.

```console
$ cargo install --locked --git https://github.com/cleanrules/cleanrules cleanrules-cli
```

## Commands

```text
cleanrules validate [--syntax <syntax>] [--format text|json] <file>
cleanrules audit    [--syntax <syntax>] [--format text|json] [--fail-on error|warning|info|never]
                    [--max-terms <n>] [--max-steps <n>] [--ruleset-digest] <file>
cleanrules draw     [--syntax <syntax>] [--format mermaid|dot|html|html_fragment]
                    [--output <path>] [--max-diagram-size <n>] <file>
```

- **`validate`** reads a document and says what it holds, with the summary of
  its rules, or why it does not read (exit code 1); `--format json` gives the
  result of the agents' `validate` tool.
- **`audit`** runs the static audit ([`spec/audit.md`](https://github.com/cleanrules/cleanrules/blob/main/spec/audit.md))
  of a rule set, a decision table, a bundle or a rule: overlapping rules,
  gaps, unsatisfiable conditions, shadowed rules, incompatible and mixed
  types, and table cells that read a field by mistake, each with a severity
  that depends on the hit policy, and an input that shows overlaps and gaps.
  `--format json` gives the audit document, with its JSON Schema
  `spec/schemas/v1/audit.schema.json`.
- **`draw`** writes the diagram of a condition, rule, rule set, decision table
  or bundle ([`spec/visual.md`](https://github.com/cleanrules/cleanrules/blob/main/spec/visual.md)): Mermaid by default,
  HTML for decision tables.

`<file>` is read in the syntax its suffix gives (`.json`, `.yaml`/`.yml`,
`.rules` for the rule syntax, `.csv` for decision tables), or the one
`--syntax` gives; `-` reads standard input, JSON unless `--syntax` says
otherwise.

## Exit codes

| Code | Meaning |
|---|---|
| `0` | done, nothing found at or above `--fail-on` |
| `1` | `audit` found a diagnostic at or above `--fail-on` (default `error`), or `validate` a document that does not read |
| `2` | usage error |
| `3` | a file could not be read or written, a document did not read, or the operation is not supported for it (such as auditing a case file) |

## Example

```console
$ cleanrules audit age.rules
Audit of a rule set: 2 rules, hit policy unique, logic boolean
error    overlap  adult, senior: rules adult and senior both fire for the witness, so the unique hit policy raises hit_policy_error: make their conditions exclusive, or use first or priority
           witness: {"age":65}
warning  gap  no rule fires for the witness, so the decision has no output: add a rule whose condition covers it; a rule with the condition TRUE would fire with every other rule
           witness: {"age":0}
Checks: types complete, fields not applicable, unsatisfiable complete, shadowed not applicable, overlap complete, gap complete
$ echo $?
1
$ cleanrules audit age.rules --fail-on never --format json > audit.json
$ cleanrules draw age.rules --format dot --output age.dot
$ cleanrules audit rules.json
error[format_error]: unknown key "whn"
  --> rules.json at /ruleset/rules/0/whn
  hint: it may come from a newer version of cleanrules
```

## In CI

[`docs/examples/ci/cleanrules-audit.yml`](https://github.com/cleanrules/cleanrules/blob/main/docs/examples/ci/cleanrules-audit.yml)
is a GitHub Actions workflow that audits every rule file of a repository and
fails on errors, keeping each JSON report as an artifact. Until binaries are
released, it builds `cleanrules` from source.

## Licence

Apache License 2.0, created by Jose L. Salmeron: redistributions keep the
[NOTICE](https://github.com/cleanrules/cleanrules/blob/main/NOTICE) with its
attribution of authorship.
