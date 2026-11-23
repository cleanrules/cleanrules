# Static audit

**Status:** draft, normative for the core and every binding. The JSON form of
an audit is defined below (*JSON*) and in `schemas/v1/audit.schema.json`.

An **audit** examines a rule set or a decision table without any input, and
reports what its conditions can and cannot do: conditions that are never
`TRUE`, tests whose types never match, rules that are never selected, rules
that fire together, and inputs for which no rule fires. Each finding is a
**diagnostic** with a stable code, a severity that takes the hit policy into
account, the rules involved and, where it helps, an input that shows it (its
**witness**).

The audit is **sound but incomplete**:

- a diagnostic is always true. A finding that something can happen (two rules
  fire together, no rule fires) comes with a witness that the rule set itself
  has evaluated, with its logic and strictness, before it is reported; a
  finding that something never happens comes from reasoning that covers every
  input;
- where the audit cannot decide, because a condition holds a test it does not
  model, a witness cannot be built or a limit is reached, it says so in the
  report (*What was checked*), and reports nothing about it.

The audit is bounded (*Limits*) and deterministic: the same rules and limits
give the same report, diagnostics and witnesses included, in every language.
It reasons about conditions with Kleene logic, as they are evaluated without
strict mode (`ast.md`), which only adds errors: a condition that is never
`TRUE` without strict mode is never `TRUE` with it. Its witnesses, instead,
are evaluated with the rule set's logic and strictness (*Witnesses*). Rule
sets and tables take no other logic until rules fire to degrees (`rules.md`,
*Evaluation*); auditing one with another logic is `unsupported`.

## What the audit reads

### Tests

In a rule's condition, `and`, `or` and `not` combine **tests**: every other
node in a condition position, such as a comparison, `in`, `between`,
`is_null`, `matches`, a string test, or a field or boolean constant used as a
condition. A decision table's rule is the conjunction of the conditions of
its input cells (`tables.md`, *Compiling*), and the audit reads each cell's
condition in its place.

A value node that reads no field is **constant**: the audit evaluates it once
(`ast.md`, without strict mode) and uses its value, so `x > 1 + 1` is read as
`x > 2`. A test that reads no field, such as `TRUE` or `1 < 'a'`, is constant
too: its truth value is the same for every input.

The **subject** of a test is the value node it tests:

- for a comparison of a value node with a constant, in either order, the
  value node: `5 < x` is read as `x > 5`;
- for `in`, `is_null`, `matches` and the string tests, the operand;
- for `between` with constant bounds, the operand;
- for a field used as a condition, the field.

A subject is a field or a value expression (arithmetic, `abs`, `neg`,
`round`). Two subjects are the same when they are structurally identical
(`ast.md`, *Structural identity*).

A test is **modelled** when it has a subject and is a comparison, `in`,
`between`, `is_null` or a field used as a condition. The other tests are
**not modelled**: a comparison of two value nodes that both read fields
(`a > b`), a `between` with a bound that reads a field, `matches` and the
string tests. The audit lists each one (*What was checked*) and assumes that
it may be `TRUE`, `FALSE` or `UNKNOWN` for any input, whatever the other
tests give. Every node of a condition is so either read by the audit or
listed as not read; none is skipped silently.

### Values of a subject

A subject is null, or a value of one of the groups of `ast.md` (*Types*). A
field can hold any value; arithmetic, `abs`, `neg` and `round` give a number
or null. The audit describes the values a subject may hold as a set: whether
it holds null, whether it holds values of the `other` group, and, for each
orderable group and for booleans, a union of intervals of that group, ordered
as `ast.md` orders them (numbers exactly by value, strings by code point,
dates and datetimes in time order, aware ones by instant).

Each modelled test is `TRUE` for one set of values of its subject and `FALSE`
for another, and `UNKNOWN` for every other value:

