# Explanations and summaries

**Status:** draft, normative for every implementation. It is derived from the
Python v1 (`rulepy.core.explain` and `rulepy.core.summary`, in the git tag
`python-reference`). The JSON form of both is in
[`json-format.md`](json-format.md) (*Explanations*, *Summaries*).

Every decision explains itself (`CLAUDE.md`, invariant 8). An explanation is
built only from a decision's result trees (`json-format.md`, *Result trees*)
and its rule set: nothing is evaluated again. A summary describes a rule set
without evaluating it.

Explanations support Kleene truth values only. Explaining a decision made with
another logic is `unsupported` until graded explanations arrive with the fuzzy
logics (phase J).

## Rule status

Each rule of the decision gets one status:

| Status | Meaning | Text mark |
|---|---|---|
| `selected` | the hit policy selected it | `[x]` |
| `fired` | it fired, but the hit policy did not select it | `[x]` |
| `not_fired` | its condition is `FALSE` | `[ ]` |
| `unknown` | its condition is `UNKNOWN` | `[?]` |

## Requirements

For each rule the explanation gives a **requirement**: the elementary
conditions behind its outcome. For a rule that fired, it says what makes it
fire; for one that did not, what would have to change for it to fire.

A requirement is either a **cause** (one elementary condition) or a **group**
of at least two requirements, of kind `all` (each one matters) or `any` (one is
enough).

### Building the requirement

The requirement is built from the root of the rule's `when` result tree, with a
flag `negated` that starts false. For a result `r`:

- **`not`**: the requirement of its operand, with `negated` flipped.
- **`and`, `or` and `between`**: a group, described below. A `between` is the
  conjunction of its two halves.
- **Anything else** (a comparison, `in`, `is_null`, a string test, a field or
  constant used as a condition, or a `between` half): a cause for `r`, with the
  current `negated`.

For a connective, De Morgan's laws swap its role under an odd number of
`not`s. Let the connective be **conjunctive** when it is `and` or `between` and
`negated` is false, or it is `or` and `negated` is true. Let the **effective**
truth of a result be its truth, negated (Kleene `not`) when `negated` is true.
The group's kind and the operands it keeps depend on the effective truth `v` of
the connective:

| Conjunctive | `v` | Kind | Operands kept |
|---|---|---|---|
| yes | `FALSE` | `all` | every operand whose effective truth is not `TRUE`: fixing only the false ones would leave the result unknown |
| yes | `UNKNOWN` | `all` | every operand whose effective truth is `UNKNOWN` |
| yes | `TRUE` | `all` | every operand |
| no | `TRUE` | `any` | every operand whose effective truth is `TRUE` |
| no | `UNKNOWN` | `any` | every operand whose effective truth is `UNKNOWN` |
| no | `FALSE` | `any` | every operand |

Each kept operand becomes a requirement in the same way, with the same
`negated`, in operand order.

### Simplifying

The requirement is then simplified, bottom up. In each group:

1. a nested group of the same kind is spliced into it;
2. repeated items are merged, keeping the first. Two causes are the same when
   their nodes are structurally identical (`ast.md`, *Structural identity*)
   and they have the same `negated`. Two groups are the same when they have
   the same kind and the same set of items;
3. **absorption**: a nested group is dropped when one of its own items is also
   a direct cause of this group (`a and (a or b)` needs only `a`, and
   `a or (a and b)` only `a`);
4. a group left with one item is replaced by that item.

Different conditions are treated as **independent**, even on the same field:
for `(x > 5) and (x < 3)` with `x = 8`, the explanation lists only `x < 3`,
although no `x` satisfies both.

### Keeping only the changes that work

For a rule that is `not_fired` or `unknown`, every way to make it fire is
checked against the whole tree, because a condition can occur in several
places, possibly under a `not`.

1. The **options** of a requirement are its ways to be satisfied: a cause has
   the single option `{cause}`; an `any` group, the options of each item, in
   order; an `all` group, every combination of one option per item, in order.
   If there are more than **64** options at any step, the requirement is kept as
   it is and stays achievable.
2. An option is **valid** when forcing its causes makes the rule fire. The
   rule's `when` tree is recombined with Kleene logic through `not`, `and`,
   `or` and `between` (`and` and `between` as the minimum, `or` as the
   maximum, with `FALSE < UNKNOWN < TRUE`). Every other node reached is a leaf:
   a leaf whose node is structurally identical to a cause of the option takes
   the truth value the rule needs from that cause (`TRUE`, or `FALSE` when
   `negated`), and every other leaf keeps its truth from the decision.
   Comparisons are never evaluated again. The option is valid when the root is
   `TRUE`. An option that needs the same node both true and false is never
   valid.
3. If every option is valid, the requirement is kept. If none is, the
   requirement is kept and the rule is **not achievable**: changing these
   conditions alone cannot make it fire, because they conflict (`a and not a`).
   Otherwise the requirement becomes an `any` group of the valid options, each
   option an `all` group of its causes (or the cause alone), in order, and is
   simplified again.

### Causes

A cause records:

