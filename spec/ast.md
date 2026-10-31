# Condition AST

**Status:** draft. The node catalogue and the semantics below are normative for
every implementation. The JSON form of each node is defined in
[`json-format.md`](json-format.md) (format version 1).

A rule's condition is a tree of immutable nodes (`rulepy.core.expr`). Users
normally build it with the `F` proxy and the operators `&`, `|` and `~` (or
their equivalent in each language). This document defines what the tree
*means*, so that every engine mode (row by row, Arrow batches, Rete) and every
language agree. It still names the Python v1 classes, which phase A replaces
with neutral terms.

## Node kinds

Every node has a **kind**, which says where it may appear:

| Kind | Meaning | Operand of `and` / `or` / `not` | Operand of comparisons and arithmetic |
|---|---|---|---|
| `condition` | evaluates to a truth value (crisp or fuzzy) | yes | no |
| `value` | evaluates to a scalar | no | yes |
| `any` | depends on the data: fields and boolean constants | yes | yes |

Building a tree that breaks these rules raises `ExpressionError` immediately,
not later at evaluation time. Comparing two conditions, as in
`(F.a > 1) == (F.b > 2)`, is rejected for this reason.

## Catalogue

Each node has a stable lowercase **tag**. Tags are used in serialized rules, in
`UnsupportedOperation.operation` and by every backend. In Python each class
lists its tags in `TAGS`, `Node` is the closed union of concrete classes, and
`Tag` lists every tag.

| Tag | Class | Fields | Kind | Task |
|---|---|---|---|---|
| `field` | `Field` | `path: tuple[str \| int, ...]` | any | 2.1a |
| `const` | `Const` | `value: Scalar` | any if `bool`, else value; `None` only under `is_null` | 2.1a |
| `eq` `ne` `lt` `le` `gt` `ge` | `Compare` | `op`, `left`, `right` | condition | 2.1a |
| `and` | `And` | `operands: tuple[Expr, ...]` (≥ 2) | condition | 2.1a |
| `or` | `Or` | `operands: tuple[Expr, ...]` (≥ 2) | condition | 2.1a |
| `not` | `Not` | `operand` | condition | 2.1a |
| `in` | `In` | `operand`, `values: tuple[Scalar, ...]` | condition | 2.1b |
| `between` | `Between` | `operand`, `low`, `high`, `closed` | condition | 2.1b |
| `is_null` | `IsNull` | `operand` | condition | 2.1b |
| `matches` | `Matches` | `operand`, `pattern: str` | condition | 2.1b |
| `starts_with` `ends_with` `contains` | `StringTest` | `op`, `operand`, `text: str` | condition | 2.1b |
| `add` `sub` `mul` `div` | `Arith` | `op`, `left`, `right` | value | 2.1b |
| `abs` | `Abs` | `operand` | value | 2.1b |
| `neg` | `Neg` | `operand` | value | 2.1b |

### Construction rules

- **`Field(*path)`**: the path is non-empty. Each segment is a non-empty `str`,
  which looks up a key or attribute, or an integer ≥ 0, which indexes a
  sequence. `bool` is not accepted as an index. `Field("total")` is a
  one-segment path.