| Test | `TRUE` for | `FALSE` for |
|---|---|---|
| `s = c` | the values of `c`'s group equal to `c` | the other values of `c`'s group |
| `s != c` | the values of `c`'s group not equal to `c` | the values equal to `c` |
| `s < c`, `s <= c`, `s > c`, `s >= c` | the values of `c`'s group in that order with `c`, when `c` is not a boolean; none when it is (`unorderable`) | the other values of `c`'s group; none for a boolean |
| `s in [v, …]` | the values equal to one of the `v`s | the other values of their group |
| `between(s, low, high)` | as the conjunction of its two comparisons (`ast.md`, *Node semantics*) | as their conjunction |
| `is_null(s)` | null | every other value |
| a field used as a condition | `true` | `false` |

A constant that is null (a constant expression such as `1 / 0`) makes the
test `UNKNOWN` for every input.

A set is **empty** only when the audit is certain of it: for strings, booleans,
dates and datetimes, exactly, from the values the data model allows (no date
lies strictly between `2026-01-01` and `2026-01-02`, and none after
`9999-12-31`); for numbers, when its bounds cross, or meet at a bound that
excludes them.

### Combining tests

An input gives each subject a value, and the audit describes the inputs that
make a condition `TRUE`, and those that make it `FALSE`, as unions of
**boxes**. A box gives a set of values to some subjects, and any value to the
others; it is empty when one of its sets is. Following Kleene's tables
(`ast.md`, *Truth values*):

- a modelled test gives a box on its subject for `TRUE`, and one for `FALSE`,
  each left out when empty;
- a constant test gives a box that holds every input for its truth value, and
  none for the other two;
- `not` swaps the two;
- `and` is `TRUE` for every intersection of one `TRUE` box of each operand, in
  operand order, without the empty ones, and `FALSE` for the `FALSE` boxes of
  every operand, in order; `or` is the reverse.

Inputs that differ only in a subject the boxes do not constrain behave the
same. Subjects are treated as independent, although two fields whose paths
are prefixes of one another (`customer` and `customer.age`) and a field and
an expression that reads it are not: this only makes some boxes hold
combinations that no input has, which the witnesses (*Witnesses*) never
show.

A condition with a not-modelled test has two descriptions of its `TRUE`
inputs: the **wide** one takes each not-modelled test as both `TRUE` and
`FALSE` for every input, and holds every input that makes the condition
`TRUE`; the **narrow** one takes it as neither, and holds only inputs that
make the condition `TRUE`. They are the same when every test is modelled. A
claim that something never happens is made from the wide ones, and a claim
that one set covers another from a narrow one covering a wide one.

## Checks

The audit runs these checks, in this order. Diagnostics are listed by code,
in the order of the table, and then in the order their check takes its items
(rules in rule order, tests in depth-first pre-order, pairs of rules by their
first rule and then by their second). Each says which rules it involves, in
rule order unless the check says otherwise.

| Code | Severity | Check |
|---|---|---|
| `incompatible_types` | warning | *Types* |
| `mixed_types` | warning | *Types* |
| `unlisted_field` | warning | *Fields of decision tables* |
| `unsatisfiable` | warning | *Unsatisfiable conditions* |
| `shadowed` | warning | *Shadowed rules* |
| `overlap` | by policy | *Overlaps* |
| `gap` | by policy | *Gaps* |

Severities:

- `error`: some input makes the evaluation fail, as the witness shows;
- `warning`: part of the rules has no effect, or probably does not do what
  was meant;
- `info`: a fact the policy makes acceptable, which may be intended.

### Types

- **`incompatible_types`**: a modelled test that is `UNKNOWN` for every input,
  because the values its subject can hold never compare with its constant:
  booleans ordered (`x > TRUE`), an arithmetic result compared with a string
  (`a + 1 = 'x'`), or a constant test that is `UNKNOWN` (`1 < 'a'`). It
  names the rule and gives the test's `pointer`.
