"""A rule: a named condition, what it outputs and what it does when it fires.

    Rule("vip", when=(F.years > 5) & (F.total > 1000), then={"discount": 0.15})
    Rule("alert", when=F.amount > 10_000, then={"review": True}, do=notify_compliance)

`then` is the **payload**: a mapping whose values are constants or value expressions
computed from the data (`{"discount": F.total * 0.1}`). `do` is an optional
**action**, a callable that `RuleSet.run` calls with an `ActionContext`;
`RuleSet.evaluate` never runs actions. Semantics: `docs/spec/rules.md`.
"""

from __future__ import annotations

from collections.abc import Callable, Iterable, Iterator, Mapping
from dataclasses import dataclass
from typing import TYPE_CHECKING, Any, TypeAlias, TypeVar, cast

from rulepy.core._compat import I64_MAX, I64_MIN
from rulepy.core.expr import Const, Expr
from rulepy.errors import ExpressionError, RuleDefinitionError

if TYPE_CHECKING:
    from rulepy.core.evaluate import NodeResult

__all__ = ["Action", "ActionContext", "FrozenMap", "Rule"]

K = TypeVar("K")
V = TypeVar("V")


class FrozenMap(Mapping[K, V]):
    """A read-only mapping that, unlike `MappingProxyType`, pickles and deep-copies.

    It deliberately has no `__eq__` of its own beyond `Mapping`'s: payload values
    are nodes, whose `==` builds conditions, so compare payloads structurally.
    """

    __slots__ = ("_data",)

    def __init__(self, data: Mapping[K, V] | Iterable[tuple[K, V]] = ()) -> None:
        self._data: dict[K, V] = dict(data)

    def __getitem__(self, key: K) -> V:
        return self._data[key]

    def __iter__(self) -> Iterator[K]:
        return iter(self._data)

    def __len__(self) -> int:
        return len(self._data)

    def __repr__(self) -> str:
        return f"FrozenMap({self._data!r})"

    def __reduce__(self) -> tuple[Any, tuple[Any, ...]]:
        return (type(self), (self._data,))

    __hash__ = None  # type: ignore[assignment]  # mutable-looking contents, like dict


@dataclass(frozen=True, slots=True, eq=False)
class ActionContext:
    """What an action receives.

    Attributes:
        rule: The rule whose action runs.
        data: The input the rule set was run on.
        tree: The evaluation result of the rule's condition.
        output: The rule's evaluated payload (`{}` if it has none).
    """

    rule: Rule
    data: object
    tree: NodeResult
    output: Mapping[str, object]


Action: TypeAlias = Callable[[ActionContext], object]
"""A callable that `RuleSet.run` calls when its rule is selected; its result is ignored."""