- **`Const(value)`** is normalized to an exact built-in type:
  - `bool` (including numpy's), `int`, `float`, `Decimal`, `str`, `date`,
    `datetime` or `None`;
  - integer-likes such as `np.int64` and `IntEnum` become `int`, `str`
    subclasses such as `StrEnum` become `str`, `Decimal` subclasses become
    `Decimal`, and date and datetime subclasses are rebuilt;
  - a `Decimal` is kept **exact**, never converted to float, including its
    infinities;
  - a time zone is kept.

  The following are rejected:
  - **NaN** (float or `Decimal`, quiet or signaling), which RulePy treats as
    missing: use `IsNull`;
  - integers outside the signed 64-bit range;
  - datetimes with sub-microsecond precision;
  - any other type.
- **`And(*operands)` / `Or(*operands)`** are n-ary and **flattened** when built:
  an `And` operand that is itself an `And` is spliced in, and the same for `Or`.
  They need at least two operands after flattening. Operand order is kept,
  because it determines explanation order.
- **Value operands.** These operands must be value or `any` nodes, never
  conditions:
  - both sides of `Compare` and `Arith`;
  - the operand of `In`, `IsNull`, `Matches`, `StringTest`, `Abs` and `Neg`;
  - all three nodes of `Between`.

  `Const(None)` is rejected everywhere except as the operand of `IsNull`,
  including as an operand of `And`, `Or` and `Not`. Under Kleene logic,
  `x == null` or `x + null` would be null for every input; use `IsNull` to test
  for missing values. (The Python v1 accepted `Const(None)` under `And`, `Or`
  and `Not`; that was a gap, not semantics.)
- **Arithmetic** (`Arith`, `Abs`, `Neg`) also rejects constants that are not
  numbers (booleans included), such as `Const("a") + 1`.
- **`In(operand, values)`** accepts any iterable of values, including a `set`.
  The values are normalized like `Const`, must be non-empty and non-null, and
  must all belong to one type group; a `str`, `bytes` or mapping is not a
  collection of values here. They are stored **sorted and without duplicates**
  (equal `int` and `float`, such as `1` and `1.0`, are both kept, in a fixed
  order), so the node is deterministic and `In(x, [2, 1])` is the same node as
  `In(x, [1, 2])`.
- **`Between(operand, low, high, closed="both")`**: `closed` is `"both"`,
  `"left"`, `"right"` or `"none"`. When both bounds are constants, they must belong
  to one orderable group (not boolean).
- **`Matches(operand, pattern)`**: the pattern must be valid in the syntax
  defined under *`matches`* below, and compile within the size limit.
- **`StringTest(op, operand, text)`**: `op` is `starts_with`, `ends_with` or
  `contains`, and `text` is a literal `str`.

### Structural identity

`a.same_as(b)` is true when two trees have the same shape, tags and constants.
`a.structural_key()` returns a hashable value with the same meaning, for use as
a dict key.

- Constants compare by **type and value**: `Const(1)`, `Const(1.0)`,
  `Const(Decimal("1"))` and `Const(True)` are four different nodes.
- `0.0` and `-0.0` are the same node, and so are `Decimal("1.0")` and
  `Decimal("1.00")`, although they may serialize differently.
- Two aware datetimes for the same instant in different zones are the same
  node, but they may serialize differently.

Nodes do **not** define structural `__eq__` or `__hash__`. From task 2.3, `==`
on a node builds a `Compare` (as it does in polars and SQLAlchemy), so
structural equality must always go through `same_as` or `structural_key`.
Nodes keep identity hashing and support weak references.

## Evaluation semantics

The reference evaluator (tasks 2.5–2.6) implements these rules, and every other
backend must match it.

### Reading data

A `Field` path is followed one segment at a time:

- A **string** segment:
  - on a **mapping**, the value under that key, if the key is present (`in` is
    checked first, so `__missing__` is never triggered and dict subclasses are
    read through their own `__getitem__`); underscore keys such as `"_id"` are
    allowed;
  - on an object with `keys()` and `__getitem__`, such as a pandas row, the value
    under that key, if `key in obj.keys()`;
  - on a **namedtuple-like record** (a sequence with `_fields`, such as SQLAlchemy
    rows or `itertuples()`), the attribute;
  - on any other object, the attribute, if it is a **public data attribute**.

  Names starting with `_` are never looked up as attributes. The lookup never
  returns methods, classes or modules, and never reads attributes of scalars
  (`bool`, numbers, strings, bytes, `Decimal`, dates and times). So a rule loaded
  from a file cannot reach into the process (`__class__`, `__globals__`,
  `os.environ`, ...), and `F.d.year` on a date is missing in every backend.
- An **integer** segment indexes a sequence-like value with a length (lists,
  tuples, numpy arrays), if `0 <= i < len`. It never looks up a mapping key.
- Anything else is **missing**.
- Pydantic models are read by **field name**, not alias. Extra and computed
  fields are readable.
- If reading raises an exception (for example, a property that fails), the value
  is null with reason `read_error`; strict mode raises `EvaluationError`, chained
  to the original exception. An `AttributeError` counts as missing.

Values are normalized once, when read:

- missing values become `None` (see *Missing values*);
- `numpy.bool_` becomes `bool`;
- integer-likes become `int`, and an integer outside the signed 64-bit range is
  null with reason `int_overflow`;
- other real numbers (except `float` and `Decimal`) become `float`;
- `numpy.datetime64` becomes a naive `datetime` (microseconds).

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
needed by phase 5*): a fuzzy truth value is an interval `[lo, hi]` of degrees.
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

