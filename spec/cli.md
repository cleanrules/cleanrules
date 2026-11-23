# Command line

**Status:** draft, normative for the `cleanrules` command (crate
`crates/cli`). Its tests run the built command; conformance vectors check the
core it calls, and its own tests run the `audit` and `draw` vectors through
it.

`cleanrules` reads documents from files and calls the core: it never
evaluates, audits or draws on its own (`CLAUDE.md`, invariant 2). Every
report and diagram it prints is written by the core, so it is the same text
as in every language, and every error is the core's, with its stable code
(`errors.md`).

```text
cleanrules <command> [options] <file>
cleanrules --help | --version
```

| Command | What it does |
|---|---|
| `validate` | reads a document and says what it holds (*`validate`*) |
| `audit` | audits the rules a document holds (*`audit`*) |
| `draw` | draws a document's diagram (*`draw`*) |
| `help` | prints the help of the command line or of a command |

## Reading documents

*Not checked by vectors: the command line's tests (`crates/cli/tests`) check it.*

Every command reads one document, `<file>`, or standard input when it is
`-`. Its syntax is the one `--syntax` gives (`json`, `yaml`, `text` or
`csv`), or else the file's suffix, compared without case:

| Suffix | Syntax |
|---|---|
| `.json` | `json` (`json-format.md`) |
| `.yaml`, `.yml` | `yaml` (`json-format.md`, *YAML*) |
| `.rules` | `text` (`text.md`) |
| `.csv` | `csv`, a decision table (`tables.md`) |

Standard input is JSON unless `--syntax` says otherwise. A file with another
suffix and no `--syntax` is a usage error. The document is read as a reader
of that syntax reads it, with the default limits (`json-format.md`, *Errors
and limits*), exactly as the agents' `validate` tool reads it (`agents.md`,
*`validate`*): a document that does not read stops the command with the
reader's error. The command reads at most one byte more than the size limit
of the syntax, so a larger file is refused with `limit_exceeded` without
being read whole. A build without the `yaml` feature reads YAML as
`unsupported`.

## Options and output

*Not checked by vectors: the command line's tests (`crates/cli/tests`) check it.*

Options are long names (`--format`, `--fail-on`), each given once, before or
after `<file>`. Every command that writes a report takes `--format`, which
says what it writes on standard output: `text` (the default) or `json`, and
for `draw`, the diagram format. With `--format json`, errors are written as
JSON too (*Errors*), even when the other arguments do not parse. Reports go
to standard output, each ending with a line break; errors go to standard
error.

## Exit codes

*Not checked by vectors: the command line's tests (`crates/cli/tests`) check it.*

| Code | Meaning |
|---|---|
| `0` | the command did its work, and found nothing at or above `--fail-on` |
| `1` | the command did its work, and found what it fails on: for `audit`, a diagnostic at or above the severity of `--fail-on`; for `validate`, a document that does not read |
| `2` | usage error: an unknown command or option, a missing or invalid argument, or a file whose syntax its suffix does not give |
| `3` | the command could not do its work: a file could not be read or written, a document did not read (for `audit` and `draw`), or the core refused the operation (such as auditing a condition, or drawing a decision table in Mermaid); the error says which, with its code |

`--help` and `--version` exit with `0`. A command added later keeps these
meanings: `1` for what it finds (failed cases, unexpected changes), `2` and
`3` for what stops it.

## Errors

*Not checked by vectors: the command line's tests (`crates/cli/tests`) check it.*

An error is written to standard error. In text:

```text
error[format_error]: unknown key "whn"
  --> broken.json at /ruleset/rules/0/whn
  hint: it may come from a newer version of cleanrules
```

- the first line has the error's code and its message;
- `-->` gives the file (`<stdin>` for standard input), followed by the line
  and column (`rules.rules:3:5`) or the pointer (`at /ruleset/rules/0`)
  where the error is, when it has one;
- `hint:` the core's hint, when it has one; `cause:` the error that caused
  it, when there is one.

With `--format json` (or `--format=json`) among the arguments, the error is
one line of JSON: an object with `error`, the error's JSON form (`errors.md`,
*JSON form*), and `file`, the file as it was given (`-` for standard input),
when the error concerns one. JSON Schema: `schemas/v1/cli-error.schema.json`.

The command line has two codes of its own, outside the hierarchy of
`errors.md`, as the C ABI has `invalid_argument` and `internal_error`:

- `usage_error` (exit code `2`): the arguments are wrong, such as an unknown
  command or option, a missing file, an invalid value, or a file whose
  syntax its suffix does not give. Its message is on one line, and its hint
  names the help to read (`see cleanrules audit --help`);
- `io_error` (exit code `3`): a file could not be read or written; its
  message is the operating system's.

Messages and hints are free text for people; the code, the pointer, the line
and the column are stable.

## `validate`