- **`mixed_types`**: a field that the conditions use with values of more
  than one group. A field's **expected groups** are the groups of the
  constants of the modelled tests whose subject it is (but for those reported
  as `incompatible_types`), `boolean` when it is used as a condition, `string`
  when it is the operand of `matches` or a string test, and `number` when it
  is an operand of arithmetic. It gives the field's `path` and its `groups`,
  in the order of `ast.md` (*Types*), and names every rule whose condition
  uses it so.

### Fields of decision tables

*Only for decision tables; `not_applicable` for a rule set.*

- **`unlisted_field`**: a cell, input or output, that reads a field whose path
  is one key, which none of the table's inputs reads. It is most often text
  written without quotes: the cell `ES` under the input `country` is
  `country = ES`, a comparison with the field `ES`, where `'ES'` was meant.
  It names the row's rule and gives the field's `path` and the cell's
  `pointer`, one diagnostic per cell and field.

### Unsatisfiable conditions

- **`unsatisfiable`**: a rule whose condition is never `TRUE`, so the rule
  never fires: its wide `TRUE` description is empty. `x > 5 AND x < 3`,
  `x BETWEEN 10 AND 1` and `x = 'a' AND x > 5` are.

Each rule is decided when it is reported, or when a witness makes it fire.

### Shadowed rules

*Under `first`, `any` and `priority`; `not_applicable` under the other
policies, where every rule that fires is selected (`rule_order`, `collect`,
and `output_order` in tables), or a rule that fires with another raises
(`unique`, see *Overlaps*).*

A rule is **ahead** of another when the policy selects it first when both
fire: under `first` and `any`, a rule earlier in rule order; under
`priority`, a rule with a higher priority, or the same priority and earlier
in rule order.

- **`shadowed`**: a rule, not reported `unsatisfiable`, that fires only for
  inputs for which a rule ahead of it fires too, so it is never selected: the
  narrow descriptions of the rules ahead of it cover its wide one. The rule
  comes first in `rules`, followed by every rule ahead of it whose wide
  description meets its own, in rule order.

Each rule not reported `unsatisfiable` is decided when it is reported, when
no rule is ahead of it, or when a witness makes it fire and no rule ahead of
it fire.

### Overlaps

*Under `first`, `priority`, `unique` and `any`; `not_applicable` under
`rule_order`, `collect` and `output_order`, which select every rule that
fires.*

- **`overlap`**: two rules that fire for one input, the witness. Its severity
  depends on the policy, and the witness shows it as *Witnesses* says:
  - `unique`: `error`: evaluating the witness raises `hit_policy_error`,
    naming both rules;
  - `any`: `error` when evaluating the witness raises `hit_policy_error`
    naming both rules and their two payloads differ on it (`rules.md`,
    *`ANY` equality*); otherwise the witness is evaluated without an error,
    both rules fire, and the overlap is `info` when the payloads are
    structurally identical, which makes them equal for every input, and
    `warning` when they are not, as they may differ for another input;
  - `first` and `priority`: `info`, since the policy decides which takes
    precedence: the witness is evaluated without an error and both rules
    fire.

Each pair of rules, neither reported `unsatisfiable`, is decided when it is
reported, or when the intersection of their wide descriptions is empty.

### Gaps

A **supported input** gives every field that a condition reads a value that
is not null, of one of its expected groups (*Types*), or of any group but
`other` when it has none.

- **`gap`**: a supported input, the witness, which the rule set evaluates
  without an error and for which no rule fires. In this version at most one
  is reported (*JSON*). Its severity is `warning` under `first`, `priority`,
  `unique` and `any`, where the decision then has no output, and `info` under
  `rule_order`, `collect` and `output_order`, where no outputs is an ordinary
  result. Its `rules` is empty.

The gap check is decided when it reports a gap, or when the narrow
descriptions of the rules cover every supported input.

## Witnesses

