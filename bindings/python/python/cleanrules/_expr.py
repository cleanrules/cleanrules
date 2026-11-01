"""Conditions as data: the `F` proxy, constants and operators.

A condition is a tree of nodes that the core evaluates later, never a Python
function, so the same rule can be explained, serialized and audited
(`spec/ast.md`). Trees are built with `F` and Python operators:

    (F.customer.age >= 18) & F.country.is_in({"ES", "PT"}) & ~F.blocked

Every node is built by the core, which applies the construction rules. This
module adds the Python side: operators, and clear errors for idioms that cannot
work on nodes (`and`/`or`/`not`, chained comparisons, `== None`, iteration),
listed in the "Common pitfalls" guide. Like in polars or SQLAlchemy, `==` builds
a node, so structural equality is `same_as`, and nodes hash by identity.
"""

from __future__ import annotations

import difflib
import keyword
import math
import operator
from collections.abc import Iterable, Iterator, Mapping
from typing import Any, ClassVar, Literal, NoReturn, TypeAlias

from cleanrules import _cleanrules as _native
from cleanrules.errors import ExpressionError

__all__ = ["Closed", "Const", "Expr", "F", "Field", "Kind"]

Kind: TypeAlias = Literal["condition", "value", "any"]
"""Where a node may appear: as a `condition`, as a `value`, or as either (`any`)."""

Closed: TypeAlias = Literal["both", "left", "right", "none"]
"""Which ends of `between` are included (the same names as polars)."""


