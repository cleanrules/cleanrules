# Condition AST

**Status:** draft. The node catalogue and the semantics below are normative for
every implementation. The JSON form of each node is defined in
[`json-format.md`](json-format.md) (format version 1), and the data that
conditions read in [`data-model.md`](data-model.md).

A rule's condition is a tree of immutable nodes. Users normally build it with
a field proxy (`F` in Python) and the operators `&`, `|` and `~`, or their
equivalent in each language. This document defines what the tree *means*, so
that every engine mode (row by row, Arrow batches, Rete) and every language
agree.

## Node kinds

Every node has a **kind**, which says where it may appear:

| Kind | Meaning | Operand of `and` / `or` / `not` | Operand of comparisons and arithmetic |
|---|---|---|---|
| `condition` | evaluates to a truth value (crisp or fuzzy) | yes | no |
| `value` | evaluates to a value | no | yes |
| `any` | depends on the data: fields and boolean constants | yes | yes |

Building a tree that breaks these rules raises `ExpressionError` immediately,
not later at evaluation time (`errors.md`). Comparing two conditions, as in
`(F.a > 1) == (F.b > 2)`, is rejected for this reason.

## Catalogue

Each node has a stable lowercase **tag**. Tags are used in serialized rules, in
errors and in every language.

| Tag | Fields | Kind |
|---|---|---|
| `field` | `path`: a list of segments, each a non-empty string or an integer ≥ 0 | any |
| `const` | `value`: a scalar value | any if boolean, else value; null only under `is_null` |
| `eq` `ne` `lt` `le` `gt` `ge` | `left`, `right` | condition |
| `and`, `or` | `operands`: at least two nodes | condition |
| `not` | `operand` | condition |
| `in` | `operand`, `values`: scalar values | condition |
| `between` | `operand`, `low`, `high`, `closed` | condition |
| `is_null` | `operand` | condition |
| `matches` | `operand`, `pattern`: a string | condition |
| `starts_with` `ends_with` `contains` | `operand`, `text`: a string | condition |
| `add` `sub` `mul` `div` | `left`, `right` | value |
| `abs`, `neg` | `operand` | value |

### Construction rules

- **`field`**: the path is non-empty. Each segment is a non-empty string, which
  looks up a key or attribute, or an integer ≥ 0, which indexes a sequence. A
  boolean is not an index.
- **`const`**: the value is a scalar of `data-model.md`: null, boolean, int,
  float, decimal, string, date or datetime. A native constant is converted with
  the tables of `data-model.md` (*Constants*), keeping decimals exact
  (infinities included) and time zones. These are rejected:
  - NaN, of any type: use `is_null`, because NaN in data is null;
  - an integer outside the signed 64-bit range, and a decimal that does not fit
    in 38 digits;
  - a datetime finer than the microsecond (data is truncated; a constant is
    rejected, because it was written on purpose);
  - anything that would be opaque.
- **`and`, `or`** are n-ary and **flattened** when built: an `and` operand that
  is itself an `and` is spliced in, and the same for `or`. They need at least
  two operands after flattening. Operand order is kept, because it determines
  explanation order.
- **Value operands.** These operands must be value or `any` nodes, never
  conditions:
  - both sides of a comparison and of `add` … `div`;
  - the operand of `in`, `is_null`, `matches`, the string tests, `abs` and
    `neg`;
  - all three nodes of `between`.

  A null constant is rejected everywhere except as the operand of `is_null`,
  including as an operand of `and`, `or` and `not`. Under Kleene logic,
  `x == null` or `x + null` would be null for every input; use `is_null` to test
  for missing values.
- **Arithmetic** (`add` … `div`, `abs`, `neg`) also rejects constants that are
  not numbers (booleans included), such as `"a" + 1`.
- **`in`** takes any collection of values the language offers (a list, a set…),
  but a string, bytes or a map is not a collection of values here. The values
  are converted like constants, and must be non-empty, non-null and all from
  one type group. They are stored **sorted and without duplicates** (equal int
  and float, such as `1` and `1.0`, are both kept, in a fixed order;
  `json-format.md`, *Writing*), so the node is deterministic and `x in [2, 1]`
  is the same node as `x in [1, 2]`.
- **`between`**: `closed` is `"both"` (the default), `"left"`, `"right"` or
  `"none"`. When both bounds are constants, they must belong to one orderable
  group (not boolean).
- **`matches`**: the pattern must be valid in the syntax defined under
  *`matches`* below, and compile within the size limit.
- **String tests**: the text is a literal string.

### Structural identity

Two trees are **structurally identical** when they have the same shape, tags
and constants. Every language offers this as a method (`same_as` and a
hashable `structural_key` in Python, and their idiomatic equivalents).