A witness is an input, a data value (`data-model.md`) written in the JSON
encoding of values (`json-format.md`, *Values*). It holds no opaque value,
so it can be kept as the `input` of a test case in a case file (`tests.md`,
*Cases*, *Inputs*), which then reproduces the finding with the same rules.
It is evaluated by the rule set itself, with its logic
and strictness (`rules.md`, *Evaluation*, *Strict mode and errors*), and
shows its finding when that evaluation gives what the finding says:

- for an `overlap` of severity `error`, a `hit_policy_error` whose `rules`
  hold both rules (and, under `any`, the two payloads differ on it);
- for any other `overlap`, a decision, without an error, in which both rules
  fire;
- for a `gap`, a decision, without an error, in which no rule fires; the
  witness is a supported input.

Any other result, such as a `missing_field` or a `type_mismatch` raised in
strict mode, shows nothing: the item is undecided, with the reason
`no_witness`. `overlap` and `gap` always have a witness, and the other codes
never do.

The core builds a witness from a box: a map that gives each field the box
constrains a value of its set, and every other field a condition reads a
value of a supported input when it can, with integer segments indexing
lists. Paths that are prefixes of one another (`customer` and
`customer.age`) leave some fields out, except in a gap's witness, which has
to be a supported input and gives none when a field cannot have its value.
A box whose fields cannot all be given their values gives no witness. A
subject that is not a field, such as `x * 2`, is not solved for: the fields
it reads get the values of a supported input, and the witness shows the
finding only if they happen to satisfy the box.

Which input the core chooses is part of its behaviour, like an explanation's
wording, and may change between versions; conformance vectors check what a
witness shows, not its values.

## What was checked

The report lists:

- **`checks`**: one entry per check, in the order `types`, `fields`,
  `unsatisfiable`, `shadowed`, `overlap`, `gap`, with its **status**:
  `complete` when every item of the check was decided, `partial` when some
  were not, and `not_applicable` when the check does not apply (*Checks*).
  `types` is always `complete`: it reads the modelled tests only, and the
  others are listed in `not_modelled`.
- **`not_modelled`**: every test the audit does not model, and every node
  read as one because of a limit, in rule order and then in depth-first
  pre-order, with its rule, its `op` (`ast.md`, *Catalogue*), its `pointer`,
  and `limit` with the limit's name when a limit is the reason.
- **`undecided`**: every item a check left undecided, in check order and then
  in the order the check takes them (rules in rule order, pairs by their
  first rule and then by their second), with the `check`, the `rules`
  involved (none for `gap`) and a `reason`:
  - `not_modelled`: a rule involved has a not-modelled test, and neither the
    reasoning nor a witness decided it;
  - `no_witness`: the reasoning says it may happen, but no witness could be
    built or none showed it: a subject that is not a field, related paths,
    no number found between two bounds, or a witness whose evaluation raised
    an error (*Witnesses*);
  - `limit`: a limit was reached first; `limit` names it.

  When `max_steps` runs out, a check stops: the items it has not decided,
  and those it cannot decide without a step, are one entry, last for that
  check, with no `rules`, the reason `limit` (`max_steps`) and
  `remaining`, how many items it did not reach. Items decided without a step
  (an `unsatisfiable` rule, a rule with no rule ahead of it, a pair with an
  `unsatisfiable` rule) are still decided.

### Limits

| Limit | Default | Bounds |
|---|---|---|
| `max_terms` | 256 | the boxes of the `TRUE` or the `FALSE` description of one node. A node that needs more is read as a not-modelled test, listed in `not_modelled` with this limit |
| `max_steps` | 1,000,000 | the work of the whole audit: each intersection or difference of two boxes, and each condition evaluated on a witness (a witness the rule set evaluates costs one step per rule), is a step. Once they run out, every `and` and `or` still to be read is read as a not-modelled test, listed with this limit (other tests need no step), and each check stops as *What was checked* says |

Both are counts of at least 0. The report gives the limits it ran with.

