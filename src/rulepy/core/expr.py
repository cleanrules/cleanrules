"""Condition AST: the immutable tree every rule condition is made of.

Conditions are data, never opaque functions, so the same tree can be evaluated,
explained, serialized, vectorized, audited and compiled to Rust. The catalogue of
nodes, their kinds and their semantics are specified in `docs/spec/ast.md`.

Trees are normally built with the `F` proxy and Python operators:

    (F.customer.age >= 18) & F.country.is_in({"ES", "PT"}) & ~F.blocked

Like in polars or SQLAlchemy, `==` on a node builds a `Compare` node instead of
returning a bool, and using a node as a Python bool raises `ExpressionError`
(`and`/`or`/`not`, `if node:`, chained comparisons such as `0 < F.x < 10`).
Structural equality is `same_as`, structural hashing is `structural_key()`, and
nodes hash by identity. See the "Common pitfalls" page of the documentation.

Note: `Field` here is a condition node, unrelated to `pydantic.Field`.
"""

from __future__ import annotations

import datetime as dt
import difflib
import keyword
import math
import numbers
import operator
import re
from collections.abc import Iterable, Iterator, Mapping
from dataclasses import dataclass, fields
from typing import Any, ClassVar, Literal, NoReturn, TypeAlias, get_args

from rulepy.errors import ExpressionError

__all__ = [
    "Abs",
    "And",
    "Arith",
    "ArithOp",
    "Between",
    "Closed",
    "Compare",
    "CompareOp",
    "Const",
    "Expr",
    "F",
    "Field",
    "In",
    "IsNull",
    "Kind",
    "Matches",
    "Neg",
    "Node",
    "Not",
    "Or",
    "Scalar",
    "StringOp",
    "StringTest",
    "Tag",
]

Scalar: TypeAlias = bool | int | float | str | dt.date | dt.datetime | None
"""Python types a `Const` holds after normalization."""

Kind: TypeAlias = Literal["condition", "value", "any"]
"""Where a node may appear: as a `condition`, as a `value`, or as either (`any`)."""

CompareOp: TypeAlias = Literal["eq", "ne", "lt", "le", "gt", "ge"]
"""Comparison operators; each one is also the tag of its `Compare` node."""

StringOp: TypeAlias = Literal["starts_with", "ends_with", "contains"]
"""Literal-text tests; each one is also the tag of its `StringTest` node."""

ArithOp: TypeAlias = Literal["add", "sub", "mul", "div"]
"""Binary arithmetic operators; each one is also the tag of its `Arith` node."""

Closed: TypeAlias = Literal["both", "left", "right", "none"]
"""Which ends of a `Between` interval are included (same values as polars)."""

Tag: TypeAlias = Literal[
    "field",
    "const",
    "eq",
    "ne",
    "lt",
    "le",
    "gt",
    "ge",
    "and",
    "or",
    "not",
    "in",
    "between",
    "is_null",
    "matches",
    "starts_with",
    "ends_with",
    "contains",
    "add",
    "sub",
    "mul",
    "div",
    "abs",
    "neg",
]
"""Every operator tag (see `docs/spec/ast.md`)."""

_COMPARE_OPS: frozenset[str] = frozenset(get_args(CompareOp))
_STRING_OPS: frozenset[str] = frozenset(get_args(StringOp))
_ARITH_OPS: frozenset[str] = frozenset(get_args(ArithOp))
_CLOSED: frozenset[str] = frozenset(get_args(Closed))
_I64_MIN, _I64_MAX = -(2**63), 2**63 - 1


