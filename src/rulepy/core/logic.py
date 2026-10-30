"""Truth values and the logics that combine them.

The evaluator never applies Python's `and`/`or`/`not` to truth values; it goes
through a `Logic` (invariant 3). `BooleanLogic` is Kleene's three-valued logic
(`TRUE`, `FALSE`, `UNKNOWN`); fuzzy logics (phase 5) implement the same protocol
over degrees in `[0, 1]`. Logics are registered by name, like fuzzytool's norms:

    logic = get_logic("boolean")
    logic.and_(Truth.TRUE, Truth.UNKNOWN)  # Truth.UNKNOWN

The `Logic` protocol is **provisional until 1.0**: it may gain members (with default
implementations where possible). Optional abilities are separate protocols, such
as `GradedLogic`, so most additions never touch existing logics.
"""

from __future__ import annotations

import enum
import functools
import threading
from collections.abc import Callable, Sequence
from typing import Any, Protocol, TypeVar, runtime_checkable

from rulepy.core._compat import is_null, is_numpy_bool
from rulepy.errors import ExpressionError, RegistryError, TypeMismatchError, UnsupportedOperation

__all__ = [
    "BooleanLogic",
    "GradedLogic",
    "Logic",
    "Truth",
    "get_logic",
    "logic_names",
    "register_logic",
    "unregister_logic",
]

TruthT = TypeVar("TruthT")


@functools.total_ordering
class Truth(enum.Enum):
    """Kleene truth values, ordered `FALSE < UNKNOWN < TRUE`.

    A `Truth` cannot be used as a Python bool, because `UNKNOWN` would silently
    count as true; ask the logic instead (`logic.is_true(value)`). It does not
    compare with numbers.
    """

    FALSE = 0
    UNKNOWN = 1
    TRUE = 2

    def __lt__(self, other: object) -> bool:
        if not isinstance(other, Truth):
            return NotImplemented
        return self.value < other.value

    def __bool__(self) -> bool:
        raise ExpressionError(
            f"{self} cannot be used as a Python bool",
            hint="use logic.is_true(value): only TRUE fires a rule, UNKNOWN does not",
        )

    def __str__(self) -> str:
        return self.name

    def __repr__(self) -> str:
        return f"Truth.{self.name}"


@runtime_checkable
class Logic(Protocol[TruthT]):
    """How truth values are produced and combined (provisional until 1.0).

    Laws every implementation must satisfy (tested for each registered logic):
    `and_`/`or_` are commutative and associative with `TRUE`/`FALSE` as identities
    and annihilators, and `not_` is an involution satisfying De Morgan's laws.

    Logics are shared across evaluations and threads, so they must be immutable and
    stateless; two logics with the same type and parameters must compare equal.
    """

    name: str
    TRUE: TruthT
    FALSE: TruthT
    UNKNOWN: TruthT

    def and_(self, a: TruthT, b: TruthT) -> TruthT:
        """Conjunction."""
        ...

    def or_(self, a: TruthT, b: TruthT) -> TruthT:
        """Disjunction."""
        ...

    def not_(self, a: TruthT) -> TruthT:
        """Negation."""
        ...

    def all_(self, values: Sequence[TruthT]) -> TruthT:
        """Conjunction of already evaluated values; `TRUE` when there are none.

        Taking a sequence, not a lazy iterable, guarantees every operand was
        evaluated, so results and errors never depend on operand order. Stopping
        early is only an optimization over values already computed.
        """
        return functools.reduce(self.and_, values, self.TRUE)

    def any_(self, values: Sequence[TruthT]) -> TruthT:
        """Disjunction of already evaluated values; `FALSE` when there are none."""
        return functools.reduce(self.or_, values, self.FALSE)

    def from_bool(self, value: bool | None) -> TruthT:
        """Convert a crisp result; `None` (missing) is `UNKNOWN`."""
        ...

    def from_value(self, value: object) -> TruthT:
        """Convert a data value used directly as a condition (e.g. `when=F.is_vip`).

        Missing values (see `rulepy.core._compat.is_null`) give `UNKNOWN`. A value
        that cannot be a truth value in this logic must raise `TypeMismatchError`,
        never return `UNKNOWN`: the evaluator decides between `UNKNOWN` and an error
        (strict mode) and adds the field path, rule and node.

        Raises:
            TypeMismatchError: If the value cannot be a truth value in this logic.
        """
        ...

    def is_true(self, value: TruthT) -> bool:
        """Return whether `value` makes a rule fire at full certainty."""
        ...


@runtime_checkable
class GradedLogic(Logic[TruthT], Protocol[TruthT]):
    """A logic whose truth values map to and from degrees in `[0, 1]`.

    Needed by degree-based features: soft comparisons and fuzzy terms (which produce
    degrees), firing degrees, alpha thresholds and degree-based hit policies.
    """

    def degree(self, value: TruthT) -> float | None:
        """Return the degree of `value`, or `None` when it is not determined."""
        ...

    def from_degree(self, degree: float) -> TruthT:
        """Return the truth value for `degree`.

        Raises:
            UnsupportedOperation: If this logic cannot represent the degree.
        """
        ...


_NOT = {Truth.FALSE: Truth.TRUE, Truth.UNKNOWN: Truth.UNKNOWN, Truth.TRUE: Truth.FALSE}
_DEGREE = {Truth.FALSE: 0.0, Truth.UNKNOWN: None, Truth.TRUE: 1.0}