- Constants compare by **type and value**: the int `1`, the float `1.0`, the
  decimal `1` and `true` are four different nodes.
- `0.0` and `-0.0` are the same node, and so are the decimals `1.0` and `1.00`,
  although they may serialize differently.
- Two aware datetimes for the same instant in different zones are the same
  node, but they may serialize differently.

In languages with operator overloading, `==` on a node builds an `eq` node (as
it does in polars and SQLAlchemy), so structural identity always has its own
method, and nodes keep identity hashing. In Java, which has no operator overloading
(comparisons are methods such as `eq`), `equals` and `hashCode` are structural
identity. So they are in Scala, whose `==` always gives a `Boolean`: equality
nodes are built with `===` and `=!=`.

## Evaluation semantics

The core implements these rules, in every engine mode.

### Reading data

A `field` path is followed one segment at a time through the input's values
(`data-model.md`):

- a **string** segment, on a map, gives the value under that key if the key is
  present; keys starting with `_`, such as `"_id"`, are ordinary keys;
- an **integer** segment, on a list, gives the element at that index if
  `0 <= i < len`, and on a **record** (a map whose entries also have positions,
  such as a Python namedtuple or pandas row) the entry at that position; it
  never looks up a map key;
- anything else is **missing**: a string segment on a list or a scalar, an
  integer segment on a map, a string or a scalar, an absent key, an index out
  of range.

Native data is read by each binding with the same rules in its language's
terms: maps, records and objects' **public data attributes** for string
segments, sequences for integer segments, and never a private member, a method,
a class, a module, or an attribute of a scalar (`data-model.md`, *Reading native
data*). So a rule loaded from a file cannot reach into the process, and
`F.d.year` on a date is missing in every language.

If reading raises an error, the value is null with reason `read_error`, and
strict mode raises `EvaluationError` chained to the original error. Native data
beyond the `max_values` limit is not evaluated: the input is refused with
`limit_exceeded`, in every mode (`data-model.md`, *Reading native data*,
*Limits*).

### Truth values

Crisp logic is Kleene three-valued: `TRUE`, `FALSE` and `UNKNOWN`. A rule fires
only when its condition is `TRUE`. `and` and `or` are commutative; the table
lists each unordered pair once.

| `a` | `b` | `a and b` | `a or b` |
|---|---|---|---|
| T | T | T | T |
| T | F | F | T |
| T | U | U | T |
| F | F | F | F |
| F | U | F | U |
| U | U | U | U |

`not T = F`, `not F = T` and `not U = U`.

Under a **graded** logic, one of the fuzzy logics of *Logics*, the
connectives are the logic's t-norm `T`, s-norm `S` and complement `N`, and a
truth value is an **interval** `[lo, hi]` of degrees, `0 ≤ lo ≤ hi ≤ 1`:

- A known degree `d` is `[d, d]`, and `UNKNOWN` is `[0, 1]`. `TRUE` is
  `[1, 1]` and `FALSE` is `[0, 0]`: comparisons, `in`, `between`, `is_null`,
  `matches` and the string tests are crisp, and give one of these three.
- Each connective is applied to the bounds: `[a, b] and [c, d] =
  [T(a, c), T(b, d)]`, and the same with `S` for `or`. `not [a, b] =
  [N(b), N(a)]`. An `and` or `or` with more operands is a left fold from the
  first one: `x and y and z` is `(x and y) and z`.
- This works because norms are monotone and complements reverse the order. It
  is associative for every norm, reduces exactly to Kleene's tables on
  `[0, 0]`, `[0, 1]` and `[1, 1]` (*Logics* keeps the boundary conditions of
  the norms exact), and matches fuzzytool's interval evaluation
  (`Antecedent.eval_interval`).
- An interval with `lo < hi` is **undetermined**, and carries a reason like
  `UNKNOWN` under Kleene logic (*Results and reasons*). Where rounding would
  put `hi` below `lo`, `hi` is `lo`.
- A rule fires to degree `lo`, so an undetermined interval never fires fully.
  How rules fire under a graded logic (firing degrees and the activation
  threshold) is defined with them (phase J, task J.2); until then a rule set
  or decision table whose logic is graded is `unsupported` (`json-format.md`,
  *Rule sets*), and graded logics evaluate one condition at a time
  (*Evaluating one node*).

No single number works instead:

- `0.5` would partly fire rules on missing data;
- `0` makes `not x` fire when `x` is missing;
- NaN breaks the laws;
- a Kleene-style sentinel breaks associativity under Łukasiewicz.