@dataclass(frozen=True, eq=False, slots=True, weakref_slot=True)
class Expr:
    """Base class of every condition node; not instantiable itself.

    Subclasses are frozen dataclasses whose fields are child nodes, tuples of child
    nodes, or plain data (operator names, normalized scalars, paths).
    """

    TAGS: ClassVar[frozenset[str]]
    """All tags instances of this class can have."""
    _tag: ClassVar[str]
    _kind: ClassVar[Kind]
    _abstract: ClassVar[bool] = True

    def __new__(cls, *args: Any, **kwargs: Any) -> Expr:
        """Refuse to instantiate abstract bases (`Expr` and the connective base)."""
        # Only abstract bases set _abstract in their own body; subclasses inherit it.
        if cls.__dict__.get("_abstract", False):
            raise ExpressionError(f"{cls.__name__} is abstract; build a concrete node instead")
        return object.__new__(cls)

    @property
    def tag(self) -> str:
        """Stable lowercase operator tag, as listed in `docs/spec/ast.md`."""
        return self._tag

    @property
    def kind(self) -> Kind:
        """Whether the node is a `condition`, a `value`, or `any` (depends on the data)."""
        return self._kind

    def children(self) -> tuple[Expr, ...]:
        """Return the direct child nodes, in field order."""
        out: list[Expr] = []
        for value in self._field_values():
            if isinstance(value, Expr):
                out.append(value)
            elif isinstance(value, tuple):
                items: tuple[object, ...] = value  # pyright: ignore[reportUnknownVariableType]
                out.extend(v for v in items if isinstance(v, Expr))
        return tuple(out)

    def walk(self) -> Iterator[Expr]:
        """Yield this node and all its descendants in pre-order (parents first)."""
        stack: list[Expr] = [self]
        while stack:
            node = stack.pop()
            yield node
            stack.extend(reversed(node.children()))

    def structural_key(self) -> tuple[object, ...]:
        """Return a hashable key such that equal keys mean `same_as` is true."""
        return (self.tag, *(_key_of(v) for v in self._field_values()))

    def same_as(self, other: object) -> bool:
        """Return whether `other` is a node with the same structure, tags and constants."""
        return isinstance(other, Expr) and self.structural_key() == other.structural_key()

    def _field_values(self) -> list[object]:
        return [getattr(self, f.name) for f in fields(self) if f.init]

    # --- Python protocol -------------------------------------------------------------

    def __repr__(self) -> str:
        return _render(self)

    def __bool__(self) -> bool:
        raise ExpressionError(
            f"a condition node cannot be used as a Python bool: {self!r}",
            hint="combine conditions with &, | and ~ (not and/or/not, not `if`); write"
            " ranges as F.x.between(low, high) instead of chained comparisons; use"
            " F.x.is_in([...]) instead of `in`",
        )

    def __iter__(self) -> Iterator[Expr]:
        # Without this, __getitem__ on fields would make Python iterate F.x[0], F.x[1],
        # ... forever (the legacy sequence protocol).
        raise ExpressionError(
            f"condition nodes are not iterable: {self!r}",
            hint="use F.items[0] for one element and F.x.is_in([...]) for membership",
        )

    def __contains__(self, item: object) -> bool:
        raise ExpressionError(
            f"`in` cannot test membership in a condition node: {item!r} in {self!r}",
            hint="use F.x.is_in([...]) to test a value against a set of constants",
        )

    # Defining __eq__ would set __hash__ to None; nodes keep identity hashing.
    __hash__ = object.__hash__

    # Make numpy arrays and pandas objects on the left of an operator defer to the node
    # (which then rejects them clearly) instead of broadcasting element by element.
    __array_ufunc__: ClassVar[None] = None
    __pandas_priority__: ClassVar[int] = 5000

    # --- Comparisons (return nodes, not bools) ---------------------------------------

    def __eq__(self, other: object) -> Compare:  # pyright: ignore[reportIncompatibleMethodOverride]
        return Compare("eq", self, _lift(other))

    def __ne__(self, other: object) -> Compare:  # pyright: ignore[reportIncompatibleMethodOverride]
        return Compare("ne", self, _lift(other))

    def __lt__(self, other: object) -> Compare:
        return Compare("lt", self, _lift(other))

    def __le__(self, other: object) -> Compare:
        return Compare("le", self, _lift(other))

    def __gt__(self, other: object) -> Compare:
        return Compare("gt", self, _lift(other))

    def __ge__(self, other: object) -> Compare:
        return Compare("ge", self, _lift(other))

    # --- Logical connectives ---------------------------------------------------------

    def __and__(self, other: object) -> And:
        return And(self, _lift_condition(other))

    def __rand__(self, other: object) -> And:
        return And(_lift_condition(other), self)

    def __or__(self, other: object) -> Or:
        return Or(self, _lift_condition(other))

    def __ror__(self, other: object) -> Or:
        return Or(_lift_condition(other), self)

    def __invert__(self) -> Not:
        return Not(self)

    # --- Arithmetic ------------------------------------------------------------------

    def __add__(self, other: object) -> Arith:
        return Arith("add", self, _lift(other))

    def __radd__(self, other: object) -> Arith:
        return Arith("add", _lift(other), self)

    def __sub__(self, other: object) -> Arith:
        return Arith("sub", self, _lift(other))

    def __rsub__(self, other: object) -> Arith:
        return Arith("sub", _lift(other), self)

    def __mul__(self, other: object) -> Arith:
        return Arith("mul", self, _lift(other))

    def __rmul__(self, other: object) -> Arith:
        return Arith("mul", _lift(other), self)

    def __truediv__(self, other: object) -> Arith:
        return Arith("div", self, _lift(other))

    def __rtruediv__(self, other: object) -> Arith:
        return Arith("div", _lift(other), self)

    def __neg__(self) -> Neg:
        return Neg(self)

    def __abs__(self) -> Abs:
        return Abs(self)

    # --- Predicates and functions ----------------------------------------------------

    def is_in(self, values: Iterable[Scalar]) -> In:
        """Test membership in a set of constants, e.g. `F.country.is_in({"ES", "PT"})`.

        Args:
            values: A collection (list, tuple, set, ...) of non-null scalars of one
                type group. A `str`, `bytes` or mapping is not accepted as the
                collection.

        Returns:
            An `In` node; its values are stored sorted and deduplicated.

        Raises:
            ExpressionError: If `values` is empty, not a collection, contains `None`
                or mixes type groups.
        """
        return In(self, values)

    def between(self, low: object, high: object, closed: Closed = "both") -> Between:
        """Test a range, e.g. `F.age.between(18, 65)` or `F.x.between(F.lo, F.hi)`.

        Args:
            low: Lower bound, a constant or a node.
            high: Upper bound, a constant or a node.
            closed: Included ends: `"both"` (default), `"left"`, `"right"` or `"none"`.

        Returns:
            A `Between` node.

        Raises:
            ExpressionError: If `closed` is unknown, or both bounds are constants of
                different type groups or booleans.
        """
        return Between(self, _lift(low), _lift(high), closed)

    def is_null(self) -> IsNull:
        """Test whether the value is missing or null; never `UNKNOWN`.

        Returns:
            An `IsNull` node.
        """
        return IsNull(self)

    def matches(self, pattern: str) -> Matches:
        """Search a string with a regular expression, e.g. `F.email.matches(r".+@.+")`.

        Args:
            pattern: The regular expression (search semantics; `(?i)` for
                case-insensitive).

        Returns:
            A `Matches` node.

        Raises:
            ExpressionError: If the pattern does not compile.
        """
        return Matches(self, pattern)

    def starts_with(self, text: str) -> StringTest:
        """Test that a string starts with literal `text` (not a pattern).

        Args:
            text: The prefix.

        Returns:
            A `StringTest` node.
        """
        return StringTest("starts_with", self, text)

    def ends_with(self, text: str) -> StringTest:
        """Test that a string ends with literal `text` (not a pattern).

        Args:
            text: The suffix.

        Returns:
            A `StringTest` node.
        """
        return StringTest("ends_with", self, text)

    def contains(self, text: str) -> StringTest:
        """Test that a string contains literal `text`.

        This works on strings only: it is not list membership (use `is_in` to test a
        value against constants) and not a pattern (use `matches`).

        Args:
            text: The substring.

        Returns:
            A `StringTest` node.
        """
        return StringTest("contains", self, text)

    def abs(self) -> Abs:
        """Return the absolute value of a number; same as `abs(node)`.

        Returns:
            An `Abs` node.
        """
        return Abs(self)