class BooleanLogic:
    """Kleene's strong three-valued logic: `and` is the minimum, `or` the maximum.

    A missing value is `UNKNOWN`, and a rule fires only on `TRUE`. Unlike classical
    logic, `x | ~x` is `UNKNOWN` when `x` is. It has no parameters, so all instances
    are equal.
    """

    __slots__ = ()

    name = "boolean"
    TRUE = Truth.TRUE
    FALSE = Truth.FALSE
    UNKNOWN = Truth.UNKNOWN

    def and_(self, a: Truth, b: Truth) -> Truth:
        """Kleene conjunction (minimum)."""
        return min(a, b)

    def or_(self, a: Truth, b: Truth) -> Truth:
        """Kleene disjunction (maximum)."""
        return max(a, b)

    def not_(self, a: Truth) -> Truth:
        """Kleene negation: swaps `TRUE` and `FALSE`, keeps `UNKNOWN`."""
        return _NOT[a]

    def all_(self, values: Sequence[Truth]) -> Truth:
        """Kleene conjunction of evaluated values; `TRUE` when there are none."""
        result = Truth.TRUE
        for value in values:
            result = min(result, value)
            if result is Truth.FALSE:  # annihilator: no later value can change it
                break
        return result

    def any_(self, values: Sequence[Truth]) -> Truth:
        """Kleene disjunction of evaluated values; `FALSE` when there are none."""
        result = Truth.FALSE
        for value in values:
            result = max(result, value)
            if result is Truth.TRUE:
                break
        return result

    def from_bool(self, value: bool | None) -> Truth:
        """Map `True`/`False` to `TRUE`/`FALSE` and `None` to `UNKNOWN`."""
        if value is None:
            return Truth.UNKNOWN
        return Truth.TRUE if value else Truth.FALSE

    def from_value(self, value: object) -> Truth:
        """Map a boolean data value; missing is `UNKNOWN`, other types mismatch."""
        if is_null(value):
            return Truth.UNKNOWN
        if isinstance(value, bool) or is_numpy_bool(value):
            return self.from_bool(bool(value))  # pyright: ignore[reportUnknownArgumentType]
        raise TypeMismatchError(
            f"expected a boolean condition, got {type(value).__name__} {value!r}",
            types=("boolean", type(value).__name__),
            hint="compare the value instead, e.g. F.x > 0 or F.x == 'yes'",
        )

    def is_true(self, value: Truth) -> bool:
        """Only `TRUE` fires a rule; `UNKNOWN` does not."""
        return value is Truth.TRUE

    def degree(self, value: Truth) -> float | None:
        """`TRUE` is 1.0, `FALSE` is 0.0 and `UNKNOWN` has no degree."""
        return _DEGREE[value]

    def from_degree(self, degree: float) -> Truth:
        """Map exactly 0 and 1; intermediate degrees need a fuzzy logic."""
        if degree == 1:
            return Truth.TRUE
        if degree == 0:
            return Truth.FALSE
        raise UnsupportedOperation(
            "degree",
            "boolean logic",
            reason=f"{degree} is neither 0 nor 1",
            hint="soft comparisons and fuzzy terms need a fuzzy logic (phase 5)",
        )

    def __eq__(self, other: object) -> bool:
        return type(other) is type(self)

    def __hash__(self) -> int:
        return hash(type(self))

    def __repr__(self) -> str:
        return "BooleanLogic()"


_REGISTRY: dict[str, Callable[..., Logic[Any]]] = {}
_BUILTIN = frozenset({"boolean"})
_LOCK = threading.Lock()  # free-threaded builds: keep check-then-set atomic


def register_logic(
    name: str, factory: Callable[..., Logic[Any]], *, overwrite: bool = False
) -> None:
    """Register a logic factory under `name`.

    Args:
        name: Name used with `get_logic`, e.g. `"boolean"`.
        factory: Callable returning a `Logic`, usually its class; `get_logic` passes
            its keyword parameters through (e.g. `tnorm="prod"`).
        overwrite: Replace an existing registration instead of raising.

    Raises:
        RegistryError: If `name` is taken and `overwrite` is false, or `name` is a
            built-in logic.
    """
    with _LOCK:
        if name in _BUILTIN:
            raise RegistryError(f"{name!r} is a built-in logic and cannot be replaced")
        if name in _REGISTRY and not overwrite:
            raise RegistryError(
                f"logic {name!r} is already registered", hint="pass overwrite=True to replace it"
            )
        _REGISTRY[name] = factory


def unregister_logic(name: str) -> None:
    """Remove a registered logic.

    Raises:
        RegistryError: If `name` is unknown or a built-in logic.
    """
    with _LOCK:
        if name in _BUILTIN:
            raise RegistryError(f"{name!r} is a built-in logic and cannot be unregistered")
        if _REGISTRY.pop(name, None) is None:
            raise RegistryError(f"unknown logic {name!r}")


def get_logic(logic: str | Logic[Any], **params: Any) -> Logic[Any]:
    """Return a logic by name (built with `params`), or `logic` itself if it is one.

    Args:
        logic: A registered name, or a `Logic` instance.
        **params: Parameters for the logic's factory, e.g. `tnorm="prod"`.

    Raises:
        RegistryError: If no logic has that name, or parameters are given together
            with an instance.
    """
    if not isinstance(logic, str):
        if params:
            raise RegistryError(
                "parameters can only be given with a logic name, not an instance",
                hint="configure the instance directly",
            )
        return logic
    try:
        factory = _REGISTRY[logic]
    except KeyError:
        raise RegistryError(
            f"unknown logic {logic!r}", hint=f"use one of: {', '.join(sorted(_REGISTRY))}"
        ) from None
    return factory(**params)


def logic_names() -> frozenset[str]:
    """Return the names of all registered logics."""
    return frozenset(_REGISTRY)


_REGISTRY["boolean"] = BooleanLogic
