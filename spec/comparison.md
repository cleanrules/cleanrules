# Comparing two versions

**Status:** draft, format version `1` (task T.4). JSON Schema:
[`schemas/v1/test_comparison.schema.json`](schemas/v1/test_comparison.schema.json).

A **comparison** runs the same test cases (`tests.md`) against two versions of
the rules, `a` (the version compared from, usually the current one) and `b`
(the version compared to, usually the candidate), and says, case by case,
which decisions changed, how, and whether each change was expected. It is how
a user checks a change to their rules before they ship it: every case whose
decision changed and whose expectation does not say so is an **unexpected
change**, which a command line turns into a failing exit code (`cli.md`,
`diff`).

A comparison is not the comparison of two bundles (`bundles.md`, *Comparing
bundles*), which looks at two packages and never runs a case; a comparison of
two bundles' runs reports it (*Bundles*).

```json
{
  "version": 1,
  "test_comparison": {
    "relation": "other_rules_same_engine",
    "a": { "source": "ruleset", "hit_policy": "first", "logic": "boolean", "ruleset_digest": "sha256:5a0e…", "engine": { "name": "cleanrules", "version": "1.0.0", "limits": { "…": 0 } }, "files": [{ "path": "cases/discounts.json", "digest": "sha256:3751…" }], "limits": {}, "summary": { "total": 2, "passed": 2, "failed": 0, "error": 0, "partial": false }, "outcomes": { "decisions": 2, "nothing_selected": 1, "with_unknown": 1, "error": 0 } },
    "b": { "…": "the same keys, for b" },
    "limits": { "max_explained": 5 },
    "summary": { "total": 2, "unchanged": 1, "changed": 1, "expected": 0, "unexpected": 1, "added": 0, "removed": 0, "not_compared": 0, "partial": false, "by_change": { "same_selection_other_output": 1, "other_selection_same_output": 0, "other_selection_other_output": 0, "other_unknown": 0, "error": 0 } },
    "items": [
      {
        "path": "cases/discounts.json",
        "id": "vip-gets-15",
        "change": "same_selection_other_output",
        "expected": false,
        "differences": ["output"],
        "unstated": ["status"],
        "a": { "status": "passed", "selected": ["vip"], "output": { "discount": { "$decimal": "0.15" } }, "outputs": [{ "discount": { "$decimal": "0.15" } }], "unknown": [] },
        "b": { "status": "failed", "selected": ["vip"], "output": { "discount": { "$decimal": "0.20" } }, "outputs": [{ "discount": { "$decimal": "0.20" } }], "unknown": [] }
      }
    ]
  }
}
```

## Running a comparison

A comparison is given two **sides**, `a` and `b`. Each side is what a run
needs (`tests.md`, *Running cases*): rules (a rule set, a decision table or
the rules of a bundle) and the case files to run against them, with the run's
limits. The two sides usually run the same case files; they may run different
ones, as two bundles each list their own (*Joining*).

Each side is run as a test run is, with its own limits: its cases in run
order, one result each, a file that cannot be read giving one result, and the
case files of a bundle checked against their digests first. Side `a` runs
first, then side `b`. Each case is evaluated with `evaluate`, never with
`run`: a comparison never calls an action. The comparison then **joins** the
results of the two sides (*Joining*) and compares the decisions of each case
that both sides ran (*Changes*).

A comparison keeps what each side's decision gives (the selected rules, the
outputs and the unknown rules, or the error), never its result trees, and
explains only the first changes (*Explaining changes*), so it holds one small
record per case of side `a` until side `b` has run.

### Relation

A comparison answers one of the two questions that `bundles.md` keeps apart:
whether **the same rules** give other decisions under **another engine**, or
whether **other rules** give other decisions under **the same engine**. Its
`relation` says which, with the names of `bundles.md` (*Comparing bundles*),
from the rules and the engine of each side's run:

| Relation | When |
|---|---|
| `same_rules_same_engine` | the same rule set digest (`json-format.md`, *Rule set digests*), and the same engine: its `name`, `version` and every limit the case files were read with |
| `same_rules_other_engine` | the same rule set digest, and another engine or limits: any change comes from the engine |
| `other_rules_same_engine` | another rule set digest, and the same engine: any change comes from the rules |
| `other_rules_other_engine` | another rule set digest, and another engine or limits |