## JSON

An audit document is the JSON form of an audit, with the content key
`"audit"` (`json-format.md`, *Documents*). Like an explanation, it is output
only: readers reject it with `unsupported`.

| Key | Required | JSON type |
|---|---|---|
| `source` | yes | `"ruleset"` or `"decision_table"`: the kind of rules audited, which may sit in a bundle or come from a rule document |
| `document` | when the rules came from another document | `"bundle"` or `"rule"`: the document that held them (*Documents*); absent when the audited document is the rule set or decision table itself |
| `hit_policy` | yes | the policy: a rule set's, or a decision table's (with `output_order`) |
| `aggregate` | with an aggregate | the aggregate |
| `logic` | yes | the logic |
| `strict` | when `true` | `true` |
| `ruleset_digest` | when asked for | string: the rule set digest of the rule set audited, or of the one the table compiles to (`json-format.md`, *Rule set digests*), as decisions and bundles carry it |
| `limits` | yes | object: `max_terms` and `max_steps`, ints |
| `rules` | yes | array, in rule order, of `{"name"}`, plus `id` when the rule has an id (`rules.md`, *Identity*) and `row`, the row's position in the table from 0, for a decision table |
| `checks` | yes | array of `{"check", "status"}` |
| `not_modelled` | yes | array of `{"rule", "op", "pointer"}`, plus `limit` |
| `undecided` | yes | array of `{"check", "rules", "reason"}`, plus `limit` for the reason `limit`, and `remaining` for the items `max_steps` did not reach |
| `diagnostics` | yes | array of diagnostics |

Every array is written even when it is empty. A **diagnostic** is an object
with:

| Key | Required | JSON type |
|---|---|---|
| `code` | yes | the code (*Checks*) |
| `severity` | yes | `"error"`, `"warning"` or `"info"` |
| `rules` | yes | array of rule names |
| `message` | yes | string: what was found and how to fix it, in words |
| `pointer` | for `incompatible_types` and `unlisted_field` | string |
| `path` | for `mixed_types` and `unlisted_field` | the field's path (*Conditions*) |
| `groups` | for `mixed_types` | array of type groups |
| `witness` | for `overlap` and `gap` | a data value |

Rules are named by their names, as in decisions and explanations. Each name
is a key into `rules`, which gives the rule's id and, in a table, its row, so
a tool that compares versions joins on the effective id (`rules.md`,
*Identity*). A row's position counts from 0, as in pointers, while a row
without a name is the rule `row_<n>`, counting from 1. Under `output_order`
the rules are in output priority order, not in row order (`tables.md`,
*Compiling*), and `rules`, diagnostics and undecided items follow rule
order.

A `pointer` is a JSON Pointer into the rule set or decision table document
of what was audited, as the core writes it: a node of a rule set, such as
`/ruleset/rules/0/when/operands/1`, or a cell of a decision table, such as
`/decision_table/rows/1/inputs/0`; within a cell, which is text, every test
points to the cell. `message`, like an error's, is free text that
conformance vectors do not compare.

**Extensions.** Within format version 1, new diagnostic codes, checks,
reasons, limits, sources (what can be audited) and optional keys may be
added. Consumers ignore those they do not know, and accept several
diagnostics of any code, `gap` included: this version reports at most one
gap, and a later one may describe each gap. The severities and the statuses
of a check (`complete`, `partial`, `not_applicable`) are closed sets, so a
command line can map severities to its exit codes. Severities are ordered by
seriousness, `info` below `warning` below `error`; a report is *at least* a
severity when one of its diagnostics is that severity or a more serious one.

