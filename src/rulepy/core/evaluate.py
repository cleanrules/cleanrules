"""Reference evaluator: the semantic oracle every other backend must match (invariant 5).

It implements `docs/spec/ast.md` directly and favours clarity over speed. Every node
is evaluated (no short circuit), so results never depend on operand order, and the
full result tree is kept for explanations:

    evaluate((F.age >= 18) & F.country.is_in({"ES"}), {"age": 30, "country": "ES"})
    # Truth.TRUE

How fields are read (mappings, sequences, records, objects) and how values are
normalized is specified in `docs/spec/ast.md`, section "Reading data". Attribute
lookup is restricted to public data attributes, so a rule loaded from a file cannot
reach into the process (`__class__`, `__globals__`, modules, ...).
"""

from __future__ import annotations

import datetime as dt
import inspect
import math
import numbers
import operator
import re
from collections.abc import Callable, Mapping, Sequence
from dataclasses import dataclass
from decimal import Decimal
from functools import lru_cache
from types import ModuleType
from typing import Any, Final, Literal, TypeAlias, TypeVar, assert_never, overload

from rulepy.core._compat import is_null, is_numpy_bool
from rulepy.core.expr import (
    Abs,
    And,
    Arith,
    Between,
    Compare,
    Const,
    Expr,
    Field,
    In,
    IsNull,
    Matches,
    Neg,
    Not,
    Or,
    StringTest,
)
from rulepy.core.logic import Logic, Truth, get_logic
from rulepy.errors import (
    EvaluationError,
    ExpressionError,
    MissingFieldError,
    TypeMismatchError,
    format_path,
)

__all__ = [
    "NodeResult",
    "Reason",
    "ReasonCode",
    "evaluate",
    "evaluate_tree",
    "evaluate_value",
    "evaluate_value_tree",
    "type_group",
]

TruthT = TypeVar("TruthT")
Path: TypeAlias = tuple[str | int, ...]

ReasonCode: TypeAlias = Literal[
    "missing_field",
    "null",
    "type_mismatch",
    "unorderable",
    "division_by_zero",
    "int_overflow",
    "nan_result",
    "read_error",
]
"""Why a value is `UNKNOWN` or null; stable across backends (see `docs/spec/ast.md`)."""


@dataclass(frozen=True, slots=True)
class Reason:
    """Structured cause of an `UNKNOWN` truth value or a null value.

    Backends are compared on `code`, `paths` and `types`; `str(reason)` is the
    human-readable form used in explanations.

    Attributes:
        code: What happened.
        paths: Field paths involved (for `null`, where the null was met; an empty
            path means the input itself).
        types: Comparison groups involved, e.g. `("string", "number")`.
        detail: Extra human-readable text (Python type names, exception text).
    """

    code: ReasonCode
    paths: tuple[Path, ...] = ()
    types: tuple[str, ...] = ()
    detail: str | None = None

    def __str__(self) -> str:
        where = ", ".join(format_path(p) if p else "the input" for p in self.paths)
        match self.code:
            case "missing_field":
                return f"missing field {where}"
            case "null":
                return f"{where} is null" if self.paths else "null value"
            case "type_mismatch":
                shown = self.detail or " vs ".join(self.types)
                return f"type mismatch: {shown}"
            case "unorderable":
                return "cannot order booleans"
            case "division_by_zero":
                return "division by zero"
            case "int_overflow":
                return "integer overflow" + (f" in {where}" if self.paths else "")
            case "nan_result":
                return "NaN result"
            case "read_error":
                return f"cannot read {where}: {self.detail}"
            case _:  # pragma: no cover - ReasonCode is a closed Literal
                assert_never(self.code)


@dataclass(frozen=True, slots=True, eq=False)
class NodeResult:
    """The outcome of evaluating one node, with the outcomes of its children.

    `eq=False`: results hold nodes, whose `==` builds conditions.

    Attributes:
        node: The node that was evaluated.
        value: A truth value for conditions, a scalar (or `None`) for values.
        reason: Why the value is `UNKNOWN` or null; `None` for any other value.
        children: Results of the child nodes, in field order, with two exceptions:
            a `Between` has its two half comparisons (`x >= low`, `x <= high`), and
            a field or constant used directly as a condition has its value result.
    """

    node: Expr
    value: Any
    reason: Reason | None = None
    children: tuple[NodeResult, ...] = ()