The engine of a side is the one its test run records (`tests.md`, *Test runs*,
`engine`): the engine that runs the comparison, with the limits its case
files were read with (a bundle's own, for a bundle's cases). A comparison runs
both sides on the engine that makes it, so only their limits can make it
`other_engine`. A comparison `other_rules_other_engine` is still made, and
the text says that its changes may come from either; to keep the two
questions apart, compare in two steps, changing one thing at a time.

"The same rules" means the same rule set digest, so a decision table and the
rule set it compiles to are the same rules; rules that differ only in a
description are other rules, which decide alike (`bundles.md`, *Comparing
bundles*). A rule renamed with its old name as its `id` stays the same rule
in every case (*Changes*), since cases name rules by effective id.

### Bundles

When both sides are bundles, the comparison also holds the comparison of the
two bundles (`bundles.md`, *Comparing bundles*) under `bundles`: their
relation as packages, whether they are identical and what differs. The two
relations can differ: `bundles.relation` looks at the engines that made the
bundles, as they record them, while `relation` looks at the runs compared,
both made by the engine that compares.

Two bundles with the same `id` and `revision` and different rule set digests
are a **conflict** (`bundles.md`, *Identifier and revision*): a comparison
refuses them, with a `decision_mismatch` that names the bundle and its
revision, and runs nothing.

## Joining

A case is identified in a run by its file's `path` and its `id` (`tests.md`,
*Identity*); a file that cannot be read gives one result, identified by its
`path` alone. The comparison joins the two sides' **items** (each case, and
each file that cannot be read) by that identity: the same path, or both
without one, and the same id, or both without one. Items without an id that
share a path on one side (a file that cannot be read, and a later file that
repeats its path) are joined in their order: the first with the other side's
first, and so on.

The items of a side are those its case files hold, whether or not the run
reached them: every case of each file that is read, and one item for a file
that cannot be read (or that repeats an earlier file's path, `tests.md`,
*Running cases*). Each item of the comparison is exactly one of:

| Item | When |
|---|---|
| **compared** | both sides have the item and gave it a result: it is unchanged or one of the changes (*Changes*); but for a file that neither side can read with the same error, which is not compared |
| **added** | a case only side `b` has: no case file of `a` holds it, and `a` reads the file at its path, or has none |
| **removed** | a case only side `a` has, the same way |
| **not compared** | any other item: both sides have it and a side did not reach it, because its run was interrupted (*Interruption*); a side cannot read the file at its path, so neither its cases nor their absence are known there; or neither side can read a file, with the same error, so none of its cases is known on either side |

Whether a case is added or removed depends on the case files only, not on
the runs: it is listed whether or not the side that has it reached it, with
that side's result when it did. A file that one side cannot read and the
other side reads is an item of the first side only: when that side reached
it, it is a change of the kind `error` against no result (*Changes*), and
otherwise it is not compared; the cases the other side reads in it are not
compared. A file that both sides cannot read with other errors is a change
of the kind `error`.

A comparison is **complete** when no side was interrupted and no item is
left not compared: then every case either side holds was compared, added or
removed. A command line treats an incomplete comparison as a failure, since
the cases it did not compare could hide a change (`cli.md`, *Not yet*,
`diff`).

**Comparison order** is the order of the items: those of side `a` in its run
order, and then those only side `b` has, in `b`'s run order. The same sides
give the same items, in the same order, on every platform.

## Changes

Two results of the same case are compared by what each decision gives,
reduced as a run reduces it (`tests.md`, *Explaining failures*): the selected
rules, the outputs and the unknown rules, or the error. The **parts** that
can differ, each compared as *Matching* compares the same key (`tests.md`)
but between the two sides:

