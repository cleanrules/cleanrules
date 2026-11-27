# Test cases

**Status:** draft, format version `1` (tasks T.1, T.2, T.3 and T.10). JSON Schemas:
[`schemas/v1/cases.schema.json`](schemas/v1/cases.schema.json) for case files
and [`schemas/v1/test_run.schema.json`](schemas/v1/test_run.schema.json) for
the results of running them.

A **test case** gives rules one input and says what evaluating it must give:
the rules selected, their outputs, the rules whose condition is unknown, or
the error. A **case file** is a document that holds a list of cases. Running
the cases of a case file against two versions of the rules, and comparing what
each gives, is how a user tests and compares those versions; a bundle
(`bundles.md`, *Cases*) names the case files that go with its rules.

Cases are run with `evaluate`, never with `run`: a test never calls an action
(`rules.md`, *Actions*), so it gives the same result whether the rules' actions
are bound or not, and it can be run any number of times. Running cases gives a
result per case and a summary (*Running cases*, *Test runs*), one result at a
time and with limits (*Batches and interruption*), and, when asked, what the
cases made each rule and condition do and which parts of the rules they never
reached (*Statistics*); comparing two versions on the same cases (task T.4)
builds on it.

```json
{
  "version": 1,
  "cases": {
    "description": "the cases of the discount rules",
    "items": [
      {
        "id": "vip-gets-15",
        "description": "a customer of more than five years gets 15%",
        "input": { "years": 7 },
        "expect": {
          "selected": ["vip"],
          "output": { "discount": { "$decimal": "0.15" } }
        }
      },
      {
        "id": "years-unknown",
        "input": {},
        "expect": { "selected": [], "output": null, "unknown": ["vip"] }
      }
    ]
  }
}
```

## Case files

A case file is a document of the JSON format (`json-format.md`, *Documents*)
with the content key `"cases"`, added to version 1 (*Versioning* there): older
readers reject it with `format_error`. Its content is an object:

| Key | Required | JSON type | Default |
|---|---|---|---|
| `description` | no | string: what the cases are about | none |
| `metadata` | no | object: key → data value, as a rule's (`json-format.md`, *Rules*) | `{}` |
| `items` | yes | array of cases (*Cases*), in the order they are run; it may be empty | |

`metadata` follows the rules of a rule's metadata: its keys are taken
literally, and its values are data values, read as data is. Writers write the
keys in the order of the table, and omit `description` when there is none and
`metadata` when it is empty.

A case file is read and written in JSON and YAML; the rule syntax and CSV
cannot hold one (`unsupported`). The content key is what marks a case file, as
for every document: case files have no file extension of their own, and are
`.json` or `.yaml` files like other documents. Within a bundle, they are named
by paths of the form that `bundles.md` (*Cases*) gives, such as
`cases/discounts.json`.

A case file holds no rules, so it gives no rule set (`json-format.md`, *A rule
set from a document*: `format_error`), and it has no diagram (`visual.md`:
`unsupported`). The same case file can be run against any rules.

## Cases

A case is an object with these keys:

| Key | Required | JSON type | Default |
|---|---|---|---|
| `id` | yes | string, not empty: the case's id (*Identity*) | |
| `description` | no | string | none |
| `input` | yes | a data value: the input of the evaluation (*Inputs*) | |
| `expect` | no | object: the expectation (*Expectations*) | `{}` |

Writers write the keys in the order of the table, omit `description` when
there is none, and omit `expect` when it is empty.

### Identity

A case's `id` identifies it: in the results of running a case file, and across
versions of the rules, when two versions run the same case and their results
are compared. As a rule's id (`rules.md`, *Identity*), it is any non-empty
string, taken literally, compared exactly and never normalized.

- Within a case file, **ids are unique**: a case whose id repeats an earlier
  case's id is a `format_error` at the second one's `id`.
- An id is required, unlike a rule's: a case has no name, and its position
  changes when cases are added or removed, so the id is what results and
  comparisons name it by.
- Across the case files of a bundle, a case is identified by the file's
  `path` (`bundles.md`, *Cases*) and its `id`: two files may hold cases with
  the same id.

### Inputs

`input` is the input that the rules evaluate, written as data is in the JSON
format (`json-format.md`, *Values*): usually a map of fields, and any data
value. It is read as any input given as JSON is (`data-model.md`): an int
outside the signed 64-bit range reads as null with the reason `int_overflow`,
and an opaque value is a `format_error`. A case runs as `evaluate` runs on an
input given as JSON, so `max_values`, which bounds native data only
(`json-format.md`, *Errors and limits*), does not apply.

## Expectations

`expect` says what evaluating the input must give. Each key checks one part of
the result, and a key that is absent checks nothing:

| Key | JSON type | Checks |
|---|---|---|
| `selected` | array of strings: rules by effective id (*Rules in expectations*) | the selected rules, in selection order |
| `output` | object (payload key → data value), or `null` | the single output: the winner's, or the aggregate |
| `outputs` | array of objects (payload key → data value) | the outputs of the selected rules, in selection order |
| `unknown` | array of strings: rules by effective id | the rules whose condition is unknown |
| `error` | object: `code`, an error code (*Errors*) | that evaluation fails with that error |