@dataclass(frozen=True, slots=True, eq=False, init=False, repr=False)
class Rule:
    """A named condition, its payload and its optional action.

    Rules compare and hash by identity (their conditions are nodes, whose `==` builds
    conditions). They are immutable, pickle and deep-copy (when their action does).

    Args:
        name: Unique name within a rule set; appears in decisions and errors.
        when: The condition; the rule fires when it passes the logic's activation
            test (`TRUE` under Kleene logic).
        then: The payload: a mapping with string keys whose values are scalar
            constants (normalized like `Const`) or value expressions such as
            `F.total * 0.1`. Stored as `FrozenMap[str, Expr]`.
        do: An action callable, run by `RuleSet.run` (never by `evaluate`).
        priority: Used by the `PRIORITY` hit policy; higher wins.
        tags: Labels for grouping and filtering.
        description: Human-readable explanation, shown in explanations.
        metadata: Free-form data (owner, ticket, version, ...), stored frozen.

    Raises:
        RuleDefinitionError: If any argument is invalid.
    """

    name: str
    when: Expr
    then: FrozenMap[str, Expr] | None
    do: Action | None
    priority: int
    tags: frozenset[str]
    description: str | None
    metadata: FrozenMap[str, object]

    def __init__(
        self,
        name: str,
        when: Expr,
        then: Mapping[str, object] | None = None,
        *,
        do: Action | None = None,
        priority: int = 0,
        tags: Iterable[str] = (),
        description: str | None = None,
        metadata: Mapping[str, object] | None = None,
    ) -> None:
        if not isinstance(name, str) or not name:  # pyright: ignore[reportUnnecessaryIsInstance]
            raise RuleDefinitionError(f"a rule name must be a non-empty string, got {name!r}")
        _check_condition(name, when)
        if do is not None and not callable(do):
            raise RuleDefinitionError(f"do must be a callable, got {do!r}", rule=name)
        if type(priority) is not int:
            raise RuleDefinitionError(
                f"priority must be an int, got {type(priority).__name__} {priority!r}", rule=name
            )
        if not I64_MIN <= priority <= I64_MAX:
            raise RuleDefinitionError(
                f"priority {priority} does not fit in 64 bits",
                rule=name,
                hint="polars, Rust and JSON cannot represent it",
            )
        if description is not None and not isinstance(description, str):  # pyright: ignore[reportUnnecessaryIsInstance]
            raise RuleDefinitionError(
                f"description must be a string, got {type(description).__name__} {description!r}",
                rule=name,
            )
        if isinstance(tags, str):
            raise RuleDefinitionError(
                f"tags must be a collection of strings, not the string {tags!r}",
                rule=name,
                hint=f"use tags=[{tags!r}]",
            )
        try:
            frozen_tags = frozenset(tags)
        except TypeError:
            raise RuleDefinitionError(
                f"tags must be a collection of strings, got {tags!r}", rule=name
            ) from None
        if not all(isinstance(tag, str) for tag in frozen_tags):  # pyright: ignore[reportUnnecessaryIsInstance]
            raise RuleDefinitionError("tags must be strings", rule=name)
        frozen_metadata = _check_metadata(name, metadata)
        object.__setattr__(self, "name", name)
        object.__setattr__(self, "when", when)
        object.__setattr__(self, "then", _check_payload(name, then))
        object.__setattr__(self, "do", do)
        object.__setattr__(self, "priority", priority)
        object.__setattr__(self, "tags", frozen_tags)
        object.__setattr__(self, "description", description)
        object.__setattr__(self, "metadata", frozen_metadata)

    def __reduce__(self) -> tuple[Any, tuple[Any, ...]]:
        # Rebuilt by keyword through the constructor, so fields added in later versions
        # take their defaults when older pickles are loaded, and validation runs again.
        options = {
            "then": None if self.then is None else dict(self.then),
            "do": self.do,
            "priority": self.priority,
            "tags": tuple(sorted(self.tags)),
            "description": self.description,
            "metadata": dict(self.metadata),
        }
        return (_rebuild_rule, (self.name, self.when, options))

    def __repr__(self) -> str:
        parts = [repr(self.name), f"when={self.when!r}"]
        if self.then is not None:
            items = ", ".join(f"{k!r}: {_show(v)}" for k, v in self.then.items())
            parts.append(f"then={{{items}}}")
        if self.do is not None:
            parts.append(f"do={getattr(self.do, '__qualname__', repr(self.do))}")
        if self.priority:
            parts.append(f"priority={self.priority}")
        if self.tags:
            parts.append(f"tags={sorted(self.tags)!r}")
        return f"Rule({', '.join(parts)})"


# Pickles store this function by name: keep it here and keep the name stable.
def _rebuild_rule(name: str, when: Expr, options: dict[str, Any]) -> Rule:
    return Rule(name, when, **options)


def _check_metadata(name: str, metadata: object) -> FrozenMap[str, object]:
    if metadata is None:
        return FrozenMap()
    if not isinstance(metadata, Mapping):
        raise RuleDefinitionError(
            f"metadata must be a mapping, got {type(metadata).__name__} {metadata!r}", rule=name
        )
    items: Mapping[object, object] = metadata  # pyright: ignore[reportUnknownVariableType]
    if not all(isinstance(key, str) for key in items):
        raise RuleDefinitionError("metadata keys must be strings", rule=name)
    return FrozenMap(cast("Mapping[str, object]", items))


def _show(value: Expr) -> str:
    return repr(value.value) if isinstance(value, Const) else repr(value)


def _check_condition(name: str, when: object) -> None:
    if not isinstance(when, Expr):
        raise RuleDefinitionError(
            f"the condition (`when`) must be a condition node, got {type(when).__name__} {when!r}",
            rule=name,
            hint="build it with F, e.g. when=F.age >= 18",
        )
    if when.kind == "value":
        raise RuleDefinitionError(
            f"the condition (`when`) is a value, not a condition: {when!r}",
            rule=name,
            hint="compare it, e.g. when=(F.a + F.b) > 0",
        )


def _check_payload(name: str, then: object) -> FrozenMap[str, Expr] | None:
    if then is None:
        return None
    if not isinstance(then, Mapping):
        hint = "pass actions with do=..." if callable(then) else "e.g. then={'discount': 0.1}"
        raise RuleDefinitionError(
            f"then must be a mapping (the payload), got {type(then).__name__} {then!r}",
            rule=name,
            hint=hint,
        )
    payload: dict[str, Expr] = {}
    items: Mapping[object, object] = then  # pyright: ignore[reportUnknownVariableType]
    for key, value in items.items():
        if not isinstance(key, str):
            raise RuleDefinitionError(f"payload keys must be strings, got {key!r}", rule=name)
        if isinstance(value, Expr):
            if value.kind == "condition":
                raise RuleDefinitionError(
                    f"payload value for {key!r} is a condition: {value!r}",
                    rule=name,
                    hint="payload values are constants or value expressions such as F.total * 0.1",
                )
            payload[key] = value
            continue
        try:
            payload[key] = Const(value)  # pyright: ignore[reportArgumentType]
        except ExpressionError as err:
            raise RuleDefinitionError(
                f"payload value for {key!r}: {err.message}", rule=name, hint=err.hint
            ) from None
    return FrozenMap(payload)
