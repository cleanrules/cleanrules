# Test cases

**Status:** draft, format version `1` (task T.1). JSON Schema:
[`schemas/v1/cases.schema.json`](schemas/v1/cases.schema.json).

A **test case** gives rules one input and says what evaluating it must give:
the rules selected, their outputs, the rules whose condition is unknown, or
the error. A **case file** is a document that holds a list of cases. Running
the cases of a case file against two versions of the rules, and comparing what
each gives, is how a user tests and compares those versions; a bundle
(`bundles.md`, *Cases*) names the case files that go with its rules.

Cases are run with `evaluate`, never with `run`: a test never calls an action
(`rules.md`, *Actions*), so it gives the same result whether the rules' actions
are bound or not, and it can be run any number of times. Running cases, their
results and summaries (task T.3), running them in batches (T.2) and comparing
two versions on the same cases (T.4) build on this format.

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

*Not checked by vectors yet: the case runner (task T.3) runs cases, and its
vectors check matching.*

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