def _lift(value: object) -> Expr:
    """Return `value` if it is a node, else wrap it in `Const`."""
    return value if isinstance(value, Expr) else Const(value)  # pyright: ignore[reportArgumentType]


def _lift_condition(value: object) -> Expr:
    # A Python bool next to & or | almost always comes from a comparison Python
    # evaluated itself, e.g. `F.product.tag == "sale"` (tag is the node's own tag).
    if isinstance(value, bool) or _is_numpy_bool(value):
        raise ExpressionError(
            f"cannot combine a condition with a Python bool ({value!r})",
            hint="if you compared a field named like a node attribute (kind, tag, path,"
            ' ...), use item access such as F.x["kind"]; for a constant write Const(True)',
        )
    return _lift(value)


def _key_of(value: object) -> object:
    if isinstance(value, Expr):
        return value.structural_key()
    if isinstance(value, tuple):
        items: tuple[object, ...] = value  # pyright: ignore[reportUnknownVariableType]
        return tuple(_key_of(v) for v in items)
    if value is None or type(value) in (bool, int, float, str, dt.date, dt.datetime):
        # Tag with the type so Const(1), Const(1.0) and Const(True) differ.
        return (type(value).__name__, value)
    # A node holding other data (e.g. a fuzzytool term) must define its own key.
    raise TypeError(f"no structural key for {type(value).__name__}; override structural_key()")


def _require_node(value: object, where: str) -> Expr:
    if not isinstance(value, Expr):
        raise ExpressionError(
            f"{where} must be an expression node, got {type(value).__name__} {value!r}",
            hint="wrap Python values in Const(...), or build conditions with F",
        )
    return value