**Data values used as conditions** under a graded logic: `true` and `false`
are `[1, 1]` and `[0, 0]`, null is `UNKNOWN`, and a number in `[0, 1]` is a
degree (`data-model.md`, *Fuzzy degrees*). Any other value, a number outside
`[0, 1]` included, is a type mismatch.

Results never depend on evaluation order. An implementation may skip evaluating
an operand only when that cannot change the result, and no operand can turn a
result into an error that another order would not raise (see *Types*).

### Logics

A logic is chosen by its **name**, which fuzzy logics complete with a
**parameter** and a **complement** (their JSON form is in `json-format.md`,
*Logics*). Names, of logics and of complements, are lowercase and matched
exactly. fuzzytool's aliases are read too, as the names they stand for:
`product` for `prod`, and `zadeh` for the `standard` complement; they are
never written. A name that is not registered is a `registry_error` with the
name and the canonical names that exist (`errors.md`).

`boolean` is Kleene's logic (*Truth values*). It takes no parameter. Every
other logic is **fuzzy**, with the capability `Graded`: a t-norm `T` (`and`),
its dual s-norm `S(a, b) = 1 − T(1 − a, 1 − b)` (`or`), and a complement `N`
(`not`), all on degrees in `[0, 1]`. They are fuzzytool's (`fuzzytool.norms`),
which is the reference for their values:

| Name | `T(a, b)` | `S(a, b)` | Parameter |
|---|---|---|---|
| `min` | `min(a, b)` | `max(a, b)` | none |
| `prod` | `a·b` | `a + b − a·b` | none |
| `lukasiewicz` | `max(0, a + b − 1)` | `min(1, a + b)` | none |
| `einstein` | `a·b / (2 − (a + b − a·b))` | `(a + b) / (1 + a·b)` | none |
| `hamacher` | `a·b / (γ + (1 − γ)·(a + b − a·b))` | `(a + b − 2·a·b + γ·a·b) / ((1 − a·b) + γ·a·b)` | `gamma` ≥ 0, default 0 |
| `drastic` | `b` if `a = 1`, `a` if `b = 1`, else 0 | `b` if `a = 0`, `a` if `b = 0`, else 1 | none |
| `nilpotent` | `min(a, b)` if `a + b > 1`, else 0 | `max(a, b)` if `a + b < 1`, else 1 | none |
| `yager` | `max(0, 1 − ((1 − a)^p + (1 − b)^p)^(1/p))` | `min(1, (a^p + b^p)^(1/p))` | `p` > 0, required |
| `dombi` | `1 / (1 + ((1/a − 1)^p + (1/b − 1)^p)^(1/p))` | `1 / (1 + ((1/a − 1)^−p + (1/b − 1)^−p)^(−1/p))` | `p` > 0, required |
| `frank` | `log_s(1 + (s^a − 1)·(s^b − 1) / (s − 1))` | `1 − T(1 − a, 1 − b)` | `s` > 0 and ≠ 1, required |

`hamacher` with `gamma` 0 is fuzzytool's Hamacher product and sum; 1 gives
`prod` and 2 `einstein`. The complement is the standard one unless the logic
names another:

| Complement | `N(a)` | Parameter |
|---|---|---|
| `standard` | `1 − a` | none |
| `sugeno` | `(1 − a) / (1 + lam·a)` | `lam` > −1, required |
| `yager` | `(1 − a^w)^(1/w)` | `w` > 0, required |

Degrees are binary64 floats, and the formulas are evaluated as fuzzytool
evaluates them:

- in the order of operations of the tables (`a + b − a·b` is
  `(a + b) − (a·b)`, and `S` of `frank` is computed from `T` of `1 − a` and
  `1 − b`);
- with fuzzytool's guards: a `hamacher` denominator of at most `1e-12` gives 0
  for `T` and 1 for `S`; `dombi` reads a degree below `1e-12` as `1e-12` in
  `T`, and clamps degrees to `[1e-12, 1 − 1e-12]` in `S`; `frank` reads an
  argument of the logarithm below `1e-12` as `1e-12`;
- with `x^1 = x`, `x^2 = x·x`, `x^0.5 = √x` and `x^−1 = 1/x`, as NumPy
  computes them on arrays, and every other power and logarithm with the
  algorithms of musl's libm (the core uses the `libm` crate), never the
  platform's, so every platform computes the same bits. fuzzytool uses the
  platform's, so the two may differ in the last places there.