class Expr:
    """A node of a condition tree, immutable.

    Build nodes with `F`, `Const`, operators and methods, never directly.
    """

    __slots__ = ("_node",)

    _node: _native.Node

    def __init__(self, node: _native.Node) -> None:
        object.__setattr__(self, "_node", node)

    def __setattr__(self, name: str, value: object) -> NoReturn:
        raise AttributeError(f"condition nodes are immutable: cannot set {name!r}")

    # --- Introspection -------------------------------------------------------------

    @property
    def tag(self) -> str:
        """The node's tag, such as `"gt"` or `"and"` (`spec/ast.md`, *Catalogue*)."""
        return self._node.tag

    @property
    def kind(self) -> Kind:
        """Whether the node is a `condition`, a `value`, or `any` (depends on the data)."""
        kind = self._node.kind
        assert kind in ("condition", "value", "any")
        return kind

    def children(self) -> tuple[Expr, ...]:
        """Return the direct child nodes, in order."""
        return tuple(_wrap(child) for child in self._node.children())

    def walk(self) -> Iterator[Expr]:
        """Yield this node and all its descendants in pre-order (parents first)."""
        stack: list[Expr] = [self]
        while stack:
            node = stack.pop()
            yield node
            stack.extend(reversed(node.children()))

    def field_paths(self) -> tuple[tuple[str | int, ...], ...]:
        """Return the paths of every field this node reads, in first-appearance order.

        Each path appears once. A path may be a prefix of another:
        `F.customer.is_null() | (F.customer.age > 3)` reads `customer` and
        `customer.age`.
        """
        return self._node.field_paths()

    def same_as(self, other: object) -> bool:
        """Return whether `other` is a node with the same structure and constants."""
        return isinstance(other, Expr) and self._node.same_as(other._node)

    def structural_key(self) -> StructuralKey:
        """Return a hashable key that is equal for nodes that are `same_as` each other."""
        return StructuralKey(self)

    # --- Python protocol -----------------------------------------------------------

    def __repr__(self) -> str:
        return _render(self)

    def _repr_pretty_(self, printer: Any, cycle: bool) -> None:
        # Used by IPython and by hypothesis when it shows a failing example.
        printer.text(repr(self))

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

    # Make numpy arrays and pandas objects on the left of an operator defer to the
    # node (which then rejects them clearly) instead of broadcasting element by element.
    __array_ufunc__: ClassVar[None] = None
    __pandas_priority__: ClassVar[int] = 5000

    # Nodes are immutable, so a copy is the node itself.
    def __copy__(self) -> Expr:
        return self

    def __deepcopy__(self, memo: dict[int, object]) -> Expr:
        return self

    # Pickles hold the node in the JSON format, the contract between versions.
    def __reduce__(self) -> tuple[Any, tuple[str]]:
        return (_unpickle, (self._node.to_json(),))

    # --- Comparisons (return nodes, not bools) -------------------------------------

    def __eq__(self, other: object) -> Expr:  # pyright: ignore[reportIncompatibleMethodOverride]
        return _compare("eq", self, other)

    def __ne__(self, other: object) -> Expr:  # pyright: ignore[reportIncompatibleMethodOverride]
        return _compare("ne", self, other)

    def __lt__(self, other: object) -> Expr:
        return _compare("lt", self, other)

    def __le__(self, other: object) -> Expr:
        return _compare("le", self, other)

    def __gt__(self, other: object) -> Expr:
        return _compare("gt", self, other)

    def __ge__(self, other: object) -> Expr:
        return _compare("ge", self, other)

    # --- Logical connectives -------------------------------------------------------

    def __and__(self, other: object) -> Expr:
        return _Node(_native.and_([self._node, _lift_condition(other)._node]))

    def __rand__(self, other: object) -> Expr:
        return _Node(_native.and_([_lift_condition(other)._node, self._node]))

    def __or__(self, other: object) -> Expr:
        return _Node(_native.or_([self._node, _lift_condition(other)._node]))

    def __ror__(self, other: object) -> Expr:
        return _Node(_native.or_([_lift_condition(other)._node, self._node]))

    def __invert__(self) -> Expr:
        return _Node(_native.not_(self._node))

    # --- Arithmetic ----------------------------------------------------------------

    def __add__(self, other: object) -> Expr:
        return _arith("add", self, _lift(other))

    def __radd__(self, other: object) -> Expr:
        return _arith("add", _lift(other), self)

    def __sub__(self, other: object) -> Expr:
        return _arith("sub", self, _lift(other))

    def __rsub__(self, other: object) -> Expr:
        return _arith("sub", _lift(other), self)

    def __mul__(self, other: object) -> Expr:
        return _arith("mul", self, _lift(other))

    def __rmul__(self, other: object) -> Expr:
        return _arith("mul", _lift(other), self)

    def __truediv__(self, other: object) -> Expr:
        return _arith("div", self, _lift(other))

    def __rtruediv__(self, other: object) -> Expr:
        return _arith("div", _lift(other), self)

    def __neg__(self) -> Expr:
        return _Node(_native.neg(self._node))

    def __abs__(self) -> Expr:
        return _Node(_native.abs_(self._node))

    # --- Predicates and functions --------------------------------------------------

    def is_in(self, values: Iterable[object]) -> Expr:
        """Test membership in a set of constants, e.g. `F.country.is_in({"ES", "PT"})`.

        Args:
            values: A collection (list, tuple, set, ...) of non-null constants of one
                type group. A `str`, `bytes` or mapping is not accepted as the
                collection.

        Returns:
            An `in` node; its values are kept sorted and without duplicates.

        Raises:
            ExpressionError: If `values` is empty, not a collection, contains `None`
                or mixes type groups.
        """
        # Checked at run time too: untyped callers pass anything.
        name, shown = type(values).__name__, repr(values)
        if not isinstance(values, Iterable) or isinstance(values, str | bytes | Mapping):  # pyright: ignore[reportUnnecessaryIsInstance]
            raise ExpressionError(
                f"is_in needs a collection of constants, got {name} {shown}",
                hint='e.g. F.country.is_in(["ES", "PT"])',
            )
        return _Node(_native.is_in(self._node, list(values)))

    def between(self, low: object, high: object, closed: Closed = "both") -> Expr:
        """Test a range, e.g. `F.age.between(18, 65)` or `F.x.between(F.lo, F.hi)`.

        Args:
            low: The lower bound, a constant or a node.
            high: The upper bound, a constant or a node.
            closed: The included ends: `"both"` (default), `"left"`, `"right"` or
                `"none"`.

        Returns:
            A `between` node.

        Raises:
            ExpressionError: If `closed` is unknown, or both bounds are constants of
                different type groups or booleans.
        """
        if closed not in ("both", "left", "right", "none"):
            raise ExpressionError(
                f"closed must be 'both', 'left', 'right' or 'none', got {closed!r}"
            )
        return _Node(_native.between(self._node, _lift(low)._node, _lift(high)._node, closed))

    def is_null(self) -> Expr:
        """Test whether the value is missing or null; never unknown.

        Returns:
            An `is_null` node.
        """
        return _Node(_native.is_null(self._node))

    def matches(self, pattern: str) -> Expr:
        """Search a string with a regular expression, e.g. `F.email.matches(r".+@.+")`.

        The syntax is the Rust `regex` crate's: linear time, without lookaround or
        backreferences.

        Args:
            pattern: The regular expression (search semantics; `(?i)` for
                case-insensitive).

        Returns:
            A `matches` node.

        Raises:
            ExpressionError: If the pattern does not compile.
        """
        _require_text(pattern, "matches")
        return _Node(_native.matches(self._node, pattern))

    def starts_with(self, text: str) -> Expr:
        """Test that a string starts with the literal `text` (not a pattern).

        Args:
            text: The prefix.

        Returns:
            A `starts_with` node.
        """
        _require_text(text, "starts_with")
        return _Node(_native.string_test("starts_with", self._node, text))

    def ends_with(self, text: str) -> Expr:
        """Test that a string ends with the literal `text` (not a pattern).

        Args:
            text: The suffix.

        Returns:
            An `ends_with` node.
        """
        _require_text(text, "ends_with")
        return _Node(_native.string_test("ends_with", self._node, text))

    def contains(self, text: str) -> Expr:
        """Test that a string contains the literal `text`.

        This works on strings only: it is not list membership (use `is_in` to test a
        value against constants) and not a pattern (use `matches`).

        Args:
            text: The substring.

        Returns:
            A `contains` node.
        """
        _require_text(text, "contains")
        return _Node(_native.string_test("contains", self._node, text))

    def abs(self) -> Expr:
        """Return the absolute value of a number; the same as `abs(node)`.

        Returns:
            An `abs` node.
        """
        return _Node(_native.abs_(self._node))


