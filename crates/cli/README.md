# cleanrules command line

`cleanrules` audits, tests, validates and draws rule sets and decision tables
of the [cleanrules](https://github.com/cleanrules/cleanrules) rules engine,
with exit codes made for CI. Every report, diagram and error it prints is written by the
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
cleanrules test     [--syntax <syntax>] [--format text|json] [--max-results <n>]
                    [--max-steps <n>] [--max-explained <n>] [--ruleset-digest]
                    <rules> [<cases>...]
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
- **`test`** runs test cases ([`spec/tests.md`](https://github.com/cleanrules/cleanrules/blob/main/spec/tests.md))
  against a rule set, decision table, bundle or rule: each case's input is
  evaluated, never running an action, and checked against what the case
  expects (the rules selected, the outputs, the rules left unknown, or an
  error). Failed cases are listed with what differs and the explanation of
  the decision (the first five, or `--max-explained`). Case files are JSON or
  YAML; without them, a bundle runs the case files it lists, checked against
  their digests. `--format json` gives the test run document, with its JSON
  Schema `spec/schemas/v1/test_run.schema.json`.
- **`draw`** writes the diagram of a condition, rule, rule set, decision table
  or bundle ([`spec/visual.md`](https://github.com/cleanrules/cleanrules/blob/main/spec/visual.md)): Mermaid by default,
  HTML for decision tables.

`<file>` (and the `<rules>` of `test`) is read in the syntax its suffix gives
(`.json`, `.yaml`/`.yml`, `.rules` for the rule syntax, `.csv` for decision
tables), or the one `--syntax` gives; `-` reads standard input, JSON unless
`--syntax` says otherwise. Case files go by their suffix, `.json`, `.yaml` or
`.yml`, and `-` may be given once; a case file's path is recorded with `/`
and without a leading `./`. A run without cases fails, with the note
`no_cases` on standard error.

## Exit codes

| Code | Meaning |
|---|---|
| `0` | done, nothing found at or above `--fail-on`; for `test`, every case ran and passed |
| `1` | `audit` found a diagnostic at or above `--fail-on` (default `error`), `validate` a document that does not read, or `test` a case that failed or erred, a case file that does not read, a partial run (`--max-results`, `--max-steps`) or no cases at all |
| `2` | usage error |
| `3` | a file could not be read or written (a case file that is not there included), a document did not read, or the operation is not supported for it (such as auditing a case file) |

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
$ cleanrules test age.rules cases/age.json
Test run of a rule set: 2 rules, hit policy unique, logic boolean
FAILED  cases/age.json  "child"
          selected: expected ["adult"], actual []
          output: expected {"band":"adult"}, actual null
          explanation:
            HIT POLICY unique: selected no rule
            [ ] adult (not fired): age >= 18
                - age >= 18 is false (age = 10); short by 8
            [ ] senior (not fired): age >= 65
                - age >= 65 is false (age = 10); short by 55
ERROR   cases/age.json  "senior"
          hit_policy_error: 2 rules fired under the unique hit policy
          hint: make the conditions mutually exclusive, or use first or priority
4 results: 2 passed, 1 failed, 1 error
$ echo $?
1
$ cleanrules test bundle.json --format json > test-run.json
$ cleanrules audit rules.json
error[format_error]: unknown key "whn"
  --> rules.json at /ruleset/rules/0/whn
  hint: it may come from a newer version of cleanrules
```

## In CI

[`docs/examples/ci/cleanrules-audit.yml`](https://github.com/cleanrules/cleanrules/blob/main/docs/examples/ci/cleanrules-audit.yml)
is a GitHub Actions workflow that audits every rule file of a repository and
runs the case files found next to it under `cases/`, and fails on errors or
failed cases, keeping each JSON report as an artifact. Until binaries are
released, it builds `cleanrules` from source.

## Licence

Apache License 2.0, created by Jose L. Salmeron: redistributions keep the
[NOTICE](https://github.com/cleanrules/cleanrules/blob/main/NOTICE) with its
attribution of authorship.