def _require_condition(node: Expr, where: str) -> None:
    if node.kind == "value":
        raise ExpressionError(
            f"{where} must be a condition, got a value node ({node.tag!r})",
            hint="compare the value first, e.g. F.x > 0",
        )


def _require_value(value: object, where: str, *, allow_null: bool = False) -> Expr:
    node = _require_node(value, where)
    if node.kind == "condition":
        raise ExpressionError(
            f"{where} is a condition ({node.tag!r}); this operand takes a value",
            hint="combine conditions with & and |, or negate with ~",
        )
    if not allow_null and isinstance(node, Const) and node.value is None:
        raise ExpressionError(
            f"{where} is None: the result would be missing for every input",
            hint="use IsNull(...) / F.x.is_null() to test for missing values",
        )
    return node


def _require_number(value: object, where: str) -> Expr:
    node = _require_value(value, where)
    if isinstance(node, Const) and _group(node.value) != "number":
        raise ExpressionError(
            f"{where} must be a number, got {type(node.value).__name__} {node.value!r}",
            hint="arithmetic is defined on int and float only",
        )
    return node


def _require_op(op: object, allowed: frozenset[str], what: str) -> None:
    if op not in allowed:
        raise ExpressionError(
            f"unknown {what} {op!r}", hint=f"use one of {', '.join(sorted(allowed))}"
        )


def _group(value: Scalar) -> str:
    """Return the comparison group of a normalized scalar (see `docs/spec/ast.md`)."""
    if isinstance(value, bool):
        return "boolean"
    if isinstance(value, int | float):
        return "number"
    if isinstance(value, dt.datetime):
        return "naive datetime" if value.tzinfo is None else "aware datetime"
    if isinstance(value, dt.date):
        return "date"
    return "string" if isinstance(value, str) else "null"


def _normalize_scalar(value: object) -> Scalar:
    """Return `value` as an exact built-in scalar type, or raise `ExpressionError`."""
    if value is None or type(value) in (bool, str, dt.date):
        return value  # pyright: ignore[reportReturnType]
    if isinstance(value, bool) or _is_numpy_bool(value):
        return bool(value)  # pyright: ignore[reportUnknownArgumentType]
    if isinstance(value, numbers.Integral):
        number = operator.index(value)
        if not _I64_MIN <= number <= _I64_MAX:
            raise ExpressionError(
                f"integer constant {number} does not fit in 64 bits",
                hint="polars, Rust and JSON cannot represent it; use a float if precision allows",
            )
        return number
    if isinstance(value, numbers.Real):
        number = float(value)
        if math.isnan(number):
            raise ExpressionError(
                "NaN is not a valid constant: RulePy treats NaN as a missing value",
                hint="use IsNull(...) / F.x.is_null() to test for missing values",
            )
        return number
    if isinstance(value, str):
        return str.__str__(value)  # exact str, even for StrEnum and other subclasses
    if isinstance(value, dt.datetime):
        if getattr(value, "nanosecond", 0):
            raise ExpressionError(f"datetime constant {value!r} has sub-microsecond precision")
        return dt.datetime(
            value.year,
            value.month,
            value.day,
            value.hour,
            value.minute,
            value.second,
            value.microsecond,
            value.tzinfo,
            fold=value.fold,
        )
    if isinstance(value, dt.date):
        return dt.date(value.year, value.month, value.day)
    raise ExpressionError(
        f"{type(value).__name__} {value!r} is not a valid constant",
        hint="constants are None, bool, int, float, str, date or datetime (use .item() on"
        " numpy scalars); use In(...) for a set of values",
    )


def _is_index(value: object) -> bool:
    # bool is an Integral too, but True/False as a path index is almost surely a bug.
    return not isinstance(value, bool) and isinstance(value, numbers.Integral)


def _is_value_collection(values: object) -> bool:
    # A str is iterable, but splitting "abc" into characters is never what was meant.
    return isinstance(values, Iterable) and not isinstance(values, str | bytes | Mapping)


def _is_numpy_bool(value: object) -> bool:
    # Checked by name so numpy is never imported by the core (invariant 4).
    cls = type(value)
    return cls.__module__ == "numpy" and cls.__name__ in ("bool", "bool_")


