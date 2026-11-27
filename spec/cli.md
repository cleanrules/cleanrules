# Command line

**Status:** draft, normative for the `cleanrules` command (crate
`crates/cli`). Its tests run the built command; conformance vectors check the
core it calls, and its own tests run the `audit`, `draw` and `test_run`
vectors through it.

`cleanrules` reads documents from files and calls the core: it never
evaluates, audits, tests or draws on its own (`CLAUDE.md`, invariant 2). Every
report and diagram it prints is written by the core, so it is the same text
as in every language, and every error is the core's, with its stable code
(`errors.md`).

```text
cleanrules <command> [options] <file>
cleanrules --help | --version
```

`test` reads rules and case files (*`test`*).

| Command | What it does |
|---|---|
| `validate` | reads a document and says what it holds (*`validate`*) |
| `audit` | audits the rules a document holds (*`audit`*) |
| `test` | runs test cases against rules (*`test`*) |
| `draw` | draws a document's diagram (*`draw`*) |
| `help` | prints the help of the command line or of a command |

## Reading documents

*Not checked by vectors: the command line's tests (`crates/cli/tests`) check it.*

Every command reads one document, `<file>`, or standard input when it is
`-`, but `test`, which reads rules and case files (*`test`*). Its syntax is the one `--syntax` gives (`json`, `yaml`, `text` or
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
being read whole, whatever byte the cut falls on (the size is checked before
the bytes are decoded, `json-format.md`, *Errors and limits*); the message
gives the file's size, or says only that it has more bytes than the limit
for standard input, whose size is not known. A build without the `yaml`
feature reads YAML as `unsupported`.

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
| `0` | the command did its work, and found nothing it fails on (for `audit`, nothing at or above `--fail-on`; for `test`, every case ran and passed) |
| `1` | the command did its work, and found what it fails on: for `audit`, a diagnostic at or above the severity of `--fail-on`; for `validate`, a document that does not read; for `test`, a case that failed or erred, a partial run, or a run without cases |
| `2` | usage error: an unknown command or option, a missing or invalid argument, a file whose syntax its suffix does not give, or standard input (`-`) given twice |
| `3` | the command could not do its work: a file could not be read or written (a case file that cannot be opened included), a document did not read (for `audit`, `draw` and the rules of `test`), or the core refused the operation (such as auditing a condition, or drawing a decision table in Mermaid); the error says which, with its code |

`--help` and `--version` exit with `0`. For `test`, `0` means that every case
ran and passed (*`test`*). A command added later keeps these
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

A **note** says something about work that was done, on standard error, and
does not change the exit code: `note[<code>]: <message>` in text, or with
`--format json` one line of JSON, an object with `note`, itself an object
with the note's `code` and `message` (the same JSON Schema). The only note
today is `no_cases`, a test run without cases (*`test`*); more may be added,
and a consumer shows a note of a code it does not know by its message.

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

## `test`

*Not checked by vectors: the command line's tests (`crates/cli/tests`) check
it, and run the `test_run` vectors through it.*

```text
cleanrules test [--syntax <syntax>] [--format text|json]
                [--max-results <n>] [--max-steps <n>] [--max-explained <n>]
                [--ruleset-digest] <rules> [<cases>...]
```

Runs test cases against rules (`tests.md`, *Running cases*) and writes the
test run: in its text (`tests.md`, *Text*) or as the `test_run` document in
JSON (`tests.md`, *Test runs*), whatever its results.

**The rules.** `<rules>` is read as every command reads its `<file>`
(*Reading documents*), with `--syntax` when it is given. A rule set and a
decision table are tested as they are, a bundle as its rules, and a rule as
the rule set of that one rule with the default settings (`json-format.md`,
*A rule set from a document*); a condition and a case file are a
`format_error`, exit code `3`.

*Rationale.* `audit` and `draw` refuse a condition and a case file with
`unsupported`, and `test` with `format_error`: `test` asks a document for
the rule set it gives, which `json-format.md` (*A rule set from a document*)
defines with `format_error` for a document that holds no rules, as the C
ABI's `cr_ruleset_read` has always given, while auditing and drawing are
operations that one kind of document does not support. Making the two the
same would change that existing contract, so they stay as they are.

**The case files.** Each of `<cases>` is a case file (`tests.md`, *Case
files*), read in the syntax its suffix gives, compared without case: `.json`
for JSON, `.yaml` or `.yml` for YAML. Another suffix is a usage error, and
`--syntax` does not apply: it is the rules'. `-` reads a case file from
standard input, in JSON; it holds one document, so `-` appears at most once
among `<rules>` and `<cases>`. The files run in the order given, each with
the path it is given by, written with `/` between its segments on every
system (`\` on Windows becomes `/`) and without a leading `./`, so that the
runs of the same files line up across systems and invocations (`tests.md`,
*Identity*): a file given twice gives the second a `format_error` result.
The file from standard input has no path. They are read with the default
limits, which the test run records in its `engine`. This suffix rule is the
command line's and stricter than the core's, which reads any path that does
not end in `.yaml` or `.yml` as JSON (`tests.md`, *Running cases*): a
command line can refuse a misnamed file before it reads anything.

Without `<cases>`, a bundle runs the case files it lists (`tests.md`, *Case
files of a bundle*): each is opened at its `path`, relative to the folder of
the bundle's file (the current folder when the bundle comes from standard
input), checked against its digest and read with the limits the bundle
records, and the test run carries the bundle's `id` and `revision`. A file
that is not there is an `io_error` whose hint names the bundle and the
pointer of the path that lists it (`/bundle/cases/<i>/path`). Rules
that are not a bundle need case files: without them the command is a usage
error, found once the rules are read (every other usage error is found
before any file is read). With `<cases>`, a bundle's rules run on those
files instead of its own, as any rules do, and the test run has no `bundle`.

**Files that cannot be run.** A case file that cannot be opened, given on
the command line or listed by a bundle, is an `io_error`, exit code `3`, and
no case runs. A file that opens but does not read as a case file is that
file's result, an `error` (`tests.md`, *Running cases*), and the run goes on:
its JSON or YAML is invalid, it holds another document, it repeats a path,
or a bundle's file does not match its digest (`decision_mismatch`). As every
document is, a case file is loaded up to one byte more than the size limit
of its syntax (for a bundle's file, the larger of the limit the bundle
records and the default one, so that a file the bundle's limit refuses is
still checked against its digest first): a larger file gives the result
`limit_exceeded` without being loaded whole, so its entry in `files` has no
`digest`.

*Rationale.* A file that cannot be opened is a wrong path in the command or
in the bundle, an error of the set-up rather than of the rules, and the core
has no code for it (`io_error` is the command line's own), so it stops the
command as it stops every command. What the core reads, it reports as a
result.

| Option | Default | Meaning |
|---|---|---|
| `--max-results` | none | the run limit `max_results` (`tests.md`, *Batches and interruption*) |
| `--max-steps` | none | the run limit `max_steps` |
| `--max-explained` | 5 | the run limit `max_explained`: failed cases beyond it have no `explanation_text` (`tests.md`, *Explaining failures*) |
| `--ruleset-digest` | off | the JSON carries the `ruleset_digest` of the rules tested |

**Exit code.** `0` when every case ran and passed: the run is complete, it
has at least one result, and every result is `passed`. Otherwise `1`: a case
failed or erred, a case file did not read, the run is partial, or it had no
cases.

- A **partial run**, interrupted by `--max-results` or `--max-steps`, exits
  with `1` even when every result it gave passed: the cases it did not run
  are not known to pass, so a CI job does not take a bounded run for a
  passing one. Its text ends with `Partial run: interrupted by <reason>, <n>
  not run`. To look at the first results only, read the report and leave the
  exit code aside.
- A run **without cases** (case files without items, or a bundle that lists
  none) exits with `1`: a test step that checked nothing does not pass
  unnoticed. Its text is the header and `0 results: 0 passed, 0 failed, 0
  errors`, and the note `no_cases` on standard error says why the run
  fails (*Errors*); standard output holds the test run only.

## Not yet

*Not checked by vectors: the command line's tests (`crates/cli/tests`) check it.*

- **`diff`** (task T.8b, after T.4): `cleanrules diff <old> <new> [<cases>...]`
  compares two versions on the same cases (`comparison.md`), or two bundles
  on their own case files, and writes the comparison in its text or as the
  `test_comparison` document in JSON, with the run limits of `test` for both
  sides and `--max-explained` for the changes explained. It refuses two
  bundles in conflict (`decision_mismatch`, `3`). It reads its documents as
  `test` does: `--syntax` applies to both versions, case files go by their
  suffix, and `-` appears at most once. The recommended exit codes: `1` when a change is unexpected (`summary.unexpected`)
  or the comparison is not complete (`comparison.md`, *Joining*: a side
  partial, or items not compared); `0` for added and removed cases, which
  `--fail-on changed` can make fail; and when no item was compared at all, a
  note on the standard error and `1`, as `test` does for a run without cases
  (`no_cases`).
- **`report`** (task T.8b, after T.5): the report of a bundle or rules with
  their cases, audit and coverage, with `--format text|json|html` and
  `--output`, and `--fail-on` as for `audit`.
- Several files per command (`cleanrules audit rules/*.json`), with one
  report per file and the most serious exit code; until then, a loop in the
  shell. `test` will not follow that form: its positional arguments after
  the rules are already the case files of one run.
- For `test`: writing each result as it comes (the run is incremental, the
  command writes the test run when it ends), running only some cases or
  files, going on when a case file cannot be opened (as a result of its
  own), a cancellation by a deadline or a signal,
  `--pass-with-no-cases`, for a run without cases that should pass, and an
  option to gather the run's statistics and coverage (`tests.md`,
  *Statistics*), which the test run then carries.
- `--format yaml` for reports; `draw --input <data>` (draw the explanation of
  a decision on an input) and `draw --decision <file>` (a table's grid with a
  decision's statuses); options for the reading limits (`--max-size`);
  JSON errors for `draw`; colour; shell completions.