```json
{
  "version": 1,
  "audit": {
    "source": "ruleset",
    "hit_policy": "unique",
    "logic": "boolean",
    "limits": { "max_terms": 256, "max_steps": 1000000 },
    "rules": [{ "name": "adult" }, { "name": "senior", "id": "AGE-2" }],
    "checks": [
      { "check": "types", "status": "complete" },
      { "check": "fields", "status": "not_applicable" },
      { "check": "unsatisfiable", "status": "complete" },
      { "check": "shadowed", "status": "not_applicable" },
      { "check": "overlap", "status": "complete" },
      { "check": "gap", "status": "complete" }
    ],
    "not_modelled": [],
    "undecided": [],
    "diagnostics": [
      {
        "code": "overlap",
        "severity": "error",
        "rules": ["adult", "senior"],
        "message": "rules adult and senior both fire for the witness, so the unique hit policy raises hit_policy_error: make their conditions exclusive, or use first or priority",
        "witness": { "age": 65 }
      },
      {
        "code": "gap",
        "severity": "warning",
        "rules": [],
        "message": "no rule fires for the witness, so the decision has no output: add a rule whose condition covers it; a rule with the condition TRUE would fire with every other rule",
        "witness": { "age": 0 }
      }
    ]
  }
}
```

## Documents

Auditing a document audits the rules it holds, with every pointer into that
document as written:

- a rule set or a decision table: as above;
- a bundle (`bundles.md`): the rule set or decision table it holds, with
  every pointer prefixed with `/bundle` (`/bundle/ruleset/rules/0/when`), as
  its rules are located there (`bundles.md`, *Rules and their digest*);
- a rule: the rule set of that one rule with the default settings
  (`json-format.md`, *A rule set from a document*), with `/rule` in place of
  `/ruleset/rules/0` (`/rule/when/operands/1`). The checks between rules,
  `shadowed`, `overlap` and `gap`, are `not_applicable`, and report nothing:
  a rule has no hit policy of its own, and a gap of the default one is not
  something the rule did. `types`, `fields` and `unsatisfiable` run as for a
  rule set;
- a condition or a case file (`tests.md`), which hold no rules:
  `unsupported`.

The report's `source` is the kind of rules audited, `ruleset` or
`decision_table`, and its `document` says when they came from a `bundle` or
a `rule` document (*JSON*).

A document read from the rule syntax (`text.md`) or from CSV (`tables.md`) is
located in the JSON form of the same document: `/ruleset/rules/2/when` is the
condition of its third rule. The text form (*Text*) also names a table's
cells by their row and column.

## Text

*Not checked by vectors: the core's snapshot tests fix it.*

The text of an audit, as the command line prints it, is generated by the
core:

```text
Audit of a rule set: 2 rules, hit policy unique, logic boolean
error    overlap  adult, senior: rules adult and senior both fire for the witness, ...
           witness: {"age":65}
warning  gap  no rule fires for the witness, ...
           witness: {"age":0}
Checks: types complete, fields not applicable, unsatisfiable complete, shadowed not applicable, overlap complete, gap complete
```

- A header with what was audited (`a rule set`, `a decision table`,
  followed by `in a bundle` for a bundle's), the number of rules and the
  configuration, as in a summary (`explain.md`, *Summaries*); for a rule
  document, `Audit of a rule` and its logic, since a rule has no hit policy.
- One line per diagnostic: its severity, its code, its rules and its message,
  followed by its pointer (`at`), path (`field`) and groups when it has them
  and its witness in compact JSON, each on an indented line; `No findings.`
  when there are none.
- In a decision table, a pointer to a row or a cell is written with the
  row's label and the column's header before it, as
  `row <label>, input <header> (<pointer>)`, `row <label>, output <name>
  (<pointer>)` or `row <label> (<pointer>)`, here and in `Not modelled`;
  the header of an input is its expression in the text syntax.
- `Checks:` with each check's status; then `Not modelled (<n>):` with each
  test (its rule, its `op` and its pointer, and the limit), and
  `Undecided (<n>):` with each item (its check, its rules, its reason, the
  limit and how many items were not reached), when there are any.
- Fields are written in the text syntax (`text.md`), as in messages.
