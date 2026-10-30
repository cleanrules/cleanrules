# Condition AST

**Status:** draft, format version `0`. The node catalogue and the semantics below
are normative for every backend. The serialized JSON form (`"rulepy_ast": 1`) is
defined in task 3.1; until then nothing is serialized.

A rule's condition is a tree of immutable nodes (`rulepy.core.expr`). Users
normally build it with the `F` proxy and the operators `&`, `|` and `~`, which
arrive in tasks 2.2–2.3. This document defines what the tree *means*, so that
the Python evaluator, polars, Rete and Rust all agree.

## Node kinds

Every node has a **kind**, which says where it may appear:

| Kind | Meaning | Operand of `and` / `or` / `not` | Operand of comparisons and arithmetic |
|---|---|---|---|
| `condition` | evaluates to a truth value (crisp or fuzzy) | yes | no |
| `value` | evaluates to a scalar | no | yes |
| `any` | depends on the data: fields, boolean constants, `null` | yes | yes |

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
| `const` | `Const` | `value: Scalar` | any if `bool` or `None`, else value | 2.1a |
| `eq` `ne` `lt` `le` `gt` `ge` | `Compare` | `op`, `left`, `right` | condition | 2.1a |
| `and` | `And` | `operands: tuple[Expr, ...]` (≥ 2) | condition | 2.1a |
| `or` | `Or` | `operands: tuple[Expr, ...]` (≥ 2) | condition | 2.1a |
| `not` | `Not` | `operand` | condition | 2.1a |
| `in` | `In` | `operand`, `values: tuple[Scalar, ...]` | condition | 2.1b |
| `between` | `Between` | `operand`, `low`, `high`, `closed` | condition | 2.1b |
| `is_null` | `IsNull` | `operand` | condition | 2.1b |
| `matches` | `Matches` | `operand`, `pattern: str` | condition | 2.1b |
| `starts_with` `ends_with` `contains` | `StringTest` | `op`, `operand`, `text` | condition | 2.1b |
| `add` `sub` `mul` `div` | `Arith` | `op`, `left`, `right` | value | 2.1b |
| `abs` | `Abs` | `operand` | value | 2.1b |
| `neg` | `Neg` | `operand` | value | 2.1b |

### Construction rules

- **`Field(*path)`**: the path is non-empty. Each segment is a non-empty `str`,
  which looks up a key or attribute, or an integer ≥ 0, which indexes a
  sequence. `bool` is not accepted as an index. `Field("total")` is a
  one-segment path.
- **`Const(value)`** is normalized to an exact built-in type:
  - `bool` (including numpy's), `int`, `float`, `str`, `date`, `datetime` or
    `None`;
  - integer-likes such as `np.int64` and `IntEnum` become `int`, `str`
    subclasses such as `StrEnum` become `str`, and date and datetime subclasses
    are rebuilt;
  - a time zone is kept.

  The following are rejected:
  - **NaN**, which RulePy treats as missing: use `IsNull`;
  - integers outside the signed 64-bit range;
  - datetimes with sub-microsecond precision;
  - any other type.
- **`And(*operands)` / `Or(*operands)`** are n-ary and **flattened** when built:
  an `And` operand that is itself an `And` is spliced in, and the same for `Or`.
  They need at least two operands after flattening. Operand order is kept,
  because it determines explanation order.
- **`Compare(op, left, right)`**: both sides must be value or `any` nodes, never
  conditions. `Const(None)` on either side is rejected, because under Kleene
  logic `x == null` is never true; use `IsNull` instead.

### Structural identity

`a.same_as(b)` is true when two trees have the same shape, tags and constants.
`a.structural_key()` returns a hashable value with the same meaning, for use as
a dict key.

- Constants compare by **type and value**: `Const(1)`, `Const(1.0)` and
  `Const(True)` are three different nodes.
- `0.0` and `-0.0` are the same node.
- Two aware datetimes for the same instant in different zones are the same
  node, but they may serialize differently.

Nodes do **not** define structural `__eq__` or `__hash__`. From task 2.3, `==`
on a node builds a `Compare` (as it does in polars and SQLAlchemy), so
structural equality must always go through `same_as` or `structural_key`.
Nodes keep identity hashing and support weak references.

## Evaluation semantics

The reference evaluator (tasks 2.5–2.6) implements these rules, and every other
backend must match it.

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
complement, and truth values are in `[0, 1]`.

Results never depend on evaluation order. A backend may skip evaluating an
operand only when that cannot change the result, and no operand can turn a
result into an error that another order would not raise (see *Types*).

### Missing values

- A `Field` whose path cannot be resolved (missing key, missing attribute,
  index out of range) evaluates to **null**, the same as an explicit `None`.
  Strict mode (task 2.5) raises `EvaluationError` instead.
- A float **NaN read from the data is null**. This keeps Python, whose
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

- **number**: `numbers.Real` except `bool` and `numpy.bool_`, so `np.int64` is a
  number;
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
    (for example, "type mismatch: str vs int") so `explain()` shows it. It is
    never a silent `FALSE`, and one bad row never aborts a whole batch.
  - **Strict mode:** it raises `TypeMismatchError` (a subclass of
    `EvaluationError`, task 2.5) with the field path and both types.
- `int` vs `float` compares **exactly**, as Python does. polars casts both to
  float, which differs above 2⁵³; its backend must handle that case.

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
- **`matches`** uses **search** semantics (Python `re.search`, Rust
  `Regex::is_match`, polars `str.contains`). It is case-sensitive unless the
  pattern starts with the inline flag `(?i)`. Patterns are restricted to the
  subset shared by Python `re` and Rust `regex`.
- **`starts_with`, `ends_with`, `contains`** take **literal** text; polars must
  call `str.contains(..., literal=True)`.
- **Arithmetic** (`add`, `sub`, `mul`, `div`, `abs`, `neg`) is defined on
  numbers:
  - `div` is true division and always returns a float;
  - **division by zero yields null**, for int and float alike;
  - an integer result outside the signed 64-bit range yields null;
  - a NaN result is null.

## Notes for the polars backend (task 4.3)

The compiler must be **schema-aware**, because polars casts implicitly where this
spec does not:

- it upcasts `Date` to `Datetime`;
- it casts `Boolean` to integers;
- it compares `int` with `float` as floats.

To match the spec, the backend:

- applies `fill_nan(None)` to float columns only;
- maps division by zero, which polars turns into `inf` or `NaN`, with
  `when(den == 0)`;
- turns type mismatches into `UNKNOWN` per row, with the reason recorded.