An empty expectation, or none, checks only that evaluation does not fail: a
case without expectations still gives a result, which comparing two versions
of the rules (task T.4) uses. `error` is given alone: with any other key, it
is a `format_error` at the expectation's `error`.

`output` and the items of `outputs` are objects whose keys are payload keys,
taken literally as those of a payload are (`json-format.md`, *Rules*: a key
that starts with `$` is an ordinary key here), and whose member values are
data values (*Values* there). Unlike an input, an **expected value is never out
of range**: an int outside the signed 64-bit range, or a decimal that does not
fit (`data-model.md`, *Decimals*), is a `format_error` at that value, as in a
constant, rather than a null: a case that expects a value must say which. To
expect a null, write `null`.

`unknown` is a set: its order is not significant, and writers write it sorted
by Unicode code point, as tags are (`json-format.md`, *Rules*). `selected` is a
list: its order is the selection order.

*Rationale.* These are exactly what a decision gives without its result trees
(`rules.md`, *Evaluation*, *Outputs*), keeping each rule's status
(`explain.md`, *Rule status*): which rules were selected, what they output,
which were unknown, or the error. So a run that keeps no result trees (task
T.6), but keeps each rule's status, can check every expectation, and a failed
one can be explained by evaluating the case again. Refusing an expected value
out of range, rather than reading it as null, keeps a case from passing
against a null it did not write; and it can be relaxed later without breaking
any file, which the reverse could not.

### Rules in expectations

`selected` and `unknown` name rules by their **effective id** (`rules.md`,
*Identity*): the id of a rule that has one, and its name otherwise. So
`selected` here holds effective ids, whereas the `selected` of a decision
document holds names (`json-format.md`, *Decisions*). A rule set gives each of
its rules' effective id, so a decision is checked through the rule set that
made it. A rule renamed with its old name as its id stays the same rule in its
cases.

Each string is not empty, and a rule is named once in each array: an empty
string, or one that repeats an earlier one in the same array, is a
`format_error` at that string. Unlike tags, whose repeats readers drop, a
repeated id is an error, because a rule can be selected or unknown only once,
and a repeat is a mistake. A string that names no rule of the rules being
tested is not an error of the case file, which is read without rules: that
expectation does not match (*Matching*).

### Errors

`error` is an object with one key, `code`: one of the codes that evaluating an
input given as JSON can raise (`errors.md`, *Codes*; `rules.md`, *Strict mode
and errors*):

| Code | Raised when |
|---|---|
| `missing_field` | in strict mode, a field's path cannot be resolved |
| `type_mismatch` | in strict mode, values cannot be compared or used as they are |
| `hit_policy_error` | the hit policy cannot decide: `UNIQUE` with more than one rule fired, `ANY` with different outputs, an aggregate over a value that is not a number |

Any other string is a `format_error` at `code`, and so is a missing `code`;
any other key is a `format_error` at that key. `evaluation_error` is not one:
on an input given as JSON, strict mode raises its specific codes. Strict mode
is a setting of the rules, not of the case (`json-format.md`, *Rule sets*): a
case expects a strict error from rules that are strict.

## Matching

A case is run by evaluating the rules being tested on its `input`, as
`evaluate` does (`rules.md`, *Evaluation*). Its result is one of:

- **error**: evaluation raised an error and the case expects none (it has no
  `error` key). Nothing else is checked.
- **failed**: otherwise, when any key of the expectation does not match.
  Every key is checked, in the order of the *Expectations* table, and a
  failure lists every key that does not match, in that order, each with what
  was expected and what was given.
- **passed**: otherwise.

When the case has an `error` key, the result is never `error`: evaluation that
succeeds, or that fails with another code, is a `failed` `error` key. When it
has none, evaluation that succeeds is checked key by key. Each key matches as
follows:

- **`selected`** matches when the rules the decision selects are exactly
  these, in this order. `[]` expects that no rule is selected.
- **`output`** is the decision's `output` (`rules.md`, *Outputs*): the
  winner's output under a single-hit policy, and the aggregate under
  `COLLECT` with an aggregate; null when no rule was selected. `null` matches
  when it is null. An object matches when it is not null and, for every key of
  the object, the output has that key, with an equal value (*Values*). The
  output's other keys are not checked, so `{}` matches whenever a rule was
  selected. A decision with no single output (`RULE_ORDER`, and `COLLECT`
  without an aggregate) matches no `output`: the key fails with the reason
  "the decision has no single output (`<policy>`)"; expect `outputs` instead.
- **`outputs`** matches when the decision has as many outputs as the array
  has items, and each item matches the output at its position as an object
  under `output` does. It works under every hit policy: under a single-hit
  policy, a decision has one output or none.