@overload
def evaluate(
    condition: Expr,
    data: object,
    *,
    logic: Literal["boolean"] = "boolean",
    strict: bool = False,
    rule: str | None = None,
) -> Truth: ...
@overload
def evaluate(
    condition: Expr,
    data: object,
    *,
    logic: Logic[TruthT],
    strict: bool = False,
    rule: str | None = None,
) -> TruthT: ...
@overload
def evaluate(
    condition: Expr,
    data: object,
    *,
    logic: str,
    strict: bool = False,
    rule: str | None = None,
) -> Any: ...
def evaluate(
    condition: Expr,
    data: object,
    *,
    logic: str | Logic[Any] = "boolean",
    strict: bool = False,
    rule: str | None = None,
) -> Any:
    """Evaluate a condition against `data` and return its truth value.

    Args:
        condition: A condition node (or a field/constant used as a condition).
        data: The input: a mapping, a sequence, a record or an object.
        logic: The logic or its registered name (default Kleene `"boolean"`).
        strict: Raise on missing fields, type mismatches and read errors instead of
            returning `UNKNOWN`. Errors are raised in depth-first, left-to-right
            post-order.
        rule: Rule name added to error messages.

    Returns:
        A truth value of `logic`, e.g. `Truth.TRUE`.

    Raises:
        ExpressionError: If `condition` is not a condition node.
        MissingFieldError: In strict mode, if a field path cannot be resolved.
        TypeMismatchError: In strict mode, if values of incompatible types meet.
        EvaluationError: In strict mode, if reading a field raised an exception.
    """
    return evaluate_tree(condition, data, logic=logic, strict=strict, rule=rule).value


def evaluate_tree(
    condition: Expr,
    data: object,
    *,
    logic: str | Logic[Any] = "boolean",
    strict: bool = False,
    rule: str | None = None,
) -> NodeResult:
    """Evaluate a condition and return the full result tree.

    Args:
        condition: A condition node (or a field/constant used as a condition).
        data: The input.
        logic: The logic or its registered name.
        strict: Raise instead of returning `UNKNOWN` (see `evaluate`).
        rule: Rule name added to error messages.

    Returns:
        The root `NodeResult`; each node's value and, when `UNKNOWN` or null, why.

    Raises:
        ExpressionError: If `condition` is not a condition node.
        EvaluationError: In strict mode (see `evaluate`).
    """
    _require_expr(condition)
    if condition.kind == "value":
        raise ExpressionError(
            f"{condition!r} is a value, not a condition",
            hint="compare it (e.g. ... > 0), or use evaluate_value for values",
        )
    return _Evaluator(get_logic(logic), strict, rule, data).condition(condition)


def evaluate_value(
    expr: Expr, data: object, *, strict: bool = False, rule: str | None = None
) -> Any:
    """Evaluate a value node (field, constant, arithmetic).

    Args:
        expr: A value node, e.g. `F.price * F.qty`.
        data: The input.
        strict: Raise instead of returning `None` for missing or mismatched data.
        rule: Rule name added to error messages.

    Returns:
        The scalar, or `None` when it is null.

    Raises:
        ExpressionError: If `expr` is not a node or is a condition.
        EvaluationError: In strict mode (see `evaluate`).
    """
    return evaluate_value_tree(expr, data, strict=strict, rule=rule).value


def evaluate_value_tree(
    expr: Expr, data: object, *, strict: bool = False, rule: str | None = None
) -> NodeResult:
    """Like `evaluate_value`, but return the result tree (why a value is null).

    Args:
        expr: A value node.
        data: The input.
        strict: Raise instead of returning `None` for missing or mismatched data.
        rule: Rule name added to error messages.

    Returns:
        The root `NodeResult`; `value` is the scalar or `None`, `reason` says why.

    Raises:
        ExpressionError: If `expr` is not a node or is a condition.
        EvaluationError: In strict mode (see `evaluate`).
    """
    _require_expr(expr)
    if expr.kind == "condition":
        raise ExpressionError(f"{expr!r} is a condition; use evaluate for conditions")
    return _Evaluator(get_logic("boolean"), strict, rule, data).value(expr)


def type_group(value: object) -> str:
    """Return the comparison group of a normalized value (see `docs/spec/ast.md`).

    One of `"number"`, `"string"`, `"boolean"`, `"date"`, `"naive datetime"`,
    `"aware datetime"` or `"other"`.
    """
    return _group(value)


def _require_expr(node: object) -> None:
    if not isinstance(node, Expr):
        raise ExpressionError(
            f"cannot evaluate {node!r}: not a node", hint="build conditions with F, e.g. F.x > 0"
        )


# --- Implementation --------------------------------------------------------------------