| Part | Differs when |
|---|---|
| `selected` | the effective ids of the selected rules, in selection order, are not the same |
| `output` | the single output differs (the winner's or the aggregate; null when no rule was selected; none when the decision has no single output), or the outputs of the selected rules differ in number or at some position: values compared by their canonical bytes (`tests.md`, *Values*), every key of each output |
| `unknown` | the effective ids of the rules whose condition is unknown are not the same set |
| `error` | one side raised an error and the other did not, or both raised errors whose codes or compared fields (`errors.md`, *Comparing errors*) differ |

When either side raised an error, only `error` is compared: a decision and an
error have no other part in common. Two errors with the same code and compared
fields are the same, whatever their messages, which are not part of the
decision. The statuses of the two results (`passed`, `failed`, `error`) are
not compared: they depend on each side's expectations as much as on the
decision.

An item whose parts are all equal is **unchanged**. Any other compared item
is a **change**, whose **kind** (`change`) is:

| Change | When |
|---|---|
| `same_selection_other_output` | the same rules selected, and other outputs |
| `other_selection_same_output` | other rules selected, and the same outputs |
| `other_selection_other_output` | other rules selected, and other outputs |
| `other_unknown` | the same rules selected and outputs, and other unknown rules |
| `error` | `error` differs: a new error, an error that is gone, or another error; and a file that one side cannot read |

A change lists the parts that differ (`differences`, in the order of the
table of parts), so the unknown rules that change with the selection are
still listed.

### Expected changes

A change is **expected** when the case states the new decision: the case of
side `b` has an expectation that is not empty (`tests.md`, *Expectations*),
`b`'s result is `passed`, and its expectation checks every part that differs:

| Part | Checked |
|---|---|
| `selected` | by the key `selected` |
| `output` | when the expectation states each way the outputs changed (below) |
| `unknown` | by the key `unknown` |
| `error` | when `b` raised an error, by the key `error` (which `b` passed, so it is that error); when `b` gave a decision instead, by `selected`, `output` or `outputs`, which state the new decision |

An expected output checks only the keys it names (`tests.md`, *Matching*), so
it states only those keys' changes. The outputs changed in up to three ways,
and the expectation states each of them:

- **Payload keys whose values differ.** A key differs when its value is not
  the same in the two outputs, or one of them lacks it. When both sides have
  a single output (the winner's, or the aggregate), the keys that differ in
  it are named by the object under `output`, or, under a single-hit policy of
  `b`, by the first item of `outputs`. At each position that both lists of
  outputs have, the keys that differ are named by the item of `outputs` at
  that position, or, under a single-hit policy, by `output` at the first
  position.
- **A switch between an output and none** (some output on one side and null,
  or a single output on one side and none at all on the other, as when the
  hit policy changes): stated by `output` (`null` or an object) or `outputs`.
- **Another number of outputs**: stated by `outputs`, or, under a single-hit
  policy of `b`, by `output`, since there the number is one or none.

So an expectation that names `discount` does not state a change of `tier`:
the change is unexpected, although `b` passes the case. To state a change,
name every key that changes; keys that keep their values need not be named.

Any other change is **unexpected**, and so is a change of a file that cannot
be read, which has no expectation. So **expected changes are written as the
expectations of the new version**: a user who changes the rules on purpose
updates the expectations of the cases the change affects, in the case files
that side `b` runs, and every other case keeps its decision or is reported.
An empty expectation states nothing about the decision, so a case without
expectations never makes a change expected: a set of recorded inputs without
expectations reports every change. A case side `a` fails, because its
expectation already states the new decision, is not a problem of the
comparison: `a` is the old version.

An unexpected change says why (`unstated`): `status` when `b`'s result is
not `passed`, and otherwise the parts that differ and that `b`'s expectation
does not check, in the order of the table of parts (every part that differs,
for a case without expectations or a case file that cannot be read). The
unknown rules often change with the selection: a rule after a new winner,
or a rule before it, is evaluated on other data paths and can turn unknown
or known. A case that states the new selection and outputs but not `unknown`
leaves that part unstated; add `unknown` to it when the unknown rules are
part of the intended change.

*Rationale.* Expectations are already how a case says what the rules must
give, and they are checked against the version a user ships; reusing them
adds nothing to the format and keeps one place where the intended behaviour
is written. A separate list of expected changes would name cases a second
time and go stale as soon as the cases are edited.

### Explaining changes

Up to `max_explained` changes (5 unless the caller sets it; 0 explains none)
are **explained**: the unexpected changes first, in comparison order, and
then the expected ones, in comparison order, since an unexpected change is
the one a reader has to understand. In each change explained, each side that
gave a decision holds the text of its explanation (`explain.md`, *Text*), so that a reader
sees why each version decided as it did. A comparison evaluates a case again
to explain it, as a run does (`tests.md`, *Explaining failures*): evaluation
is deterministic, so the decision is the one compared.

## Interruption

Each side runs with its own limits (`tests.md`, *Batches and interruption*:
`max_results`, `max_steps`), and the caller may cancel the comparison by a
callback, asked before each item of either side, in their order (side `a`'s
first). Once it cancels, both sides stop: a side not finished is partial
(`cancelled`), and side `b` gives no results when `a` was cancelled.

A side that is interrupted is **partial**, and its summary says why and how
many results it did not give, as a test run's does. The comparison is then
partial too (`summary.partial`): an item that both sides have and that a
partial side did not reach is **not compared**, never unchanged, so a partial
comparison never reports a case as unchanged that it did not run on both
sides. The items it compared are the same, with the same changes, as in the
full comparison, since each side's results are a prefix of its full run's;
added and removed cases are the same too, since they depend on the case
files only.

## JSON

A test comparison document has the content key `"test_comparison"`
(`json-format.md`, *Documents*). Like a test run, it is output only: readers
reject it with `unsupported`. The key names what is compared, the results of
test runs; the comparison of two bundles (`bundles.md`) is not a document.

| Key | Required | JSON type |
|---|---|---|
| `relation` | yes | the relation of the two sides (*Relation*) |
| `bundles` | when both sides are bundles | object: the comparison of the two bundles (`bundles.md`, *Comparing bundles*): `relation`, `identical`, `differences` |
| `a` | yes | object: side `a` (below) |
| `b` | yes | object: side `b` (below) |
| `limits` | yes | object: `max_explained`, an int |
| `summary` | yes | object: the summary (below) |
| `items` | yes | array: the items listed (below), in comparison order |

A **side** is the header and summary of its test run (`tests.md`, *Test
runs*), without its results, and the counts of its outcomes:

| Key | Required | JSON type |
|---|---|---|
| `source`, `hit_policy`, `aggregate`, `logic`, `strict` | as in a test run | what was run, as a test run writes it |
| `ruleset_digest` | yes | string: the rule set digest of the side's rules, which the relation compares |
| `engine`, `bundle`, `files` | as in a test run | the engine, the bundle and the case files of the side's run |
| `limits` | yes | object: `max_results` and `max_steps` when they are set, ints; it may be empty |
| `summary` | yes | object: the side's run summary, as a test run's (`total`, `passed`, `failed`, `error`, `partial`, and `interrupted` when partial) |
| `outcomes` | yes | object: among the side's cases that ran, `decisions` (evaluation gave a decision), `nothing_selected` (the decision selected no rule), `with_unknown` (some rule's condition was unknown) and `error` (evaluation raised an error, expected or not: the cases that ran less `decisions`), ints |
| `statistics` | when the side's run gathers them | object: the side's statistics and coverage, as a test run writes them (`tests.md`, *Statistics*, *JSON*) |

The **summary** of the comparison counts its items, listed or not:

| Key | Required | JSON type |
|---|---|---|
| `total` | yes | int: every item of the two sides, joined |
| `unchanged` | yes | int |
| `changed` | yes | int: the changes, `expected` plus `unexpected` |
| `expected` | yes | int |
| `unexpected` | yes | int |
| `added` | yes | int |
| `removed` | yes | int |
| `not_compared` | yes | int |
| `partial` | yes | boolean: whether either side was interrupted |
| `by_change` | yes | object: the number of changes of each kind, every kind of the table of changes in its order |

A comparison is complete (*Joining*) when `partial` is `false` and
`not_compared` is 0.

`items` lists the changes, the added cases and the removed cases, in
comparison order; unchanged and not compared items are only counted. An
**item**:

| Key | Required | JSON type |
|---|---|---|
| `path` | when its file has one | string |
| `id` | for a case | string: the case's id; absent for a file that cannot be read |
| `change` | yes | a kind of change (*Changes*), `"added"` or `"removed"` |
| `expected` | for a change | boolean: whether the change is expected (*Expected changes*) |
| `differences` | for a change | array of the parts that differ, in the order of their table |
| `unstated` | for an unexpected change | array: why it is unexpected (*Expected changes*): `"status"`, or the parts that differ and that `b`'s expectation does not check, in the order of their table |
| `a` | when side `a` gave the item a result | object: what side `a` gave (below) |
| `b` | when side `b` gave the item a result | object: what side `b` gave |

What a side gave, in the forms of a test run (`tests.md`, *Test runs*):

| Key | Required | JSON type |
|---|---|---|
| `status` | yes | the result's status: `"passed"`, `"failed"` or `"error"` |
| `selected` | for a decision | array: the effective ids of the selected rules, in selection order |
| `output` | for a decision with a single output | the single output with all its keys, or `null` when no rule was selected |
| `outputs` | for a decision | array: the outputs of the selected rules, in selection order, each with all its keys |
| `unknown` | for a decision | array: the effective ids of the rules whose condition is unknown, sorted by Unicode code point |
| `error` | for an error | the error evaluation raised (whatever the status: a case that expects that error has passed), or the error of a file that cannot be read, in its JSON form (`errors.md`, *JSON form*) |
| `explanation_text` | for a change explained | string: the text of the decision's explanation |

Writers write the keys in the order of their tables, and every array even
when it is empty. Errors are written in their JSON form; conformance vectors
compare them by code and compared fields (`errors.md`, *Comparing errors*).

**Extensions.** Within format version 1, new optional keys, new kinds of
change, new parts and new relations may be added: consumers ignore the keys
they do not know, show a kind or a part they do not know as it is written,
and count an item as a change exactly when it has `expected`; an unknown
reason in `unstated` is shown as it is written. Whether a
change is expected is a closed set (`true` or `false`), as the statuses of a
test run are, so that a command line can map unexpected changes to its exit
codes.

## Text

*Not checked by vectors: the core's snapshot tests fix it.*

The text of a comparison, as the command line prints it, is generated by the
core:

```text
Comparison of two test runs: other rules, same engine (other_rules_same_engine)
a: a rule set, 3 rules, hit policy priority, logic boolean, sha256:8fbea85c5715c98c5d026e8a85bffcb48d9a0fecfccd6641249f14ab82b263bd
b: a rule set, 3 rules, hit policy priority, logic boolean, sha256:d46695c1f45a34646895abac362da9c116947c1d6e2b145297646eb7ea257c18
UNEXPECTED  cases/discounts.json  "vip-gets-15"
            same_selection_other_output (output)
            b did not pass its case
            a: passed, selected ["vip"], output {"discount":{"$decimal":"0.15"}}, unknown ["blocked"]
            b: failed, selected ["vip"], output {"discount":{"$decimal":"0.20"}}, unknown ["blocked"]
            a explanation:
              HIT POLICY priority: selected vip
              ...
            b explanation:
              HIT POLICY priority: selected vip
              ...
4 items: 3 unchanged, 1 changed (0 expected, 1 unexpected), 0 added, 0 removed, 0 not compared
```

- A header with the relation, in words and by its name, and one line per
  side: its bundle's id and revision when it has one, what it ran, its
  number of rules and its configuration, as in a test run (`tests.md`,
  *Text*), and its rule set digest. When the engine limits differ, a line
  `engine limits differ: ` with each limit that differs and its values,
  `max_depth a 256, b 100`, separated by `; `. When both sides are bundles,
  a line with the bundles' relation and differences (or `identical`); under
  `other_rules_other_engine`, a note that a change may come from the rules
  or the engine.
- One block per item listed, in comparison order: `EXPECTED` or
  `UNEXPECTED` for a change, `ADDED` or `REMOVED`, then its path and its id,
  quoted as a JSON string, when it has them (`(a case file without a path)`
  when it has neither); then, indented, the kind of a change with the parts
  that differ in parentheses; for an unexpected change, why:
  `b did not pass its case`, `b's expectation does not check: <parts>`, or
  `a case file has no expectation`; a line per side that gave a result: its status
  and its decision in compact JSON (the selected rules, the single output or,
  without one, the outputs, and the unknown rules when there are any), or its
  error as its code, its location (`at` a pointer, `the root` for `""`, or
  `at line <l>, column <c>`), the rule or rules it names (`in rule "a"`) and
  its message; and each side's explanation, indented further.
- The summary: the number of items and how many of each; and, for each
  partial side, `Partial run of <a|b>: interrupted by <reason>, <n> not run`.

## Not yet

*Informative: what this version leaves out, so that a later version can add
it as optional keys (`json-format.md`, *Versioning*).*

- Comparing with results recorded by another engine, such as an older
  release: a comparison runs both sides on the engine that makes it. The
  case files of a bundle, whose expectations its engine met, are how a newer
  engine is checked today, by running them (`tests.md`, *Case files of a
  bundle*); recording what rules gave as expectations, with the rules and
  engine that recorded them (`tests.md`, *Not yet*), is the way to it.
- Noting that a case's `input` differs between the two sides' case files:
  each side evaluates its own case, joined by path and id.
- Accepting changes without writing expectations (all the changes of a file,
  of a rule, or of an output key).
- A comparison that runs the two sides in step, without holding a record per
  case of side `a`, and a cursor over its items, as a run has.
- Listing unchanged or not compared items, or only some of the changes.