@dataclass(frozen=True, eq=False, slots=True, init=False, repr=False)
class Field(Expr):
    """A value looked up in the input data by path, e.g. `Field("orders", 0, "total")`.

    Args:
        *path: Keys or attribute names (`str`) and sequence indices (`int` >= 0),
            outermost first.
    """

    path: tuple[str | int, ...]

    TAGS = frozenset({"field"})
    _tag = "field"
    _kind = "any"

    def __init__(self, *path: str | int) -> None:
        if not path:
            raise ExpressionError(
                "a field path needs at least one segment", hint='e.g. Field("customer", "age")'
            )
        normalized: list[str | int] = []
        for segment in path:
            if isinstance(segment, str) and segment:
                normalized.append(str.__str__(segment))
            elif _is_index(segment):
                index = operator.index(segment)  # pyright: ignore[reportArgumentType]
                if index < 0:
                    raise ExpressionError(f"negative index {index} in field path {path!r}")
                normalized.append(index)
            else:
                raise ExpressionError(
                    f"invalid field path segment {segment!r} in {path!r}",
                    hint="use non-empty names (str) and indices >= 0 (int)",
                )
        object.__setattr__(self, "path", tuple(normalized))

    def __getattr__(self, name: str) -> Field:
        # Only called when normal lookup fails. Underscore names are never fields, so
        # protocols probed by copy, pickle, numpy, ... (`__deepcopy__`, ...) still work;
        # reserved names reach here only while unset (a failed __init__), never as fields.
        if name.startswith("_") or name in _RESERVED:
            raise AttributeError(name)
        if name in _NAMESPACES:
            raise AttributeError(
                f"{name!r} looks like a polars namespace; RulePy methods live on the field"
                f" itself (e.g. F.x.starts_with(...)). For a field named {name!r} use"
                f" F.x[{name!r}]"
            )
        return Field(*self.path, name)

    def __call__(self, *args: object, **kwargs: object) -> NoReturn:
        """Fail clearly when a field path is called like a method (`F.x.isin(...)`)."""
        method = str(self.path[-1])
        close = difflib.get_close_matches(method, _METHODS, n=1, cutoff=0.6)
        hint = (
            f"did you mean .{close[0]}(...)?"
            if close
            else "fields are values; build conditions with operators and methods such as"
            " .is_in(...), .between(...), .starts_with(...)"
        )
        raise ExpressionError(f"{self!r} is a field path, not a method", hint=hint)

    def __getitem__(self, key: str | int | tuple[str | int, ...]) -> Field:
        return Field(*self.path, *(key if isinstance(key, tuple) else (key,)))


@dataclass(frozen=True, eq=False, slots=True, repr=False)
class Const(Expr):
    """A constant scalar, normalized to an exact built-in type.

    numpy and pandas scalars, `Enum` members and other subclasses are converted
    (`np.int64` to `int`, `StrEnum` to `str`, ...). NaN and integers outside 64 bits
    are rejected.

    Args:
        value: `None`, `bool`, `int`, `float`, `str`, `datetime.date` or
            `datetime.datetime`.
    """

    value: Scalar

    TAGS = frozenset({"const"})
    _tag = "const"
    _kind = "any"

    def __post_init__(self) -> None:
        object.__setattr__(self, "value", _normalize_scalar(self.value))

    @property
    def kind(self) -> Kind:
        """`any` for `None` and booleans (a condition or a value), `value` otherwise."""
        return "any" if self.value is None or isinstance(self.value, bool) else "value"


@dataclass(frozen=True, eq=False, slots=True, repr=False)
class Compare(Expr):
    """A binary comparison; its tag is the operator (`eq`, `ne`, `lt`, `le`, `gt`, `ge`).

    Args:
        op: The comparison operator.
        left: Left-hand value node.
        right: Right-hand value node.
    """

    op: CompareOp
    left: Expr
    right: Expr

    TAGS = _COMPARE_OPS
    _kind = "condition"

    def __post_init__(self) -> None:
        _require_op(self.op, _COMPARE_OPS, "comparison operator")
        _require_value(self.left, f"the left side of {self.op!r}")
        _require_value(self.right, f"the right side of {self.op!r}")

    @property
    def tag(self) -> str:
        """The comparison operator."""
        return self.op


@dataclass(frozen=True, eq=False, slots=True, init=False, repr=False)
class _Connective(Expr):
    operands: tuple[Expr, ...]

    _kind = "condition"
    _abstract = True

    def __init__(self, *operands: Expr) -> None:
        flat: list[Expr] = []
        for i, operand in enumerate(operands):
            where = f"operand {i} of {self._tag!r}"
            node = _require_node(operand, where)
            _require_condition(node, where)
            # Splice same-type children so the tree stays flat whatever the build order.
            if type(node) is type(self):
                flat.extend(node.operands)  # pyright: ignore[reportAttributeAccessIssue, reportUnknownMemberType, reportUnknownArgumentType]
            else:
                flat.append(node)
        if len(flat) < 2:
            hint = "use the single condition directly" if flat else "pass at least two conditions"
            raise ExpressionError(
                f"{self._tag!r} needs at least two operands, got {len(flat)}", hint=hint
            )
        object.__setattr__(self, "operands", tuple(flat))