Two rules come before the formulas, so that crisp values combine exactly as
in Kleene's tables. First, the **boundary conditions** of norms hold exactly:
`T(a, b)` is 0 when `a` or `b` is 0, and otherwise `b` when `a` is 1 and `a`
when `b` is 1; `S(a, b)` is 1 when `a` or `b` is 1, and otherwise `b` when `a`
is 0 and `a` when `b` is 0. fuzzytool's formulas only approximate some of
them in floating point (Łukasiewicz's `T(1, 0.1)` gives `0.10000000000000009`,
`dombi`'s `S(0, 0)` about `2e-12`, and `frank`'s `T(1, 1)` may give
`0.9999999999999998`). Second, every result is **clamped** to `[0, 1]`, and a
negative zero is 0.

A parameter is a finite number in its range. The logics without a parameter
take none, `boolean` takes no complement either, and `yager`, `dombi` and
`frank` cannot be named without theirs. A parameter or complement that breaks
these rules is a `rule_definition_error` naming the logic (`logic "yager"`) or
the complement and the parameter; the logic's parameter is checked before the
complement's.

Where logics are used:

- `evaluate_tree` takes any logic (*Evaluating one node*);
- a rule set or decision table takes `boolean` only, and a graded logic there
  is `unsupported` until rules fire to degrees (task J.2; `json-format.md`,
  *Rule sets*).

### Missing values

- A `field` whose path cannot be resolved evaluates to **null**, the same as an
  explicit null. Strict mode raises `MissingFieldError` instead, even under
  `is_null`.
- A null met **in the middle** of a path (`F.customer.age` with `customer`
  null) makes the field null, even in strict mode: the path exists up to a
  null. A null input is null at the empty path ("the input"). The exception is
  a null that stands for a **read error** (reading `customer` raised): the field
  is null with reason `read_error`, and strict mode raises, as when the error
  is met at the end of the path (*Reading data*). A failure to read never
  passes for an ordinary null. *Not checked by vectors: read errors only come
  from native data, so each binding's tests check this.*
- **NaN read from the data is null**, whatever its type (`data-model.md`).
  Languages disagree on how NaN compares and orders, so treating it as missing
  is the only portable choice. NaN produced by arithmetic, such as
  `inf - inf`, is null too.
- Null is checked **before** types. Any comparison, string test or arithmetic
  operation with a null operand yields `UNKNOWN` (or null, for arithmetic),
  never an error. The only exception is `is_null`, which is never `UNKNOWN`.
- A `field` or `const` used directly as a condition is converted by the active
  logic:
  - Kleene logic (`"boolean"`) maps `true`/`false` to `TRUE`/`FALSE` and null
    to `UNKNOWN`; any other type is a type mismatch (see below);
  - fuzzy logics also accept numbers in `[0, 1]` as degrees.

### Types

Values belong to one of these comparison groups (`data-model.md`, *Values*):

- **number**: ints, floats and decimals;
- **string**;
- **boolean**;
- **date**;
- **naive_datetime**;
- **aware_datetime**;
- **other**: lists, maps and opaque values.

Rules:

- Ordering (`lt`, `le`, `gt`, `ge`) is defined within number, string (by Unicode
  code point), date and both datetime groups, but not for booleans.
- Comparing values from different groups, comparing anything with a value of
  the `other` group, or ordering booleans, is a **type mismatch**:
  - **Default:** the comparison is `UNKNOWN`, and the result records the reason
    (for example, "type mismatch: string vs number") so `explain()` shows it. It
    is never a silent `FALSE`, and one bad row never aborts a whole batch.
  - **Strict mode:** it raises `TypeMismatchError` with the field path and both
    types.
- Ints, floats and decimals compare **exactly**, by value: `2⁵³ + 1` does not
  equal the float `2⁵³`, and the decimal `0.1` does not equal the float `0.1`.

### Node semantics

- **`in`**:
  - `values` is a non-empty list of non-null scalars, all from **one** type
    group; this is checked at construction, and to test for null you combine
    it with `is_null`;
  - the result is `TRUE` if the operand equals one of the values, `FALSE`
    otherwise, and `UNKNOWN` if the operand is null or from another group.
- **`between`**:
  - `low` and `high` are nodes, so `F.x.between(F.lo, F.hi)` is allowed; audit
    is exact when they are constants;
  - `closed` is `"both"` (the default), `"left"`, `"right"` or `"none"`;
  - it is equivalent to the matching pair of comparisons joined by `and`.