_MISSING: Final = object()
_I64_MIN, _I64_MAX = -(2**63), 2**63 - 1
_COMPARE: Final[dict[str, Callable[[Any, Any], bool]]] = {
    "eq": operator.eq,
    "ne": operator.ne,
    "lt": operator.lt,
    "le": operator.le,
    "gt": operator.gt,
    "ge": operator.ge,
}
_ORDERING = frozenset({"lt", "le", "gt", "ge"})
# Leaves whose attributes are never data (F.d.year on a date is Python-only).
_OPAQUE = (
    bool, int, float, complex, str, bytes, bytearray, Decimal,
    dt.date, dt.time, dt.timedelta, type, ModuleType,
)  # fmt: skip


class _Evaluator:
    __slots__ = ("data", "logic", "rule", "strict")

    def __init__(self, logic: Logic[Any], strict: bool, rule: str | None, data: object) -> None:
        self.logic = logic
        self.strict = strict
        self.rule = rule
        self.data = data

    # --- conditions ------------------------------------------------------------------

    def condition(self, node: Expr) -> NodeResult:
        match node:
            case And() | Or():
                results = tuple(self.condition(op) for op in node.operands)
                values = [r.value for r in results]
                combine = self.logic.all_ if isinstance(node, And) else self.logic.any_
                return self._combined(node, combine(values), results)
            case Not():
                inner = self.condition(node.operand)
                return self._combined(node, self.logic.not_(inner.value), (inner,))
            case Field() | Const():
                return self._value_as_condition(node)
            case Compare():
                left, right = self.value(node.left), self.value(node.right)
                return self._compare(node, node.op, left, right)
            case In():
                operand = self.value(node.operand)
                truth, why = self._in(node, operand)
                return NodeResult(node, truth, why, (operand,))
            case Between():
                return self._between(node)
            case IsNull():
                operand = self.value(node.operand)
                return NodeResult(
                    node, self.logic.from_bool(operand.value is None), None, (operand,)
                )
            case Matches() | StringTest():
                operand = self.value(node.operand)
                truth, why = self._string_test(node, operand)
                return NodeResult(node, truth, why, (operand,))
            case _:  # pragma: no cover - value nodes are rejected before this point
                raise ExpressionError(f"{node!r} is not a condition")

    def _combined(self, node: Expr, value: Any, results: tuple[NodeResult, ...]) -> NodeResult:
        # A reason explains UNKNOWN only: take it from the first UNKNOWN child.
        why = None
        if value == self.logic.UNKNOWN:
            why = next((r.reason for r in results if r.value == self.logic.UNKNOWN), None)
        return NodeResult(node, value, why, results)

    def _value_as_condition(self, node: Field | Const) -> NodeResult:
        result = self.value(node)
        try:
            truth = self.logic.from_value(result.value)
        except TypeMismatchError:
            path = node.path if isinstance(node, Field) else None
            got = _group(result.value)
            truth, why = self._mismatch(
                node, (got, "boolean"), path, f"{_type_label(result.value)} used as a condition"
            )
            return NodeResult(node, truth, why, (result,))
        why = result.reason if truth == self.logic.UNKNOWN else None
        return NodeResult(node, truth, why, (result,))

    def _compare(self, node: Expr, op: str, left: NodeResult, right: NodeResult) -> NodeResult:
        truth, why = self._compare_values(node, op, left, right)
        return NodeResult(node, truth, why, (left, right))

    def _compare_values(
        self, node: Expr, op: str, left: NodeResult, right: NodeResult
    ) -> tuple[Any, Reason | None]:
        a, b = left.value, right.value
        if a is None or b is None:  # null before types: never an error
            return self.logic.UNKNOWN, left.reason if a is None else right.reason
        group_a, group_b = _group(a), _group(b)
        if group_a == "boolean" and group_b == "boolean" and op in _ORDERING:
            reason = Reason("unorderable", _paths(node), ("boolean", "boolean"))
            return self._fail(node, reason)
        if group_a != group_b or group_a == "other":
            return self._mismatch(node, (group_a, group_b), None, _detail(a, b))
        return self.logic.from_bool(_COMPARE[op](a, b)), None

    def _in(self, node: In, operand: NodeResult) -> tuple[Any, Reason | None]:
        value = operand.value
        if value is None:
            return self.logic.UNKNOWN, operand.reason
        expected = _group(node.values[0])
        if _group(value) != expected or expected == "other":
            return self._mismatch(
                node, (_group(value), expected), None, _detail(value, node.values[0])
            )
        return self.logic.from_bool(value in node.values), None

    def _between(self, node: Between) -> NodeResult:
        operand, low, high = self.value(node.operand), self.value(node.low), self.value(node.high)
        low_op = "ge" if node.closed in ("both", "left") else "gt"
        high_op = "le" if node.closed in ("both", "right") else "lt"
        # Exactly `(x >= low) and (x <= high)` under the active logic (see the spec);
        # the halves are kept as children so explanations can say which one failed.
        low_half = self._compare(Compare(low_op, node.operand, node.low), low_op, operand, low)
        high_half = self._compare(Compare(high_op, node.operand, node.high), high_op, operand, high)
        truth = self.logic.and_(low_half.value, high_half.value)
        return self._combined(node, truth, (low_half, high_half))

    def _string_test(
        self, node: Matches | StringTest, operand: NodeResult
    ) -> tuple[Any, Reason | None]:
        value = operand.value
        if value is None:
            return self.logic.UNKNOWN, operand.reason
        if not isinstance(value, str):
            return self._mismatch(node, (_group(value), "string"), None, _detail(value, ""))
        if isinstance(node, Matches):
            found = _compiled(node.pattern).search(value) is not None
        elif node.op == "starts_with":
            found = value.startswith(node.text)
        elif node.op == "ends_with":
            found = value.endswith(node.text)
        else:
            found = node.text in value
        return self.logic.from_bool(found), None

    def _mismatch(
        self, node: Expr, groups: tuple[str, ...], path: Path | None, detail: str | None
    ) -> tuple[Any, Reason]:
        paths = (path,) if path is not None else _paths(node)
        return self._fail(node, Reason("type_mismatch", paths, groups, detail))

    def _fail(self, node: Expr, reason: Reason) -> tuple[Any, Reason]:
        if self.strict:
            raise TypeMismatchError(
                str(reason),
                types=reason.types,
                path=reason.paths[0] if reason.paths else None,
                rule=self.rule,
                node=node,
            )
        return self.logic.UNKNOWN, reason

    # --- values ----------------------------------------------------------------------

    def value(self, node: Expr) -> NodeResult:
        match node:
            case Field():
                return self._read(node)
            case Const():
                return NodeResult(node, node.value, Reason("null") if node.value is None else None)
            case Arith():
                left, right = self.value(node.left), self.value(node.right)
                result, why = self._arith(node, left, right)
                return NodeResult(node, result, why, (left, right))
            case Abs() | Neg():
                operand = self.value(node.operand)
                result, why = self._unary(node, operand)
                return NodeResult(node, result, why, (operand,))
            case _:  # pragma: no cover - conditions are never value operands
                raise ExpressionError(f"{node!r} is not a value")

    def _read(self, field: Field) -> NodeResult:
        current: object = self.data
        for i, segment in enumerate(field.path):
            if current is None:  # a null on the way: null, even in strict mode
                return NodeResult(field, None, Reason("null", (field.path[:i],)))
            try:
                following = _step(current, segment)
            except Exception as err:  # a property or __getitem__ raised
                return self._read_error(field, field.path[: i + 1], err)
            if following is _MISSING:
                if self.strict:
                    raise MissingFieldError(
                        field.path, rule=self.rule, node=field, hint=_available(current)
                    )
                return NodeResult(field, None, Reason("missing_field", (field.path[: i + 1],)))
            current = None if is_null(following) else following
        value = _normalize(current)
        if value is _OVERFLOW:
            return NodeResult(field, None, Reason("int_overflow", (field.path,)))
        return NodeResult(field, value, Reason("null", (field.path,)) if value is None else None)

    def _read_error(self, field: Field, path: Path, err: Exception) -> NodeResult:
        reason = Reason("read_error", (path,), detail=f"{type(err).__name__}: {err}")
        if self.strict:
            raise EvaluationError(str(reason), rule=self.rule, node=field) from err
        return NodeResult(field, None, reason)

    def _arith(self, node: Arith, left: NodeResult, right: NodeResult) -> tuple[Any, Reason | None]:
        a, b = left.value, right.value
        if a is None or b is None:
            return None, left.reason if a is None else right.reason
        if _group(a) != "number" or _group(b) != "number":
            _, why = self._mismatch(node, (_group(a), _group(b)), None, _detail(a, b))
            return None, why
        if b == 0 and node.op == "div":
            return None, Reason("division_by_zero", _paths(node))
        if node.op == "div" or isinstance(a, Decimal) or isinstance(b, Decimal):
            a, b = float(a), float(b)  # spec: division and Decimal operands are float64
        if node.op == "add":
            result = a + b
        elif node.op == "sub":
            result = a - b
        elif node.op == "mul":
            result = a * b
        else:
            result = a / b
        return _checked(result, node)

    def _unary(self, node: Abs | Neg, operand: NodeResult) -> tuple[Any, Reason | None]:
        value = operand.value
        if value is None:
            return None, operand.reason
        if _group(value) != "number":
            _, why = self._mismatch(node, (_group(value), "number"), None, None)
            return None, why
        if isinstance(value, Decimal):
            value = float(value)
        return _checked(abs(value) if isinstance(node, Abs) else -value, node)