@dataclass(frozen=True, eq=False, slots=True, init=False, repr=False)
class And(_Connective):
    """Conjunction of two or more conditions, e.g. `And(a, b, c)`; nested `And`s are flattened.

    Args:
        *operands: The conditions, in explanation order.
    """

    TAGS = frozenset({"and"})
    _tag = "and"


@dataclass(frozen=True, eq=False, slots=True, init=False, repr=False)
class Or(_Connective):
    """Disjunction of two or more conditions, e.g. `Or(a, b)`; nested `Or`s are flattened.

    Args:
        *operands: The conditions, in explanation order.
    """

    TAGS = frozenset({"or"})
    _tag = "or"


@dataclass(frozen=True, eq=False, slots=True, repr=False)
class Not(Expr):
    """Negation of a condition.

    Args:
        operand: The condition to negate.
    """

    operand: Expr

    TAGS = frozenset({"not"})
    _tag = "not"
    _kind = "condition"

    def __post_init__(self) -> None:
        where = "the operand of 'not'"
        _require_condition(_require_node(self.operand, where), where)


@dataclass(frozen=True, eq=False, slots=True, init=False, repr=False)
class In(Expr):
    """Membership in a finite set of constants, e.g. `In(F.country, {"ES", "PT"})`.

    Values are normalized like `Const`, must be non-null and of one type group, and
    are stored sorted and deduplicated, so the node is deterministic.

    Args:
        operand: The value to look up.
        values: The allowed values (any iterable, including a `set`).
    """

    operand: Expr
    values: tuple[Scalar, ...]

    TAGS = frozenset({"in"})
    _tag = "in"
    _kind = "condition"

    def __init__(self, operand: Expr, values: Iterable[Scalar]) -> None:
        _require_value(operand, "the operand of 'in'")
        if not _is_value_collection(values):
            raise ExpressionError(
                f"'in' values must be a collection of scalars,"
                f" got {type(values).__name__} {values!r}",
                hint=f"wrap a single value: In(x, [{values!r}])",
            )
        normalized: list[Scalar] = []
        for value in values:
            if value is None:
                raise ExpressionError(
                    "'in' values cannot contain None",
                    hint="combine with IsNull(...): In(x, [...]) | IsNull(x)",
                )
            try:
                normalized.append(_normalize_scalar(value))
            except ExpressionError as err:
                raise ExpressionError(f"'in' values: {err.message}", hint=err.hint) from None
        if not normalized:
            raise ExpressionError(
                "'in' needs at least one value",
                hint="a condition that is never true is better written as Const(False)",
            )
        groups = {_group(v) for v in normalized}
        if len(groups) > 1:
            raise ExpressionError(
                f"'in' values mix type groups: {', '.join(sorted(groups))}",
                hint="use values of a single type; comparisons across types are never true",
            )
        # Sort by value, then type name, and deduplicate on the structural key, so that
        # equal int and float values (1 and 1.0) are both kept in a fixed order.
        ordered = sorted(normalized, key=lambda v: (v, type(v).__name__))  # pyright: ignore[reportArgumentType, reportUnknownLambdaType]
        unique: list[Scalar] = []
        for value in ordered:
            if not unique or _key_of(value) != _key_of(unique[-1]):
                unique.append(value)
        object.__setattr__(self, "operand", operand)
        object.__setattr__(self, "values", tuple(unique))


@dataclass(frozen=True, eq=False, slots=True, repr=False)
class Between(Expr):
    """Range test `low <= operand <= high`, with `closed` choosing the inclusive ends.

    Args:
        operand: The value to test.
        low: Lower bound (a node, so it may be another field).
        high: Upper bound.
        closed: `"both"` (default), `"left"`, `"right"` or `"none"`.
    """

    operand: Expr
    low: Expr
    high: Expr
    closed: Closed = "both"

    TAGS = frozenset({"between"})
    _tag = "between"
    _kind = "condition"

    def __post_init__(self) -> None:
        for name in ("operand", "low", "high"):
            _require_value(getattr(self, name), f"the {name} of 'between'")
        _require_op(self.closed, _CLOSED, "'closed' value for 'between'")
        if isinstance(self.low, Const) and isinstance(self.high, Const):
            groups = {_group(self.low.value), _group(self.high.value)}
            if len(groups) > 1 or groups == {"boolean"}:
                raise ExpressionError(
                    f"'between' bounds {self.low.value!r} and {self.high.value!r} cannot"
                    " bound one ordered value",
                    hint="use two bounds of the same orderable type (number, string, date)",
                )