- **`unknown`** matches when the rules whose condition is unknown are exactly
  these, in any order: under Kleene logic, those whose condition's truth value
  is `UNKNOWN` (`explain.md`, *Rule status*: `unknown`). `[]` expects that every
  condition is known, so that no rule was left undecided by missing or null
  data.
- **`error`** matches when evaluation fails with exactly that code. The
  error's other fields are not compared.

### Values

An expected value matches an actual value when both have the same canonical
bytes (`json-format.md`, *Canonical bytes*, *Every type*): the same type and
the same value as the canonical form writes it.

- An int, a float and a decimal never match each other: `1`, `1.0` and
  `{"$decimal": "1"}` are three different values.
- Decimals match with their scale: `{"$decimal": "0.15"}` does not match the
  decimal `0.150`. Decimal arithmetic is exact (`ast.md`, *Decimal
  arithmetic*), so every result has one exact value and one scale to expect.
- Floats match exactly, and so does the sign of a zero: the core gives the
  same float on every platform. A float that holds an integer is written
  `1.0` or `{"$float": 1}`: a bare `1` is an int, and does not match it.
- Datetimes match with their offset and zone: the same instant in another zone
  does not match.
- Lists match item by item, in order; maps key by key, whatever their order.
- A null matches a null, whatever its reason: reasons are not compared.

*Rationale.* A test pins what the rules give, exactly as a decision document
records it: the type of a number and the scale of a decimal show wherever an
output is printed, stored or compared, and the same comparison is the one
readers use for recorded values (`json-format.md`, *Reading a decision*) and
conformance runners use for every vector. A tolerance, or comparing numbers
across types, would make a passing case say less than the decision does.

## Reading and writing

A case file is read as every document is (`json-format.md`, *Errors and
limits*): size, nesting and syntax; `version`; then the shape in pre-order:
the keys of each object in the order of its table and then its unknown keys by
Unicode code point (so the file's `description` and `metadata` before its
`items`, and the cases in order), the members of a map, of metadata and of an
expected output by key, by code point; then the top level's other keys. Every
error in a case file is a `format_error` at its pointer, those in its values
included (`json-format.md`, *Values*), except the size and nesting limits
(`limit_exceeded`): a case file has no construction rules. The checks that are
not in a table come where they are met:

- an `id` that is empty or repeats an earlier case's, at `…/id`, when the
  case's `id` is read;
- an empty or repeated string in `selected` or `unknown`, at that string;
- an expected value out of range, at that value;
- an `error` given with another key, at `…/expect/error`, after the
  expectation's keys.

A case file is bounded by the document size limit (16 MiB by default), which
a caller can raise; large sets of cases can also be split into several files,
which a bundle lists in order.

Writers write the canonical form: the keys in the order of their tables,
values in their canonical form, the members of maps, of metadata and of
expected outputs sorted by key, `unknown` sorted, and the optional keys
omitted as *Case files* and *Cases* say. A case file has canonical bytes and
a digest as every document has (`json-format.md`, *Canonical bytes*,
*Digests*). A bundle does not use them: it records the digest of a case
file's bytes as stored (`bundles.md`, *Cases*), so that it names one exact
file.

## Running cases

A **run** evaluates rules on the cases of one or more case files and gives
one **result** per case. The rules are a rule set, a decision table (as the
rule set it compiles to, `tables.md`, *Compiling*) or the rules of a bundle.
Each case is matched as *Matching* says: it is evaluated with `evaluate`
(`rules.md`, *Evaluation*) on its own `input`, without actions and without
anything carried over from another case, and its result is `passed`,
`failed` or `error`.