- its node and truth value;
- `negated`: whether the rule needs it **false**;
- `holds`: whether its truth value is the one the rule needs (`TRUE`, or
  `FALSE` when `negated`);
- the **values** it read: for every field under it, in depth-first pre-order,
  the value read, when it has no reason (missing and null values are reported
  by the reason instead). Each path appears once, with its first value;
- its **reason**, when its truth value is `UNKNOWN`;
- a **counterfactual**, when it does not hold (below).

## Counterfactuals

A counterfactual says what a field would have to satisfy, and how far its value
is from it. It is given for a cause that does not hold and is not `UNKNOWN`,
whose node is a comparison between one field and one constant:

1. If the constant is on the left, the comparison is flipped so the field is on
   the left (`5 < x` becomes `x > 5`).
2. If the cause is `negated`, the comparison is negated (`>` becomes `<=`,
   `==` becomes `!=`): it is what the field must satisfy.
3. For `eq` and `ne` there is no distance: a counterfactual without a gap is
   given only when the cause is `negated`, because only then does the needed
   comparison differ from the rule's; otherwise there is none.
4. For an ordering, the **gap** is `target − value` for `gt` and `ge`, and
   `value − target` for `lt` and `le`. A gap is `0` for a strict comparison
   exactly at its limit. When no gap can be computed (below), a counterfactual
   without a gap is given only when the cause is `negated`.

A gap is computed only between values of the same kind, both finite. Its
arithmetic is its own, not that of `ast.md`, because a gap must be exact
whenever it can be:

- two ints: their difference, checked in 64 bits;
- a float and an int or a float: the difference in float64;
- two decimals, or a decimal and an int: the exact difference, as a decimal
  of `data-model.md` (38 digits), with the larger of the two scales (an int has
  scale 0), so `10.05 − 9.9` is `0.15`;
- a decimal and a float: no gap;
- two dates: a whole number of days;
- two naive datetimes: the difference of their wall-clock times, in
  microseconds;
- two aware datetimes: the difference between the two instants (UTC), in
  microseconds.

A difference that does not fit its type, or is not finite, gives no gap.

A counterfactual records the field's path, the comparison it needs, the
target constant, the gap if any, and whether the cause is `negated`.

## Notes for rules that fired but were not selected

A rule with status `fired` gets a **note** saying why the policy chose another
rule, the **winner** (the first selected rule):

| Note | When |
|---|---|
| `lower_priority` | `"priority"`, and the winner's priority is higher |
| `same_priority` | `"priority"`, and the winner has the same priority and comes first |
| `any_agrees` | `"any"`: the outputs agree, and the policy reports the first |
| `earlier_rule` | any other policy: the winner comes first |

Under `"rule_order"` and `"collect"` every rule that fired is selected, so no
rule gets a note.

## Text

*Not checked by vectors: the core's snapshot tests fix the text (`CLAUDE.md`,
invariant 8).*

`explain()` also renders the explanation as text. The text is generated by the
core, so it is the same in every language, and its exact form is fixed by
snapshot tests of the core, not by this document. Its layout:

```text
HIT POLICY priority: selected e
[ ] a (not fired): x > 10 AND NOT y <= 3
    would fire if all of these changed:
    - x > 10 is false (x = 8); short by 2
    - y <= 3 is true (y = 2), but the rule needs it false; needs y > 3 (short by 1)
[ ] b (not fired): x BETWEEN 0 AND 5
    - x <= 5 is false (x = 8); over by 3
[ ] c (not fired): x > 1 AND NOT x > 1
    cannot fire by changing these conditions alone; they conflict:
    - x > 1 is true (x = 8), but the rule needs it false; needs x <= 1 (over by 7)
[x] d (fired, not selected): x > 1
    fired, but e has a higher priority (5 > 1)
    - x > 1 is true (x = 8)
[x] e (selected): x > 0
    - x > 0 is true (x = 8)
```

(The decision of `{"x": 8, "y": 2}` under the rule set
`HIT POLICY priority`, `RULE a IF x > 10 AND NOT y <= 3`,
`RULE b IF x BETWEEN 0 AND 5`, `RULE c IF x > 1 AND NOT x > 1`,
`RULE d PRIORITY 1 IF x > 1` and `RULE e PRIORITY 5 IF x > 0`.)

- A header with the policy (and `AGGREGATE <aggregate>` when there is one) and
  the selected rules, or `no rule`: `HIT POLICY collect AGGREGATE sum: selected
  a, b`.
- Per rule, in rule order: the mark, the name, the status (`selected`,
  `fired, not selected`, `not fired` or `unknown`) and the condition, then the
  rule's description, the note, and the requirement, each indented by four
  spaces. A single cause is listed alone. A group is introduced by
  `because all of these hold:` (or `any`) for a rule that fired, and by
  `would fire if all of these changed:` (or `any`) otherwise, and nested groups
  read `- all of:` or `- any of:`, their items indented by two more spaces. A
  rule that is not achievable says
  `cannot fire by changing these conditions alone; they conflict:`.
