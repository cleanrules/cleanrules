"""Condition AST: the immutable tree every rule condition is made of.

Conditions are data, never opaque functions, so the same tree can be evaluated,
explained, serialized, vectorized, audited and compiled to Rust. The catalogue of
nodes, their kinds and their semantics are specified in `docs/spec/ast.md`.

Nodes deliberately keep identity-based `==` and `hash`: from task 2.3 on, `==`
builds a `Compare` node (as in polars or SQLAlchemy), so structural equality is
`same_as` and structural hashing is `structural_key()`.

Note: `Field` here is a condition node, unrelated to `pydantic.Field`.
"""

from __future__ import annotations

import datetime as dt
import math
import numbers
import operator
import re
from collections.abc import Iterable, Iterator, Mapping
from dataclasses import dataclass, fields
from typing import Any, ClassVar, Literal, TypeAlias, get_args

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


@dataclass(frozen=True, eq=False, slots=True, init=False)
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


@dataclass(frozen=True, eq=False, slots=True)
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


@dataclass(frozen=True, eq=False, slots=True)
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


@dataclass(frozen=True, eq=False, slots=True, init=False)
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


@dataclass(frozen=True, eq=False, slots=True, init=False)
class And(_Connective):
    """Conjunction of two or more conditions, e.g. `And(a, b, c)`; nested `And`s are flattened.

    Args:
        *operands: The conditions, in explanation order.
    """

    TAGS = frozenset({"and"})
    _tag = "and"


@dataclass(frozen=True, eq=False, slots=True, init=False)
class Or(_Connective):
    """Disjunction of two or more conditions, e.g. `Or(a, b)`; nested `Or`s are flattened.

    Args:
        *operands: The conditions, in explanation order.
    """

    TAGS = frozenset({"or"})
    _tag = "or"


@dataclass(frozen=True, eq=False, slots=True)
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


@dataclass(frozen=True, eq=False, slots=True, init=False)
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


@dataclass(frozen=True, eq=False, slots=True)
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


@dataclass(frozen=True, eq=False, slots=True)
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


@dataclass(frozen=True, eq=False, slots=True)
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


@dataclass(frozen=True, eq=False, slots=True)
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


@dataclass(frozen=True, eq=False, slots=True)
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


@dataclass(frozen=True, eq=False, slots=True)
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


@dataclass(frozen=True, eq=False, slots=True)
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
