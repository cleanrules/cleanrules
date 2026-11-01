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
method, and nodes keep identity hashing.

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
strict mode raises `EvaluationError` chained to the original error.

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

Under fuzzy logic the connectives are the configured t-norm, s-norm and
complement, and truth values are degrees in `[0, 1]`.

**Unknown under fuzzy logic** (*proposed, pending the author's confirmation;
needed by phase J*): a fuzzy truth value is an interval `[lo, hi]` of degrees.

- A known degree `d` is `[d, d]`, and `UNKNOWN` is `[0, 1]`.
- Each connective is applied to the endpoints: `[a, b] and [c, d] =
  [T(a, c), T(b, d)]`, and the same with the s-norm for `or`. `not [a, b] =
  [N(b), N(a)]`.
- This works because norms and complements are monotone. It is associative for
  every norm, reduces exactly to Kleene's tables on `{0, 1}`, and matches
  fuzzytool's interval evaluation (`Antecedent.eval_interval`).
- A rule fires to degree `lo`; an undetermined interval never fires fully.

No single number works instead:

- `0.5` would partly fire rules on missing data;
- `0` makes `not x` fire when `x` is missing;
- NaN breaks the laws;
- a Kleene-style sentinel breaks associativity under Łukasiewicz.

**Data values used as conditions** under a fuzzy logic: numbers in `[0, 1]` are
degrees, and a number outside that range is a type mismatch.

Results never depend on evaluation order. An implementation may skip evaluating
an operand only when that cannot change the result, and no operand can turn a
result into an error that another order would not raise (see *Types*).

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
When a truth value is `UNKNOWN` or a value is null, a **reason** says why, and
only then: a `TRUE` or `FALSE` never carries one. Implementations are compared
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

A condition that is `UNKNOWN` takes the reason of its first `UNKNOWN` operand.
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
`value` use them.

## Differences from the Python v1

- A null constant under `and`, `or` and `not` was accepted by the v1; it was a
  gap, not semantics.
- Patterns follow the Rust `regex` syntax. Python's `re` accepted some patterns
  this syntax rejects, and its `$` also matched before a trailing newline.
  Vectors taken from the v1 skip those cases.

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