@dataclass(frozen=True, eq=False, slots=True, repr=False)
class IsNull(Expr):
    """True when the operand is missing or null (never `UNKNOWN`).

    Args:
        operand: The value to test.
    """

    operand: Expr

    TAGS = frozenset({"is_null"})
    _tag = "is_null"
    _kind = "condition"

    def __post_init__(self) -> None:
        _require_value(self.operand, "the operand of 'is_null'", allow_null=True)


@dataclass(frozen=True, eq=False, slots=True, repr=False)
class Matches(Expr):
    """Regular-expression search in a string (case-sensitive unless `(?i)`).

    Args:
        operand: The string value.
        pattern: The regular expression; it must compile.
    """

    operand: Expr
    pattern: str

    TAGS = frozenset({"matches"})
    _tag = "matches"
    _kind = "condition"

    def __post_init__(self) -> None:
        _require_value(self.operand, "the operand of 'matches'")
        if not isinstance(self.pattern, str):  # pyright: ignore[reportUnnecessaryIsInstance]
            raise ExpressionError(f"the 'matches' pattern must be a str, got {self.pattern!r}")
        object.__setattr__(self, "pattern", str.__str__(self.pattern))
        try:
            re.compile(self.pattern)
        except re.error as err:
            raise ExpressionError(
                f"invalid regular expression {self.pattern!r}: {err.msg} at position {err.pos}",
                hint="escape special characters with re.escape(...) to match them literally",
            ) from None


@dataclass(frozen=True, eq=False, slots=True, repr=False)
class StringTest(Expr):
    """Literal-text test; its tag is the operator (`starts_with`, `ends_with`, `contains`).

    Args:
        op: The test.
        operand: The string value.
        text: The literal text to look for (not a pattern).
    """

    op: StringOp
    operand: Expr
    text: str

    TAGS = _STRING_OPS
    _kind = "condition"

    def __post_init__(self) -> None:
        _require_op(self.op, _STRING_OPS, "string test")
        _require_value(self.operand, f"the operand of {self.op!r}")
        if not isinstance(self.text, str):  # pyright: ignore[reportUnnecessaryIsInstance]
            raise ExpressionError(
                f"the text of {self.op!r} must be a literal str, got {self.text!r}",
                hint="use Matches(...) for patterns",
            )
        object.__setattr__(self, "text", str.__str__(self.text))

    @property
    def tag(self) -> str:
        """The string test operator."""
        return self.op


@dataclass(frozen=True, eq=False, slots=True, repr=False)
class Arith(Expr):
    """Binary arithmetic on numbers; its tag is the operator (`add`, `sub`, `mul`, `div`).

    Args:
        op: The operator; `div` is true division.
        left: Left-hand number.
        right: Right-hand number.
    """

    op: ArithOp
    left: Expr
    right: Expr

    TAGS = _ARITH_OPS
    _kind = "value"

    def __post_init__(self) -> None:
        _require_op(self.op, _ARITH_OPS, "arithmetic operator")
        _require_number(self.left, f"the left side of {self.op!r}")
        _require_number(self.right, f"the right side of {self.op!r}")

    @property
    def tag(self) -> str:
        """The arithmetic operator."""
        return self.op


@dataclass(frozen=True, eq=False, slots=True, repr=False)
class Abs(Expr):
    """Absolute value of a number.

    Args:
        operand: The number.
    """

    operand: Expr

    TAGS = frozenset({"abs"})
    _tag = "abs"
    _kind = "value"

    def __post_init__(self) -> None:
        _require_number(self.operand, "the operand of 'abs'")


@dataclass(frozen=True, eq=False, slots=True, repr=False)
class Neg(Expr):
    """Negation of a number.

    Args:
        operand: The number.
    """

    operand: Expr

    TAGS = frozenset({"neg"})
    _tag = "neg"
    _kind = "value"

    def __post_init__(self) -> None:
        _require_number(self.operand, "the operand of 'neg'")


Node: TypeAlias = (
    Field
    | Const
    | Compare
    | And
    | Or
    | Not
    | In
    | Between
    | IsNull
    | Matches
    | StringTest
    | Arith
    | Abs
    | Neg
)
"""Closed union of concrete nodes: backends `match` on it with `assert_never`."""