def _checked(result: Any, node: Expr) -> tuple[Any, Reason | None]:
    """Turn arithmetic results the spec defines as null into `None` with a reason."""
    if isinstance(result, int) and not _I64_MIN <= result <= _I64_MAX:
        return None, Reason("int_overflow", _paths(node))
    if isinstance(result, float) and math.isnan(result):
        return None, Reason("nan_result", _paths(node))
    return result, None


def _step(obj: object, segment: str | int) -> object:
    """Follow one path segment, or return `_MISSING` (see the spec, "Reading data")."""
    # Type checks go on `obj`; operations on `target`, typed loosely on purpose
    # because it is arbitrary user data.
    target: Any = obj
    if isinstance(segment, int):
        if isinstance(obj, (str | bytes | Mapping, _OPAQUE)):
            return _MISSING
        if not (hasattr(obj, "__len__") and hasattr(obj, "__getitem__")):
            return _MISSING
        return target[segment] if segment < len(target) else _MISSING
    if isinstance(obj, Mapping):
        # `in` first: indexing would call __missing__ (and defaultdict would insert), and
        # `.get()` would bypass a subclass's own __getitem__.
        return target[segment] if segment in target else _MISSING  # noqa: SIM401
    if not isinstance(obj, _OPAQUE) and hasattr(type(obj), "keys"):  # pandas rows
        # `.keys()` explicitly: such objects need not implement __contains__.
        return target[segment] if segment in target.keys() else _MISSING  # noqa: SIM118
    if segment.startswith("_") or isinstance(obj, _OPAQUE):
        return _MISSING  # never private attributes, never attributes of scalars
    if _is_plain_sequence(obj):
        return _MISSING  # a name on a list is never data (namedtuples are records)
    found = getattr(target, segment, _MISSING)
    if inspect.isroutine(found) or isinstance(found, type | ModuleType):
        return _MISSING  # methods, classes and modules are not data
    return found