Results never depend on evaluation order. A backend may skip evaluating an
operand only when that cannot change the result, and no operand can turn a
result into an error that another order would not raise (see *Types*).

### Missing values

- A `Field` whose path cannot be resolved (missing key, missing attribute,
  index out of range) evaluates to **null**, the same as an explicit `None`.
  Strict mode (task 2.6) raises `MissingFieldError` instead, even under
  `is_null`.
- A null met **in the middle** of a path (`F.customer.age` with `customer`
  null) makes the field null, even in strict mode: the path exists up to a
  null. A null input (`data=None`) is null at the empty path ("the input").
- **NaN read from the data is null**: Python, numpy and `Decimal` NaN, as well
  as `pandas.NA` and `NaT` (`rulepy.core._compat.is_null`). This keeps Python, whose
  comparisons with NaN are `False`, and polars, which orders NaN above every
  number, from diverging. NaN produced by arithmetic, such as `inf - inf`, is
  null too.
- Null is checked **before** types. Any comparison, string test or arithmetic
  operation with a null operand yields `UNKNOWN` (or null, for arithmetic),
  never an error. The only exception is `is_null`, which is never `UNKNOWN`.
- A `Field` or `Const` used directly as a condition is converted by the active
  `Logic`:
  - `BooleanLogic` maps `True`/`False` to `TRUE`/`FALSE` and null to `UNKNOWN`;
    any other type is a type mismatch (see below);
  - `FuzzyLogic` also accepts numbers in `[0, 1]` as degrees.

### Types

Values belong to one of these comparison groups:

- **number**: `numbers.Real` except `bool` and `numpy.bool_` (so `np.int64` is a
  number), plus `Decimal`, in data (databases return it for NUMERIC columns) and
  in constants;
- **string**: including polars `Categorical`/`Enum` columns;
- **boolean**;
- **date**;
- **naive datetime**;
- **aware datetime**.

- Ordering (`lt`, `le`, `gt`, `ge`) is defined within number, string (by Unicode
  code point), date and both datetime groups, but not for booleans.
- Comparing values from different groups, or ordering booleans, is a **type
  mismatch**:
  - **Default:** the comparison is `UNKNOWN`, and the trace records the reason
    (for example, "type mismatch: string vs number") so `explain()` shows it. It is
    never a silent `FALSE`, and one bad row never aborts a whole batch.
  - **Strict mode:** it raises `TypeMismatchError` (a subclass of
    `EvaluationError`, task 2.5) with the field path and both types.
- `int`, `float` and `Decimal` compare **exactly**, as Python does. polars casts
  to float, which differs above 2⁵³; its backend must handle that case.

### Nodes of task 2.1b

- **`in`**:
  - `values` is a non-empty tuple of non-null scalars, all from **one** type
    group; this is checked at construction, and to test for null you combine
    it with `is_null`;
  - the result is `TRUE` if the operand equals one of the values, `FALSE`
    otherwise, and `UNKNOWN` if the operand is null or from another group.