- **`matches`** uses **search** semantics: it is `TRUE` when the pattern
  matches anywhere in the string.
  - The syntax is that of the Rust [`regex`](https://docs.rs/regex) crate, 1.x,
    with its defaults: Unicode on, case-sensitive, `^` and `$` match only at
    the start and end of the text (`$` never before a trailing newline), and
    `.` does not match `\n`. Inline flags such as `(?i)` change this within the
    pattern. There is no lookaround and there are no backreferences, so
    matching takes linear time (`CLAUDE.md`, invariant 9; decision D9).
  - A pattern that does not parse, or whose compiled form exceeds the crate's
    default size limit, is an `ExpressionError`.
- **`starts_with`, `ends_with`, `contains`** take **literal** text, never a
  pattern.
- **Arithmetic** (`add`, `sub`, `mul`, `div`, `abs`, `neg`) is defined on
  numbers:
  - `div` converts both operands to float64 **first**, then divides, so it
    always returns a float;
  - decimal operands are converted to float64 (correctly rounded,
    `data-model.md`) for every operation;
  - **division by zero yields null**, for int and float alike;
  - an integer result outside the signed 64-bit range yields null;
  - a NaN result is null, while `inf` and `-inf` results are kept;
  - an operand that is not a number makes the result null with reason
    `type_mismatch` (strict mode raises `TypeMismatchError`).

## Results and reasons

Every node evaluates to a truth value (conditions) or a value or null (values).
When a truth value is `UNKNOWN` (or undetermined, under a graded logic) or a
value is null, a **reason** says why, and only then: a `TRUE`, a `FALSE` or a
degree never carries one. Implementations are compared
on the reason's **code**, the **field paths** involved and the type **groups**;
the human-readable detail may differ.

| Code | Meaning |
|---|---|
| `missing_field` | the path cannot be resolved |
| `null` | a null value (or NaN) was met; the path says where |
| `type_mismatch` | values of incompatible groups, or a value of the `other` group, met; or a non-boolean used as a condition |
| `unorderable` | booleans were ordered with `<`, `<=`, `>` or `>=` |
| `division_by_zero` | a division by zero |
| `int_overflow` | an integer outside the signed 64-bit range |
| `decimal_overflow` | a decimal in the data that does not fit (`data-model.md`, *Decimals*) |
| `nan_result` | an arithmetic result is NaN |
| `read_error` | reading the data raised an error |

A reason's **paths** and **types** are lists whose order and duplicates are
part of the contract:

- **paths**:
  - for `missing_field` and `null` met while reading a field, the path read up
    to the point where it failed or met the null;
  - for `read_error`, the path being read;
  - for a field used as a condition that is not a boolean, that field's path,
    and none for a constant;
  - otherwise, the paths of the fields among the node's direct children, in
    field order, duplicates kept (`F.a > F.a` gives two paths).
- **types**:
  - for comparisons and arithmetic, the groups of the left and right values,
    in that order, duplicates kept (`unorderable` is always `boolean`,
    `boolean`);
  - for `abs` and `neg`, the operand's group and `number`;
  - for `in`, the operand's group and the values' group;
  - for `matches` and the string tests, the operand's group and `string`;
  - for a field or constant used as a condition, its group and `boolean`;
  - empty for every other code.

A condition that is `UNKNOWN` takes the reason of its first `UNKNOWN` operand,
and under a graded logic an undetermined one takes the reason of its first
undetermined operand (*Truth values*).
A `between` is reported as its two half comparisons, so explanations can say
which bound failed.

**Strict mode** raises the first error found in depth-first, left-to-right
post-order: children before their parent, operands in order. Every engine mode
raises the same error; in a batch, it is the first failing input in order
(`errors.md`). Strict mode raises only for missing fields, type mismatches
(`unorderable` included) and read errors: nulls, division by zero, overflows
and NaN results stay nulls with their reasons.

## Evaluating one node

Besides rules, every language can evaluate one node on an input and return its
result tree (`json-format.md`, *Result trees*): `evaluate_tree` for a condition,
with the options `logic` and `strict`, and `evaluate_value_tree` for a value,
with `strict` (Python names; `evaluateTree` and `evaluateValueTree` where
camel case is the idiom). The conformance vectors of kinds `condition` and
`value` use them. `logic` is any logic of *Logics*, graded ones included, by
name or in its JSON form (`json-format.md`, *Logics*); `boolean` by default.

## Notes for the Arrow mode (phase I)

*Informative.* Columnar engines cast implicitly where this spec does not, so
the vectorised mode must be **schema-aware**:

- never upcast dates to datetimes, never cast booleans to integers, and compare
  ints with floats exactly, not as floats (they differ above 2⁵³);
- turn NaN into null in float columns only;
- widen `Float32` columns to `Float64`, and never narrow constants;
- compare decimal columns exactly with constants, and convert them to float for
  arithmetic;
- map division by zero, which columnar engines turn into `inf` or NaN, to null;
- turn type mismatches into `UNKNOWN` per row, with the reason recorded.