def _is_plain_sequence(obj: object) -> bool:
    return isinstance(obj, Sequence) and not hasattr(type(obj), "_fields")  # pyright: ignore[reportUnknownArgumentType]


_OVERFLOW: Final = object()


def _normalize(value: object) -> object:
    """Missing to `None`, numpy scalars to Python ones; out-of-range ints to `_OVERFLOW`."""
    if value is None or is_null(value):
        return None
    if isinstance(value, bool) or is_numpy_bool(value):
        return bool(value)  # pyright: ignore[reportUnknownArgumentType]
    if isinstance(value, numbers.Integral):
        number = operator.index(value)
        return number if _I64_MIN <= number <= _I64_MAX else _OVERFLOW
    if isinstance(value, Decimal | float | str | dt.date):
        return value
    if isinstance(value, numbers.Real):
        return float(value)
    cls = type(value)
    if cls.__module__ == "numpy" and cls.__name__ == "datetime64":
        converted: Any = value
        return converted.astype("datetime64[us]").item()
    return value


def _group(value: object) -> str:
    """Comparison group of a normalized value (see `docs/spec/ast.md`)."""
    if isinstance(value, bool):
        return "boolean"
    if isinstance(value, int | float | Decimal):
        return "number"
    if isinstance(value, str):
        return "string"
    if isinstance(value, dt.datetime):
        return "naive datetime" if value.tzinfo is None else "aware datetime"
    if isinstance(value, dt.date):
        return "date"
    return "other"


def _type_label(value: object) -> str:
    group = _group(value)
    return f"{group} ({type(value).__name__})" if group == "other" else group


def _detail(a: object, b: object) -> str | None:
    """Human text naming Python types, only when an `other` value is involved."""
    if _group(a) != "other" and _group(b) != "other":
        return None
    return f"{_type_label(a)} vs {_type_label(b)}"


def _paths(node: Expr) -> tuple[Path, ...]:
    """Paths of the fields among a node's direct children."""
    return tuple(child.path for child in node.children() if isinstance(child, Field))


def _available(container: object) -> str | None:
    if isinstance(container, Mapping):
        keys = [repr(k) for k in list(container)[:10]]  # pyright: ignore[reportUnknownArgumentType, reportUnknownVariableType]
        return f"available keys: {', '.join(keys)}" if keys else "the mapping is empty"
    return None


@lru_cache(maxsize=1024)
def _compiled(pattern: str) -> re.Pattern[str]:
    return re.compile(pattern)