- **`between`**:
  - `low` and `high` are nodes, so `F.x.between(F.lo, F.hi)` is allowed; audit
    is exact when they are constants;
  - `closed` is `"both"` (the default), `"left"`, `"right"` or `"none"`, exactly
    as in polars;
  - it is equivalent to the matching pair of comparisons joined by `and`.
- **`matches`** uses **search** semantics: it is `TRUE` when the pattern
  matches anywhere in the string (Rust `Regex::is_match`).
  - The syntax is that of the Rust [`regex`](https://docs.rs/regex) crate, 1.x,
    with its defaults: Unicode on, case-sensitive, `^` and `$` match only at
    the start and end of the text (`$` never before a trailing newline), and
    `.` does not match `\n`. Inline flags such as `(?i)` change this within the
    pattern. There is no lookaround and there are no backreferences, so
    matching takes linear time (`CLAUDE.md`, invariant 9; decision D9).
  - A pattern that does not parse, or whose compiled form exceeds the crate's
    default size limit, is an `ExpressionError`.
  - Python's `re` accepted some patterns this syntax rejects and read `$`
    differently; vectors taken from the v1 skip those cases.
- **`starts_with`, `ends_with`, `contains`** take **literal** text; polars must
  call `str.contains(..., literal=True)`.
- **Arithmetic** (`add`, `sub`, `mul`, `div`, `abs`, `neg`) is defined on
  numbers:
  - `div` converts both operands to float64 **first**, then divides (what
    polars and Rust do), so it always returns a float;
  - `Decimal` operands are converted to float64 for every operation;
  - **division by zero yields null**, for int and float alike;
  - an integer result outside the signed 64-bit range yields null;
  - a NaN result is null, while `inf` and `-inf` results are kept;
  - an operand that is not a number makes the result null with reason
    `type_mismatch` (strict mode raises `TypeMismatchError`).

## Results and reasons

Every node evaluates to a truth value (conditions) or a scalar or null (values).
When a truth value is `UNKNOWN` or a value is null, a **reason** says why, and
only then: a `TRUE` or `FALSE` never carries one. Backends are compared on the
reason's **code**, the **field paths** involved and the type **groups**; the
human-readable text may differ.

| Code | Meaning |
|---|---|
| `missing_field` | the path cannot be resolved |
| `null` | a null value (or NaN) was met; the path says where |
| `type_mismatch` | values of incompatible groups met, or a non-boolean used as a condition |
| `unorderable` | booleans were ordered with `<`, `<=`, `>` or `>=` |
| `division_by_zero` | a division by zero |
| `int_overflow` | an integer outside the signed 64-bit range |
| `nan_result` | an arithmetic result is NaN |
| `read_error` | reading the data raised an exception |

A condition that is `UNKNOWN` takes the reason of its first `UNKNOWN` operand.
A `Between` is reported as its two half comparisons, so explanations can say
which bound failed.

**Strict mode** raises the first error found in depth-first, left-to-right
post-order: children before their parent, operands in order. The Rust backend
follows the same order. polars checks schemas up front, so its parity tests
only assert that some `EvaluationError` is raised.

## Notes for the polars backend (task 4.3)

The compiler must be **schema-aware**, because polars casts implicitly where this
spec does not:

- it upcasts `Date` to `Datetime`;
- it casts `Boolean` to integers;
- it compares `int` with `float` as floats.

To match the spec, the backend:

- applies `fill_nan(None)` to float columns only;
- casts `Float32` columns to `Float64` and never downcasts literals
  (`np.float32(0.1) == 0.1` is `FALSE` in the reference evaluator);
- compares `Decimal` columns exactly with literals, and converts them to
  `Float64` for arithmetic;
- maps division by zero, which polars turns into `inf` or `NaN`, with
  `when(den == 0)`;
- turns type mismatches into `UNKNOWN` per row, with the reason recorded.
