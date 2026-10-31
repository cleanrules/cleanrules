# Rules, rule sets and decisions

**Status:** draft, normative for every engine (Python reference, Rust). The
serialized form is defined in `json-format.md` (*Rules*, *Rule sets*).

## Rules

A rule has:

- a unique, non-empty **name**;
- a **condition** (`when`): a node of kind `condition` or `any` (`ast.md`, *Node
  kinds*), such as a comparison, a field, or a boolean constant. Value nodes,
  such as number or string constants and arithmetic, are rejected when the rule
  is built;
- a **payload** (`then`, optional): string keys mapped to value nodes, in the
  order they were defined. Python constants are normalized to `Const` when the
  rule is built, so a payload is defined with scalars and value expressions. A
  field in a payload passes through whatever the data holds, which may be a
  non-scalar value such as a list (type group `other`);
- an **action** (`do`, optional): a callable. Actions are not part of the
  decision (see *Actions*);
- a **priority** (a signed 64-bit integer, default 0; not a boolean), **tags**
  (strings), a **description** (a string) and **metadata** (string keys mapped
  to data values, so it can be serialized; `json-format.md`), none
  of which affects evaluation except the priority, through `PRIORITY`.

## Evaluation

For one input, every rule's condition is evaluated in rule order with the rule
set's logic and strictness (`ast.md`). A rule **fires** when its truth
value passes the logic's **activation test**: `TRUE` under Kleene logic. Under a
graded logic (phase 5) the test will be "degree ≥ the rule set's alpha"; the
definition of *fires* does not change.

The **hit policy** then selects rules among those that fired:

| Policy | Selected | Error |
|---|---|---|
| `FIRST` | the first fired rule | — |
| `PRIORITY` | the fired rule with the highest priority; ties go to rule order | — |
| `UNIQUE` | the fired rule | more than one fired |
| `ANY` | the first fired rule | fired rules have different payloads |
| `RULE_ORDER` | every fired rule, in rule order | — |
| `COLLECT` | every fired rule, in rule order | — |

Errors are `HitPolicyError` and list the rules involved, in rule order.

### When payloads are evaluated

Payload values are evaluated against the same input, with the same strictness,
**only for the selected rules**, plus every fired rule under `ANY`, whose payloads
must be compared. Except under `ANY`, a rule that fired but was discarded by the
policy therefore never raises in strict mode because of its payload. Within a
payload, values are evaluated in key order. A null payload value keeps its reason
(the value's result tree), like any other value.

**`ANY` equality**: two payloads are equal when they have the same keys and, for
every key, either both values are null, or they have the same type group and are
equal:

- values compare as in `ast.md` (*Types*), so `1` equals `1.0`, but `1` does not
  equal `True` and `"1"` does not equal `1`;
- `int`, `float` and `Decimal` compare exactly, so `Decimal("0.1")` does not
  equal `0.1`;
- aware datetimes compare by instant, whatever their time zone;
- unlike in conditions, null equals null here, whatever the reason;
- values of the `other` group (lists, maps and opaque values) compare as
  `data-model.md` (*Equality*) says: lists element by element, maps key by key
  whatever their order, and opaque values by identity.

### Outputs

- `outputs`: the payloads of the selected rules, in rule order (`{}` for a rule
  without a payload). Each payload keeps its keys in definition order. Outputs
  are read-only.
- `output`: for single-hit policies (`FIRST`, `PRIORITY`, `UNIQUE`, `ANY`), the
  winner's payload, or null when nothing was selected. For `COLLECT` with an
  aggregate, the aggregate. For `RULE_ORDER` and `COLLECT` without an aggregate,
  asking for `output` is an error: there are several, read `outputs`.

### `COLLECT` aggregates

`aggregate` is `sum`, `min`, `max` or `count`, applied **per payload key** over
the selected payloads. Within a key:

- null values are skipped, as in SQL. `count` counts the non-null values; `sum`,
  `min` and `max` of no values are null;
- `sum`, `min` and `max` need numbers (not booleans) and raise `HitPolicyError`
  otherwise;
- `Decimal` values are converted to float64 before `sum`, `min` and `max`, as
  arithmetic does with `Decimal` operands;
- the result follows the arithmetic rules: an integer `sum` outside the signed
  64-bit range is null, and a NaN `sum` (such as `inf + -inf`) is null;
- keys appear in the order they are first met, in rule order;
- with no selected rule at all, the aggregate is null.

### Deviations from DMN

These deviations are deliberate:

- **Aggregates are per payload key.** DMN aggregates a single output column.
- **`count` counts non-null values.** DMN engines differ on whether `C#` counts
  matches or distinct outputs.
- **`PRIORITY` uses each rule's `priority`**, not DMN's ordered list of output
  values. Decision tables (task 3.5) compile output-value priorities into rule
  priorities.
- **`COLLECT` without an aggregate keeps rule order**, because results must be
  deterministic; DMN leaves the order unspecified. It is then equivalent to
  `RULE_ORDER`.

## Strict mode and errors

In strict mode, every condition is evaluated first, in rule order, and conditions
raise as specified in `ast.md`. Payloads come next, and raise in the order they
are evaluated: under `ANY`, the payloads of every fired rule, in rule order;
otherwise those of the selected rules, in rule order; within a payload, in key
order. A hit-policy error comes last, after all conditions and the needed
payloads have been evaluated. One exception: `UNIQUE` raises as soon as more
than one rule has fired, before any payload is evaluated.

When a batch is evaluated (`evaluate_many`), an `EvaluationError` carries the
position of the input (`row`). Passing something whose iteration does not yield
inputs (a string, a mapping, a DataFrame) is an error raised immediately.

## Actions

`evaluate` is pure: it never runs actions. A decision can be computed any number
of times, for explanations, validation or model predictions, without side
effects. `run` evaluates and then calls the actions of the **selected** rules, in
rule order, each with its rule, the input, the condition's result and the
evaluated payload. The first action that raises stops the run with `ActionError`,
which carries the decision and the names of the actions that had already run.