Results come in **run order**: the case files in the order they are given
(a bundle's in the order of its `cases`), and the cases of each file in the
order of its `items`. A case is identified by its `id` and, when its file has
one, the file's `path` (*Identity*); the same rules and case files give the
same results, in the same order, on every platform.

Within a run, the path and the id of a case are unique together: a file
whose path repeats an earlier file's path, and a second file without a path,
give one result each, a `format_error` without a pointer and without an
`id`, and none of their cases runs.

An error is the result of the case that raised it, and the run goes on with
the next case: a case never stops the run. A **file that cannot be read** as
a case file is one result with the status `error`, its `path` and its error,
and no `id`; the run goes on with the next file. The core never opens files:
the caller gives the bytes of each case file, and a file the caller cannot
open is the caller's error: a command line reports it as it reports any
file it cannot open, or, to go on, gives the run the file with its own
error, which is then that file's result. Reading the bytes is reading a document
(*Reading and writing*), in YAML when the file's path ends in `.yaml` or
`.yml` (in any ASCII case) and in JSON otherwise, and a document with another
content key, such as a rule set, is a `format_error` at the root (`""`).

The core gives these three things the same names in every language: a
**case file reference** (`CaseFileRef`) is a bundle's reference to a case
file, its path and digest (`bundles.md`, *Cases*); the **cases**
(`Cases`) are the content of a case file document; and a **case file**
(`CaseFile`) is one file given to a run: its path when it has one, the
digest of the bytes it was read from when it was, and its cases or the error
that keeps it from being read.

### Case files of a bundle

Running a bundle runs its rules on the case files it lists (`bundles.md`,
*Cases*), in its order, each with its `path`. Before a file is read, its
bytes are checked against the digest the bundle records for it: when they
differ, the file's result is a `decision_mismatch` at
`/bundle/cases/<i>/digest`, `<i>` its position in the bundle's `cases`, and
none of its cases runs, since they are not the ones the bundle was made with.
Then the file is read as above, under the limits the bundle records
(`bundles.md`, *Engine*: they are the ones that can change a result), and
the test run records those limits (*Test runs*, `engine`).

*Rationale.* `decision_mismatch` is the code that says that a record does not
match what it is checked against, as for a bundle's `ruleset_digest`
(`bundles.md`, *Rules and their digest*); the pointer says which file of the
bundle it is.

### Explaining failures

A failed case whose evaluation gave a decision is **explained**: its result
holds the text of the decision's explanation (`explain.md`, *Text*), which
says why each rule was or was not selected. A run explains the first
`max_explained` failed cases that have a decision, in run order (5 unless
the caller sets it; 0 explains none), so that a run over many cases keeps few
explanations. A case whose result is `error`, and a failed `error` key whose
evaluation raised another error, have no decision to explain: their error
says what happened.

A run keeps no result trees: each case's decision is reduced to what its
expectation checks (the selected rules, the outputs, the single output and
the unknown rules, which are each rule's status, `explain.md`, *Rule
status*), and explained only when it is one of the failures explained, before
the next case is evaluated. A failure that is not explained can be explained
later by evaluating its case again: evaluation is deterministic, so this
gives the same decision and the same explanation.

### Batches and interruption

A run is **incremental**: it gives its results one at a time, in run order,
each before the next case is evaluated, so a caller can show, store or drop
each result as it comes, and the run itself keeps only the counts of its
summary (*Test runs*), and of its statistics when they are asked for
(*Statistics*), between results.

A run ends when every case has a result, or when it is **interrupted**: then
it gives no more results, and it is **partial**. Before each item (a case, or
a file that cannot be read, which gives one result), and only when one
remains, the run is interrupted when:

| Reason | When |
|---|---|
| `max_results` | `max_results` results have been given, those of files that cannot be read included |
| `max_steps` | evaluating the next case would take the steps past `max_steps`: running a case costs one step per rule of the rules being tested (each rule's condition is evaluated once), and a file that cannot be read costs none |
| `cancelled` | the caller cancels the run: by a callback, which the run asks before each item, or by stopping it between two results, which includes asking for its test run before it is finished |

The checks are made in this order. Both limits are unbounded unless the
caller sets them, and each is a count of at least 0. A run that has given its
last result is complete, never partial, whatever the limits or the callback
would say.

An interruption is not an error, and it is not a case's result: a case
either has its full result, or it is not run. So the results of a partial
run are the first results of the full run with the same rules and case files,
the same in every way, and its summary says why it stopped and how many
results it did not give (`remaining`), among the case files given to the run:
one per case not run, and one per file not reached that gives one result
(it cannot be read, or it repeats a path). A caller that stops taking
results and asks for the test run gets a partial run: a run is never
reported complete before its last result.

A run needs no threads: it is never interrupted while it evaluates a case,
and a callback that a caller gives (one that reads a flag, or checks a
deadline) is asked between cases only.

*Rationale.* Limits count results and steps rather than time, so a partial
run is deterministic: the same limits give the same results on every
platform. A deadline is the caller's, as a cancellation.

## Test runs

A **test run** document is the JSON form of a run's results and summary, with
the content key `"test_run"` (`json-format.md`, *Documents*). Like an audit,
it is output only: readers reject it with `unsupported`.

```json
{
  "version": 1,
  "test_run": {
    "source": "ruleset",
    "hit_policy": "priority",
    "logic": "boolean",
    "engine": {
      "name": "cleanrules",
      "version": "1.0.0",
      "limits": { "max_size": 16777216, "max_nesting": 1024, "max_depth": 256, "max_path_segments": 256, "max_table_size": 2097152, "max_values": 1000000 }
    },
    "files": [
      { "path": "cases/discounts.json", "digest": "sha256:37517e5f3dc66819f61f5a7bb8ace1921282415f10551d2defa5c3eb0985b570" },
      { "path": "cases/strict.json", "digest": "sha256:0f2b6c5d8e8c1b7a3f6e4d2c9b8a7f6e5d4c3b2a1f0e9d8c7b6a5f4e3d2c1b0a" }
    ],
    "limits": { "max_explained": 5 },
    "summary": { "total": 3, "passed": 1, "failed": 1, "error": 1, "partial": false },
    "results": [
      { "path": "cases/discounts.json", "id": "vip-gets-15", "status": "passed" },
      {
        "path": "cases/discounts.json",
        "id": "loyal-gets-5",
        "status": "failed",
        "mismatches": [
          { "key": "selected", "expected": ["loyal"], "actual": ["vip"] },
          { "key": "output", "expected": { "discount": { "$decimal": "0.05" } }, "actual": { "discount": { "$decimal": "0.15" } } }
        ],
        "explanation_text": "HIT POLICY priority: selected vip\n..."
      },
      {
        "path": "cases/strict.json",
        "status": "error",
        "error": { "code": "format_error", "message": "two cases have the id \"a\"", "hint": "give each case of a file its own id", "pointer": "/cases/items/1/id" }
      }
    ]
  }
}
```

| Key | Required | JSON type |
|---|---|---|
| `source` | yes | `"ruleset"` or `"decision_table"`: what was tested (a bundle's rules are one or the other) |
| `hit_policy` | yes | the policy: a rule set's, or a decision table's (with `output_order`) |
| `aggregate` | with an aggregate | the aggregate |
| `logic` | yes | the logic |
| `strict` | when `true` | `true` |
| `ruleset_digest` | when asked for | string: the rule set digest of the rules tested, or of the rule set the table compiles to (`json-format.md`, *Rule set digests*), as decisions and audits carry it |
| `engine` | yes | object: the engine that ran the cases, as a bundle records one (`bundles.md`, *Engine*): its `name`, its `version` and the `limits` its case files were read with (a bundle's own, for a bundle's cases) |
| `bundle` | for a bundle's cases | object: the bundle's `id` and `revision` (`bundles.md`, *Identifier and revision*) |
| `files` | yes | array, in run order, of the case files given to the run, each an object with its `path` when it has one and the `digest` of its bytes (`json-format.md`, *Digests*) when it was read from bytes |
| `limits` | yes | object: `max_explained`, and `max_results` and `max_steps` when they are set, ints |
| `summary` | yes | object: the summary (below) |
| `results` | yes | array of results, in run order (below) |
| `statistics` | when asked for | object: the run's statistics and coverage (*Statistics*, *JSON*) |

The **summary**:

| Key | Required | JSON type |
|---|---|---|
| `total` | yes | int: the number of results |
| `passed` | yes | int: the results `passed` |
| `failed` | yes | int: the results `failed` |
| `error` | yes | int: the results `error`, files that cannot be read included |
| `partial` | yes | boolean: whether the run was interrupted |
| `interrupted` | when `partial` | object: `reason` (`max_results`, `max_steps` or `cancelled`) and `remaining`, an int |

`results` holds the results the run gave, so its length is `total`; a
consumer does not rely on it, since a later version may let a test run keep
only some of its results (*Not yet*), and counts with the summary.

A **result**:

| Key | Required | JSON type |
|---|---|---|
| `path` | when its file has one | string: the case file's path |
| `id` | for a case | string: the case's id; absent for a file that cannot be run |
| `status` | yes | `"passed"`, `"failed"` or `"error"` |
| `mismatches` | for `failed` | array of mismatches, in the order of the *Expectations* table |
| `error` | for `error` | the error, in its JSON form (`errors.md`, *JSON form*). Its `pointer` points into the case file at `path` (a case file's `format_error`), but for a `decision_mismatch`, which points into the bundle (`/bundle/cases/<i>/digest`) |
| `explanation_text` | for a failure explained | string: the text of the decision's explanation (*Explaining failures*). The key `explanation` is kept for the explanation's JSON form, as other documents hold it |

A **mismatch** is a key of the expectation that does not match:

| Key | Required | JSON type |
|---|---|---|
| `key` | yes | `"selected"`, `"output"`, `"outputs"`, `"unknown"` or `"error"` |
| `expected` | yes | what the case expects under that key, as a case file writes it (`unknown` sorted, `error` as `{"code"}`) |
| `actual` | but for a decision without a single output | what was given, in the same form (below) |
| `reason` | for a decision without a single output | string: "the decision has no single output (`<policy>`)", with the run's `hit_policy` (*Matching*) |

What was given is: for `selected`, the effective ids of the selected rules, in
selection order; for `output`, the decision's output with all its keys, or
`null`; for `outputs`, the outputs of the selected rules, in selection order,
each with all its keys; for `unknown`, the effective ids of the rules whose
condition is unknown, sorted by Unicode code point; for `error`, the error
evaluation raised, in its JSON form, or `null` when it raised none. Outputs
are objects whose keys are payload keys, taken literally, and whose values
are in the JSON encoding of values (`json-format.md`, *Values*), as expected
outputs are written (*Expectations*).

Writers write the keys in the order of their tables, and every array even
when it is empty. Errors are written in their JSON form, whose `message` and
`hint`, like an explanation's text, are the core's and the same in every
language; conformance vectors compare errors by code and compared fields
(`errors.md`, *Comparing errors*). Within format version 1, new optional keys,
new reasons of interruption and new mismatch keys (as new expectation keys
are added) may be added: consumers ignore the keys and reasons they do not
know, and show a mismatch of a key they do not know from its `expected`,
`actual` and `reason` as they are. The statuses are a closed set, as an
audit's severities, so that a command line can map them to its exit codes.

### Text

*Not checked by vectors: the core's snapshot tests fix it.*

The text of a test run, as the command line prints it, is generated by the
core:

```text
Test run of a rule set: 3 rules, hit policy priority, logic boolean
FAILED  cases/discounts.json  "loyal-gets-5"
          selected: expected ["loyal"], actual ["vip"]
          output: expected {"discount":{"$decimal":"0.05"}}, actual {"discount":{"$decimal":"0.15"}}
          explanation:
            HIT POLICY priority: selected vip
            ...
ERROR   cases/strict.json
          format_error at /cases/items/1/id: two cases have the id "a"
          hint: give each case of a file its own id
3 results: 1 passed, 1 failed, 1 error
```

- A header with what was tested, its number of rules and its configuration,
  as in an audit (`audit.md`, *Text*).
- One block per result that is not `passed`, in run order: its status in
  capitals (`FAILED`, `ERROR`), its path and its id, quoted as a JSON
  string, when it has them (`(a case file without a path)` when it has
  neither); then,
  indented, each mismatch: its key, what was expected and what was given in
  compact JSON (or the reason, when nothing was given), and for `error` the
  code expected and the error given, as its code and message, or
  `no error`; the explanation, indented further, when there is one; and the
  error, as its code, its location (`at` a pointer, `the root` for `""`, or
  `at line <l>, column <c>`) and its message, with its hint on the next line
  when it has one.
- The summary: the number of results and how many of each status; and, for a
  partial run, `Partial run: interrupted by <reason>, <n> not run`.
- The statistics, when the run has them (*Statistics*, *Text*).

## Statistics

A run can also count what its cases made the rules do: how many times each
rule was selected and fired, how many times each of its conditions was
`TRUE`, `FALSE` or `UNKNOWN`, and so which parts of the rules the cases never
reached, their **coverage**. These are the run's **statistics**. The caller
asks for them; a run that is not asked counts nothing more.

The statistics are counted while the run goes, from each case's decision,
before the next case is evaluated, as the rest of its result is (*Explaining
failures*): the run keeps the counts, and no result tree. A result tree has
one child per operand of `and`, `or` and `not` and a truth value for each
(`json-format.md`, *Result trees*), so one evaluation per case gives every
count: each rule's status, and the truth values of its tests in pre-order.

They count the **decisions** of the run: every case whose evaluation gives a
decision counts, whatever its expectation and its result, `passed` or
`failed`. So a case file of inputs without expectations measures what those
inputs reach. A case whose evaluation raises an error gives no decision: it
counts as a case and nowhere else. A file that cannot be read counts
nowhere. A partial run counts the cases it ran (*Batches and interruption*).

### Rules

For each rule of the rules tested, in rule order (a decision table's are the
rules it compiles to, `tables.md`, *Compiling*), the statistics count the
decisions in which:

| Count | The rule |
|---|---|
| `selected` | was selected by the hit policy |
| `fired` | fired (`rules.md`, *Evaluation*): its condition is `TRUE`, whether the hit policy selected it or not |
| `not_fired` | did not fire, its condition being `FALSE` |
| `unknown` | did not fire, its condition being `UNKNOWN` |

`fired`, `not_fired` and `unknown` add up to the number of decisions, and
`selected` is at most `fired`. An explanation's rule status `fired`
(`explain.md`, *Rule status*) is narrower: a rule that fired and was not
selected, which these counts give as `fired` minus `selected`. A rule is
located by a JSON Pointer into the document of the rules tested: the rule
(`/ruleset/rules/1`), or a decision table's row (`/decision_table/rows/1`);
for a bundle's rules, under `/bundle`.

### Conditions

For each rule, the statistics count the truth values of its **conditions**,
the elementary parts of its condition that a case can make `TRUE` or `FALSE`
on their own:

- in a rule set, the **tests** of the rule's condition (`audit.md`, *What the
  audit reads*, *Tests*): every node in a condition position other than
  `and`, `or` and `not`, such as a comparison, `in`, `between` (one test, not
  its two halves), `is_null`, a string test, or a field or constant used as
  a condition; in depth-first pre-order;
- in a decision table, the tests of the **input cells** of the rule's row
  that are not `-`, in input order: each cell holds one test, under a `not`
  for `NOT IN`, `NOT BETWEEN` and the other negated cells, and its test is
  counted, as in a rule set, with the test's truth value and `op` (`in` for
  `NOT IN ('ES', 'PT')`). The row's conjunction is the rule's own condition,
  counted by the rule.

Each condition is located by a JSON Pointer into the document of the rules
tested, as an audit locates it (`audit.md`, *JSON*, *Documents*): a node of a
rule set (`/ruleset/rules/1/when/operands/0`) or, for a cell's test, the cell
of a decision table (`/decision_table/rows/1/inputs/0`), since a cell is
text; for a bundle's rules, under `/bundle`.
Its counts are the decisions in which it was `TRUE`, `FALSE` and `UNKNOWN`,
which add up to the number of decisions.

*Rationale.* Tests are the granularity of condition coverage: what a test
gives is what the data decides, while the truth value of a connective follows
from its operands' in each case. With the rules' own counts, which are the
truth values of whole conditions, they measure both condition and decision
coverage. Tests are the leaves of a condition's diagram (`visual.md`,
*Conditions*: every node other than `and`, `or` and `not`, a `between` one
leaf), what an audit reads (`audit.md`), and what the cells of a table hold,
so they name the same things. A node of a test, such as an arithmetic
expression, has a value and no truth value of its own.

### Outputs

For a decision table, the statistics count, for each output column with
output values (`tables.md`, *Model*), in column order, how many times each of
its values was given: the selections of rows whose cell in that column holds
that value (a cell holds a constant equal to one of them, `tables.md`,
*Output cells*), in all the decisions, whatever the hit policy. A rule set's
outputs, and the columns of a table without output values, can hold any
value, and are not counted.

### Coverage

The coverage reports, for each **measure**, how many of its items the cases
reached at least once (`covered`), out of how many can be reached (`total`),
and which ones they never reached (`missed`). The measures, in this order:

| Measure | Items | An item is reached when |
|---|---|---|
| `rule_selected` | the rules | the rule's `selected` is not 0 |
| `rule_fired` | the rules | its `fired` is not 0 |
| `rule_not_fired` | the rules | its `not_fired` is not 0 |
| `rule_unknown` | the rules | its `unknown` is not 0 |
| `condition_true` | the conditions of every rule | the condition's `true` is not 0 |
| `condition_false` | the conditions of every rule | its `false` is not 0 |
| `condition_unknown` | the conditions of every rule | its `unknown` is not 0 |
| `output_value` | the output values of a decision table (*Outputs*) | the value's `selected` is not 0 |

`output_value` is reported for a decision table with output values only.

An item that reads no field, a **constant** one (`audit.md`, *What the audit
reads*: a node that reads no field), has the same truth value for every
input: a test such as `TRUE` or `1 > 2`, or a rule whose condition reads no
field, such as a decision table's row whose cells are all `-`, whose
condition is `TRUE`. It can never reach the measures of the other truth
values: a rule whose condition is `TRUE` never has `not_fired` or `unknown`,
one whose condition is `FALSE` or `UNKNOWN` never fires nor is selected, and
a test is never counted under the other two truth values. Such an item is
**unreachable** in those measures: it is left out of their `total` and
`missed`, and counted in their `unreachable`, so that full coverage is
always possible and the reader still sees what was left out. A constant
item counts in the other measures as any other.

The items missed are listed in the order of the statistics: rules in rule
order, conditions in rule order and then in their own order, and output
values by column and then in their column's order. A rule that never fires
is a rule whose output the cases never check; a condition never `FALSE`
does not show that the rule needs it; a rule never `UNKNOWN` was never given
missing or null data.

### JSON

The statistics are the `statistics` member of a test run (*Test runs*), an
object:

| Key | Required | JSON type |
|---|---|---|
| `cases` | yes | int: the cases run, with or without a decision |
| `decisions` | yes | int: the cases whose evaluation gave a decision |
| `rules` | yes | array of rules, in rule order (below) |
| `outputs` | for a decision table | array, in column order, of each output column with output values: `{"output", "values"}`, its name and, in the column's order, each value as `{"value", "selected"}`, the value in the JSON encoding of values (`json-format.md`, *Values*) and its count |
| `coverage` | yes | array of measures, in the order of the *Coverage* table (below) |

A **rule**:

| Key | Required | JSON type |
|---|---|---|
| `name` | yes | string: the rule's name, as decisions and audits name rules |
| `id` | when the rule has one | string: its id (`rules.md`, *Identity*) |
| `row` | for a decision table | int: the row's position in the table, from 0, as in an audit (`audit.md`, *JSON*) |
| `pointer` | yes | string: the rule's JSON Pointer (*Rules*) |
| `selected`, `fired`, `not_fired`, `unknown` | yes | ints: the counts of *Rules* |
| `conditions` | yes | array of conditions, in their order (*Conditions*): each `{"pointer", "op", "true", "false", "unknown"}`, its JSON Pointer, the `op` of its test's node and its counts |

A **measure**:

| Key | Required | JSON type |
|---|---|---|
| `measure` | yes | string: the measure (*Coverage*) |
| `covered` | yes | int: the items reached |
| `total` | yes | int: the items that can be reached |
| `unreachable` | when not 0 | int: the constant items left out of `total` and `missed` (*Coverage*) |
| `missed` | yes | array of the items not reached, in order, each with its place: `{"rule", "pointer"}` for a rule (the rule's pointer) and for a condition (the condition's), and `{"output", "value", "pointer"}` for an output value, the pointer to the value in its output's `values` (`/decision_table/outputs/0/values/2`) |

```json
{
  "cases": 3,
  "decisions": 3,
  "rules": [
    {
      "name": "vip", "pointer": "/ruleset/rules/0",
      "selected": 1, "fired": 1, "not_fired": 1, "unknown": 1,
      "conditions": [
        { "pointer": "/ruleset/rules/0/when/operands/0", "op": "gt", "true": 2, "false": 0, "unknown": 1 },
        { "pointer": "/ruleset/rules/0/when/operands/1", "op": "gt", "true": 2, "false": 1, "unknown": 0 }
      ]
    }
  ],
  "coverage": [
    { "measure": "rule_selected", "covered": 1, "total": 1, "missed": [] },
    { "measure": "rule_fired", "covered": 1, "total": 1, "missed": [] },
    { "measure": "rule_not_fired", "covered": 1, "total": 1, "missed": [] },
    { "measure": "rule_unknown", "covered": 1, "total": 1, "missed": [] },
    { "measure": "condition_true", "covered": 2, "total": 2, "missed": [] },
    { "measure": "condition_false", "covered": 1, "total": 2, "missed": [{ "rule": "vip", "pointer": "/ruleset/rules/0/when/operands/0" }] },
    { "measure": "condition_unknown", "covered": 1, "total": 2, "missed": [{ "rule": "vip", "pointer": "/ruleset/rules/0/when/operands/1" }] }
  ]
}
```

Counts are exact ints. Writers write the keys in the order of their tables,
and every array even when it is empty. Within format version 1, new
measures, new counts and new optional keys may be added: consumers ignore
those they do not know, and show a measure they do not know from its
`measure`, `covered`, `total`, `unreachable` and `missed`, whose items may
have other keys.

*Rationale.* The statistics are a member of the test run, not a document of
their own: they are counted over the run's cases, so what they mean depends
on what the run says, the rules tested, the case files, and whether the run
was partial, and one document keeps them together. The coverage repeats what
the counts say, so that a report or a command line shows what was missed
without deriving it.

### Text

*Not checked by vectors: the core's snapshot tests fix it.*

The text of the statistics, which the text of a test run ends with when the
run has them:

```text
Statistics of 3 cases: 3 decisions
  vip: selected 1, fired 1, not fired 1, unknown 1
Coverage:
  rules selected: 1 of 1
  rules fired: 1 of 1
  rules not fired: 1 of 1
  rules unknown: 1 of 1
  conditions true: 2 of 2
  conditions false: 1 of 2
    vip  gt at /ruleset/rules/0/when/operands/0
  conditions unknown: 1 of 2
    vip  gt at /ruleset/rules/0/when/operands/1
```

- `Statistics of <n> cases: <d> decisions`.
- One line per rule, in rule order: its name and its four counts.
- `Coverage:` and one line per measure, `<what> <how>: <covered> of
  <total>` (`rules selected`, …, `conditions unknown`, `output values`),
  followed by `, <n> unreachable` when it has unreachable items, and then by
  its items missed, one per line and indented further: a rule by its name; a
  condition in a rule set as `<rule>  <op> at <pointer>`, and in a decision
  table as `<op> at row <label>, input <header> (<pointer>)`, as an audit
  writes a cell (`audit.md`, *Text*), the row's label naming the rule; an
  output value as `<output> = <value>`, the value in compact JSON.

## Not yet

*Informative: what this version leaves out, so that a later version can add
it as optional keys (`json-format.md`, *Versioning*) without changing what
existing case files mean.*

- The rules and engine that recorded the expectations of a case file (a
  `ruleset_digest` and an engine, as a bundle states them), planned for
  comparing two versions (task T.4).
- Expectations on the rules that fired, on the truth value of each rule, or
  on the explanation (a rule's status or requirement, a counterfactual).
- Expecting that an output has no value under a key, or comparing numbers
  with a tolerance or across types.
- The context of an expected error (`rule`, `path`, `rules`), or a parent code
  that matches its children (`evaluation_error` for `missing_field`).
- Tags or other selections of cases, and a name besides the id.
- Case files in CSV (a column per input field and expected output) and a
  streaming form, such as one case per line, for files beyond the size limit.
- Expected changes between two versions of the rules, which comparing them
  (task T.4) defines.
- Settings of a case that change how the rules evaluate (strict mode, limits):
  they are settings of the rules, under their digest.
- Running only some of the cases (by id, tag or file), and keeping only some
  results in a test run document (the failures, say).
- Reading case files lazily, one at a time as the run reaches them: today a
  run is given every case file, read, and `remaining` counts within them.
- A limit of time: runs are bounded by results and steps, which are
  deterministic, and a deadline is a cancellation (*Batches and
  interruption*).
- Statistics beyond counts of truth values: the counts of a connective
  (`and`, `or`, `not`), pairs of conditions (modified condition/decision
  coverage), the values of the outputs of a rule set or of a column without
  output values, the counts of errors by code, statistics over the cases
  whose evaluation raises an error, and statistics under a graded logic
  (degrees, once rules fire to degrees).
- Statistics without a test run: over a batch of inputs given as data (a
  table in Arrow, task I), rather than as the cases of case files.