*Not checked by vectors: the command line's tests (`crates/cli/tests`) check it.*

```text
cleanrules validate [--syntax <syntax>] [--format text|json] <file>
```

Reads the document, and writes on standard output whether it reads. In
text, a document that reads gives `<file>: valid <content>`, the content key
of what it holds (`condition`, `rule`, `ruleset`, `decision_table`, `bundle`
or `cases`), followed, when it gives a rule set (`json-format.md`, *A rule
set from a document*), by that rule set's summary in text (`explain.md`,
*Summaries*); one that does not gives `<file>: invalid` followed by its
error in the text of *Errors*. In JSON, it writes the result of the agents'
`validate` tool for the document's text and syntax (`agents.md`,
*`validate`*), the same object an agent gets, valid or not; bytes that are
not UTF-8, which a tool call cannot carry, give `valid` `false` and the
reader's error, as an agent error. The exit code is `0` when the document
reads and `1` when it does not; a file that cannot be read is an
`io_error`, exit code `3`.

## `audit`

*Not checked by vectors: the command line's tests (`crates/cli/tests`) check it.*

```text
cleanrules audit [--syntax <syntax>] [--format text|json]
                 [--fail-on error|warning|info|never]
                 [--max-terms <n>] [--max-steps <n>] [--ruleset-digest] <file>
```

Audits the rules the document holds (`audit.md`, *Documents*): a rule set, a
decision table, a bundle (pointers prefixed with `/bundle`) or a rule
(pointers under `/rule`, without the checks between rules, so a lone rule
reports no gap). A condition or a case file is `unsupported`, exit code
`3`. It writes the audit in text (`audit.md`, *Text*) or as the audit
document in JSON (`audit.md`, *JSON*).

| Option | Default | Meaning |
|---|---|---|
| `--fail-on` | `error` | the exit code is `1` when a diagnostic has this severity or a more serious one (`error` before `warning` before `info`); with `never`, it is `0` whatever the audit finds |
| `--max-terms` | 256 | the limit `max_terms` (`audit.md`, *Limits*) |
| `--max-steps` | 1,000,000 | the limit `max_steps` |
| `--ruleset-digest` | off | the JSON carries the `ruleset_digest` of the rules audited |

Undecided items and checks that are `partial` never fail the command: they
are listed in the report.

## `draw`

*Not checked by vectors: the command line's tests (`crates/cli/tests`) check it.*

```text
cleanrules draw [--syntax <syntax>] [--format mermaid|dot|html|html_fragment]
                [--output <path>] [--max-diagram-size <n>] <file>
```

Draws the document's diagram (`visual.md`): a condition's, a rule's, a rule
set's, a decision table's grid or a bundle's rules. `--format` is the
diagram format, `mermaid` by default, or `html` for a decision table and a
bundle of one, which are drawn in HTML only; a case file has no diagram
(`unsupported`). The diagram goes to standard output, or to the file
`--output` names, created or replaced, ending with a line break.
`--max-diagram-size` raises or lowers the limit `max_diagram_size`
(`visual.md`, *Limits*). Errors are written as text.

## Not yet

*Not checked by vectors: the command line's tests (`crates/cli/tests`) check it.*

- **Several documents** in one command (task T.8b): `--syntax` applies to
  the rules (to both versions in `diff`); case files are read by their
  suffix, as their syntax is JSON or YAML; and `-` may appear once.
- **`test`** (task T.8b): `cleanrules test <rules> [<cases>...]` runs case
  files (`tests.md`, *Running cases*) against the rules, or a bundle's own
  case files, found by the paths it records and checked against their
  digests (`tests.md`, *Case files of a bundle*). It writes the test run
  (`tests.md`, *Test runs*) in its text or as the `test_run` document in
  JSON, with options for the run's limits (`--max-results`, `--max-steps`,
  `--max-explained`: failures beyond it have no `explanation_text`) and
  `--ruleset-digest`, and exits with `1` when a case fails or errs. How a
  partial run (`tests.md`, *Batches and interruption*) maps to the exit
  codes is decided with T.8b.
- **`diff`** (task T.8b, after T.4): `cleanrules diff <old> <new> [<cases>...]`
  compares two versions on the same cases, and exits with `1` on unexpected
  changes.
- **`report`** (task T.8b, after T.5): the report of a bundle or rules with
  their cases, audit and coverage, with `--format text|json|html` and
  `--output`, and `--fail-on` as for `audit`.
- Several files per command (`cleanrules audit rules/*.json`), with one
  report per file and the most serious exit code; until then, a loop in the
  shell.
- `--format yaml` for reports; `draw --input <data>` (draw the explanation of
  a decision on an input) and `draw --decision <file>` (a table's grid with a
  decision's statuses); options for the reading limits (`--max-size`);
  JSON errors for `draw`; colour; shell completions.