class _FieldRoot:
    """The `F` proxy: `F.name` and `F["name"]` start a `Field` path."""

    __slots__ = ()

    def __getattr__(self, name: str) -> Field:
        if name.startswith("_"):
            raise AttributeError(name)
        return Field(name)

    def __getitem__(self, key: str | int | tuple[str | int, ...]) -> Field:
        return Field(*(key if isinstance(key, tuple) else (key,)))

    def __iter__(self) -> Iterator[Field]:
        raise ExpressionError(
            "F is not iterable", hint='start a field path with F.name or F["name"]'
        )

    def __contains__(self, item: object) -> bool:
        raise ExpressionError(
            "`in` cannot test membership in F", hint="use F.x.is_in([...]) for membership"
        )

    def __repr__(self) -> str:
        return "F"


F = _FieldRoot()
"""Entry point for building conditions: `F.customer.age >= 18`, `F["a key"]`, `F.items[0]`."""


# --- Rendering: repr() is Python that rebuilds the same tree with F and Const ----------

_COMPARE_SYMBOLS = {"eq": "==", "ne": "!=", "lt": "<", "le": "<=", "gt": ">", "ge": ">="}
_ARITH_SYMBOLS = {"add": "+", "sub": "-", "mul": "*", "div": "/"}
# Names that already mean something on a Field (methods, properties, `path`, ...).
_RESERVED = frozenset(name for name in dir(Field) if not name.startswith("_"))
# Builder methods, for "did you mean" hints when a field path is called.
_METHODS = frozenset(
    name for name in _RESERVED if callable(getattr(Field, name)) and name.islower()
)
# polars-style namespaces (F.x.str.starts_with) would silently become field paths.
_NAMESPACES = frozenset({"str", "dt", "list", "arr", "struct", "cat"})


def _render(node: Expr) -> str:
    match node:
        case Field():
            return _render_path(node.path)
        case Const():
            return f"Const({_literal(node.value)})"
        case Compare():
            left, right = _operand(node.left), _operand(node.right, literal=True)
            return f"{left} {_COMPARE_SYMBOLS[node.op]} {right}"
        case And() | Or():
            symbol = " & " if isinstance(node, And) else " | "
            parts = (_operand(op, parens=(Compare, And, Or)) for op in node.operands)
            return symbol.join(parts)
        case Not():
            return f"~{_operand(node.operand, parens=(Compare, And, Or))}"
        case In():
            values = ", ".join(_literal(v) for v in node.values)
            return f"{_receiver(node.operand)}.is_in([{values}])"
        case Between():
            closed = "" if node.closed == "both" else f", closed={node.closed!r}"
            low, high = _operand(node.low, literal=True), _operand(node.high, literal=True)
            return f"{_receiver(node.operand)}.between({low}, {high}{closed})"
        case IsNull():
            return f"{_receiver(node.operand)}.is_null()"
        case Matches():
            return f"{_receiver(node.operand)}.matches({node.pattern!r})"
        case StringTest():
            return f"{_receiver(node.operand)}.{node.op}({node.text!r})"
        case Arith():
            left = _operand(node.left, parens=(Arith,))
            right = _operand(node.right, literal=True, parens=(Arith,))
            return f"{left} {_ARITH_SYMBOLS[node.op]} {right}"
        case Abs():
            return f"abs({_operand(node.operand)})"
        case Neg():
            return f"-{_operand(node.operand, parens=(Arith,))}"
        case _:  # pragma: no cover - a node subclass outside the closed union
            return f"<{type(node).__name__} {node.tag}>"


def _operand(node: Expr, *, literal: bool = False, parens: tuple[type, ...] = ()) -> str:
    # A bare literal is only safe where the other operand is already a node, so Python
    # dispatches to the node's operator (`F.x > 5`); elsewhere it would be computed by
    # Python itself (`1 + 2`, `True & False`), so it is spelled `Const(...)`.
    if isinstance(node, Const) and literal:
        return _literal(node.value)
    text = _render(node)
    return f"({text})" if isinstance(node, parens) else text


def _receiver(node: Expr) -> str:
    return _operand(node, parens=(Arith, Neg))


def _render_path(path: tuple[str | int, ...]) -> str:
    out = ["F"]
    for i, segment in enumerate(path):
        if isinstance(segment, str) and _is_attribute_name(segment, root=i == 0):
            out.append(f".{segment}")
        else:
            out.append(f"[{segment!r}]")
    return "".join(out)


def _is_attribute_name(name: str, *, root: bool) -> bool:
    # ASCII only: Python NFKC-normalizes non-ASCII identifiers (F.ª would read "a").
    plain = name.isascii() and name.isidentifier() and not keyword.iskeyword(name)
    reserved = name in _RESERVED or name in _NAMESPACES
    return plain and not name.startswith("_") and (root or not reserved)


def _literal(value: Scalar) -> str:
    if isinstance(value, float) and math.isinf(value):
        return "float('inf')" if value > 0 else "float('-inf')"
    return repr(value)