class _Node(Expr):
    """A node that is neither a field nor a constant.

    `Field`, `Const` and this class are siblings, never subclasses of each other:
    Python tries the right operand's reflected comparison first when its class
    subclasses the left one's, which would turn `a > F.x` into `F.x < a`.
    """

    __slots__ = ()


class StructuralKey:
    """A hashable stand-in for a node, equal for nodes that are `same_as` each other.

    Use it to key dictionaries by structure: `{node.structural_key(): ...}`.
    """

    __slots__ = ("_hash", "_node")

    def __init__(self, node: Expr) -> None:
        self._node = node
        self._hash = node._node.structural_hash()  # pyright: ignore[reportPrivateUsage]

    def __eq__(self, other: object) -> bool:
        return isinstance(other, StructuralKey) and self._node.same_as(other._node)

    def __hash__(self) -> int:
        return self._hash

    def __repr__(self) -> str:
        return f"StructuralKey({self._node!r})"


class Field(Expr):
    """A value looked up in the input data by path, e.g. `Field("orders", 0, "total")`.

    `F.orders[0].total` builds the same field.

    Args:
        *path: Keys or attribute names (`str`) and sequence indices (`int` >= 0),
            outermost first.
    """

    __slots__ = ()

    def __init__(self, *path: str | int) -> None:
        if not path:
            raise ExpressionError(
                "a field path needs at least one segment", hint='e.g. Field("customer", "age")'
            )
        segments: list[str | int] = []
        for segment in path:
            if isinstance(segment, str) and segment:
                segments.append(str.__str__(segment))
            elif _is_index(segment):
                index = operator.index(segment)  # pyright: ignore[reportArgumentType]
                if index < 0:
                    raise ExpressionError(f"negative index {index} in field path {path!r}")
                segments.append(index)
            else:
                raise ExpressionError(
                    f"invalid field path segment {segment!r} in {path!r}",
                    hint="use non-empty names (str) and indices >= 0 (int)",
                )
        super().__init__(_native.field(segments))

    @property
    def path(self) -> tuple[str | int, ...]:
        """The keys and indices of the path, outermost first."""
        return self._node.details()["path"]

    def __getattr__(self, name: str) -> Field:
        # Only called when normal lookup fails. Underscore names are never fields, so
        # protocols probed by copy, numpy, ... (`__deepcopy__`, ...) still work.
        if name.startswith("_") or name in _RESERVED:
            raise AttributeError(name)
        if name in _NAMESPACES:
            raise AttributeError(
                f"{name!r} looks like a polars namespace; cleanrules methods live on the"
                f" field itself (e.g. F.x.starts_with(...)). For a field named {name!r} use"
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


class Const(Expr):
    """A constant: `None`, `bool`, `int`, `float`, `Decimal`, `str`, `date` or `datetime`.

    Python values next to a node become constants on their own (`F.x > 5`); write
    `Const(...)` where no node is involved, such as `Const(True)`. numpy scalars,
    `Enum` members and other subclasses are converted. NaN, integers outside 64
    bits and decimals beyond 38 digits are rejected, never rounded.

    Args:
        value: The constant.
    """

    __slots__ = ()

    def __init__(self, value: object) -> None:
        super().__init__(_native.constant(value))

    @property
    def value(self) -> object:
        """The constant, as a Python value."""
        return self._node.details()["value"]


class _FieldRoot:
    """The `F` proxy: `F.name` and `F["name"]` start a field path."""

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
"""The start of every field path: `F.customer.age >= 18`, `F["a key"]`, `F.items[0]`."""


# --- Building --------------------------------------------------------------------------


def _wrap(node: _native.Node) -> Expr:
    """Wrap a native node in the Python class for its tag."""
    cls = Field if node.tag == "field" else Const if node.tag == "const" else _Node
    wrapped = object.__new__(cls)
    object.__setattr__(wrapped, "_node", node)
    return wrapped


# Pickles store this function by name: keep the name.
def _unpickle(text: str) -> Expr:
    return _wrap(_native.node_from_json(text))


def _lift(value: object) -> Expr:
    """Return `value` if it is a node, else a constant."""
    return value if isinstance(value, Expr) else Const(value)


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


def _compare(op: str, left: Expr, right: object) -> Expr:
    if right is None:
        raise ExpressionError(
            f"comparing with None is never true: {left!r} {_COMPARE_SYMBOLS[op]} None",
            hint="missing values are unknown; test for them with .is_null()",
        )
    return _Node(_native.compare(op, left._node, _lift(right)._node))  # pyright: ignore[reportPrivateUsage]


def _arith(op: str, left: Expr, right: Expr) -> Expr:
    return _Node(_native.arith(op, left._node, right._node))  # pyright: ignore[reportPrivateUsage]


def _require_text(value: object, method: str) -> None:
    if not isinstance(value, str):
        raise ExpressionError(
            f"{method} takes a str, got {type(value).__name__} {value!r}",
            hint="the text is a literal (or, for matches, a pattern), never a node",
        )


def _is_index(value: object) -> bool:
    if isinstance(value, bool) or _is_numpy_bool(value):
        return False
    try:
        operator.index(value)  # pyright: ignore[reportArgumentType]
    except TypeError:
        return False
    return True


def _is_numpy_bool(value: object) -> bool:
    cls = type(value)
    return cls.__module__ == "numpy" and cls.__name__.startswith("bool")


# --- Rendering: repr() is Python that builds the same tree with F and Const -------------

_COMPARE_SYMBOLS = {"eq": "==", "ne": "!=", "lt": "<", "le": "<=", "gt": ">", "ge": ">="}
_ARITH_SYMBOLS = {"add": "+", "sub": "-", "mul": "*", "div": "/"}
_CONNECTIVES = frozenset({"and", "or"})
_COMPARISONS = frozenset(_COMPARE_SYMBOLS)
_ARITHMETIC = frozenset(_ARITH_SYMBOLS)
# Names that already mean something on a Field (methods, properties, `path`, ...).
_RESERVED = frozenset(name for name in dir(Field) if not name.startswith("_"))
# Builder methods, for "did you mean" hints when a field path is called.
_METHODS = frozenset(
    name for name in _RESERVED if callable(getattr(Field, name)) and name.islower()
)
# polars-style namespaces (F.x.str.starts_with) would silently become field paths.
_NAMESPACES = frozenset({"str", "dt", "list", "arr", "struct", "cat"})


def _render(node: Expr) -> str:
    tag = node.tag
    details = node._node.details()  # pyright: ignore[reportPrivateUsage]
    children = node.children()
    if tag == "field":
        return _render_path(details["path"])
    if tag == "const":
        return f"Const({format_constant(details['value'])})"
    if tag in _COMPARISONS:
        left, right = _operand(children[0]), _operand(children[1], literal=True)
        return f"{left} {_COMPARE_SYMBOLS[tag]} {right}"
    if tag in _CONNECTIVES:
        symbol = " & " if tag == "and" else " | "
        parse = _COMPARISONS | _CONNECTIVES
        return symbol.join(_operand(child, parens=parse) for child in children)
    if tag == "not":
        return f"~{_operand(children[0], parens=_COMPARISONS | _CONNECTIVES)}"
    if tag == "in":
        values = ", ".join(format_constant(v) for v in details["values"])
        return f"{_receiver(children[0])}.is_in([{values}])"
    if tag == "between":
        closed = "" if details["closed"] == "both" else f", closed={details['closed']!r}"
        low, high = _operand(children[1], literal=True), _operand(children[2], literal=True)
        return f"{_receiver(children[0])}.between({low}, {high}{closed})"
    if tag == "is_null":
        return f"{_receiver(children[0])}.is_null()"
    if tag == "matches":
        return f"{_receiver(children[0])}.matches({details['pattern']!r})"
    if tag in ("starts_with", "ends_with", "contains"):
        return f"{_receiver(children[0])}.{tag}({details['text']!r})"
    if tag in _ARITHMETIC:
        left = _operand(children[0], parens=_ARITHMETIC)
        right = _operand(children[1], literal=True, parens=_ARITHMETIC)
        return f"{left} {_ARITH_SYMBOLS[tag]} {right}"
    if tag == "abs":
        return f"abs({_operand(children[0])})"
    if tag == "neg":
        return f"-{_operand(children[0], parens=_ARITHMETIC)}"
    return f"<node {tag}>"  # pragma: no cover - a tag added to the core later


def _operand(node: Expr, *, literal: bool = False, parens: frozenset[str] = frozenset()) -> str:
    # A bare literal is only safe where the other operand is already a node, so Python
    # dispatches to the node's operator (`F.x > 5`); elsewhere it would be computed by
    # Python itself (`1 + 2`, `True & False`), so it is spelled `Const(...)`.
    if literal and node.tag == "const":
        return format_constant(node._node.details()["value"])  # pyright: ignore[reportPrivateUsage]
    text = _render(node)
    return f"({text})" if node.tag in parens else text


def _receiver(node: Expr) -> str:
    return _operand(node, parens=_ARITHMETIC | {"neg"})


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


def format_constant(value: object) -> str:
    """Render a constant as Python source, as `repr()` of nodes does (`float('inf')`)."""
    if isinstance(value, float) and math.isinf(value):
        return "float('inf')" if value > 0 else "float('-inf')"
    return repr(value)