- A note reads `fired, but <winner> has a higher priority (<p> > <q>)`,
  `fired, but <winner> has the same priority (<p>) and comes first`,
  `fired; its output agrees with <winner>, which comes first` (`any`) or
  `fired, but <winner> comes first`.
- A cause reads `<condition> is <true|false|unknown>`, with the values read in
  parentheses (`path = value`), then the reason after a colon when it is
  unknown, or `, but the rule needs it false` when it does not hold and is
  `negated`, then the counterfactual after a semicolon: `short by <gap>`
  (`gt`, `ge`) or `over by <gap>` (`lt`, `le`), `at the limit (needs …)` for a
  gap of zero, and `needs <field> <op> <target>` when there is no gap or the
  cause is `negated` (`needs y > 3 (short by 1)`).
- A reason reads `missing field <path>`, `<path> is null` (`null value` without
  a path), `type mismatch: <detail>` or `type mismatch: <group> vs <group>`,
  `cannot order booleans`, `division by zero`, `integer overflow in <path>`,
  `decimal out of range in <path>`, `NaN result` or `cannot read <path>:
  <detail>`.

Conditions, field paths, rule names, tags, output keys and constants are written
as the canonical text writes them (`text.md`, *Printing*), so a condition
copied from an explanation reads back as the rule's own. The empty path is
written `the input`. Values read from the data are written as constants
(`x = 8`, `rate = 0.25e0`, `day = DATE "2026-10-29"`, `name = "Ana"`), lists as
`[…]` and maps as `{"key": …}`, and an opaque value as its type between angle
brackets (`<bytes>`). They are bounded so that one input cannot flood the text: a
string or map key longer than 60 characters shows its first 57 and `...` before
the closing quote, a list or map its first 6 items and `...`, and a list or map
nested two deep is written `[...]` or `{...}`. Text from outside the rules (an
opaque value's type name, the detail of a reason, such as the message of the
error that reading a field raised) is cut in the same way and written with the
escapes of JSON strings, without the quotes, so it stays on one line. A field key
with a control character, which has no canonical text, is written with JSON
escapes between backticks (`text.md`, *Printing*). Every line of the text is
therefore one rule, one line of its description, a note, a group header or a
cause.

Gaps are numbers, not constants: an int as digits, a decimal with its scale
(`0.15`), and a float rounded to 12 significant digits so that float noise does
not show (`0.05`, not `0.05000000000000004`), in plain notation when its
magnitude is 0 or at least `1e-4` and below `1e16` and in exponent notation
otherwise. A gap between dates is `<n> day` or `<n> days`; between datetimes,
days, hours, minutes and seconds, leaving out the zero ones (`1 hour 30 minutes
0.5 seconds`).

## Differences from the Python v1

- A gap between two ints that overflows 64 bits has none; the v1 had
  unbounded ints.
- A decimal gap is exact or absent; the v1 rounded it to 28 significant digits
  (Python's default decimal context).
- A gap between two aware datetimes in the same named zone is the difference
  between instants; the v1 subtracted wall-clock times across a daylight saving
  change (01:00 to 04:00 in Europe/Madrid on 2026-10-25 gave 3 hours, not 4).

## Summaries

`summary()` describes a rule set without evaluating it:

- its configuration: policy, aggregate, logic and strictness;
- its **rules**, in rule order: name, priority, output keys, whether the rule
  has an action, and the action's name when it has one;
- the **fields** it reads, in order of first appearance. Rules are visited in
  rule order; within a rule, the condition first, then the payload values in
  member order; within a node, in depth-first pre-order, with children in the
  key order of *Conditions* in `json-format.md` (for `between`: operand, low,
  high). A path that is a
  prefix of another is listed on its own (`customer` and `customer.age`). Each
  field lists the rules whose condition reads it, in rule order, and the
  `(rule, output key)` pairs whose payload value reads it, in rule and member
  order;
- the **outputs**: every payload key, in order of first appearance;
- the **tags**, sorted by Unicode code point, each with the names of the rules
  that carry it, in rule order.

Its text form is, like the explanation's, generated by the core and fixed by
snapshot tests:

```text
Rule set: 2 rules, hit policy priority, logic boolean
  priority  name   condition                             outputs
        10  vip    customer.years > 5 AND since IS NULL  discount, label
         5  loyal  customer.years > 2                    discount DO notify
Fields read (2):
  customer.years  conditions of vip, loyal
  since           conditions of vip
Outputs: discount, label
Tags: gold (vip)
```

- A header with the number of rules and the configuration: `hit policy`,
  `aggregate` when there is one, `logic`, and `strict` when set.
- A table of the rules, with aligned columns: priority, name, condition (cut to
  60 characters, the last three `...`) and output keys, followed by `DO` and
  the action's name when the rule names its action, or `+action` when its
  action is code, or `-` when it has neither; `(no rules)` for an
  empty rule set.
- `Fields read (<n>):` with, for each field, `conditions of <rules>` and
  `payloads of <rule> (<key>), …`, separated by `;`.
- `Outputs:` and `Tags:` (each tag with its rules), when there are any.
