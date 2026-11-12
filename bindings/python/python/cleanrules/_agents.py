"""Tools for AI agents (`spec/agents.md`): the core's catalog answers each call.

A `Catalog` holds rule sets by name. A call's arguments and its result are
JSON, the same in every language and in the MCP server, so an adapter for an
agent framework only passes them through. Tools never run actions.
"""

from __future__ import annotations

import enum
import json
import math
from collections.abc import Iterable, Mapping, Sequence
from dataclasses import dataclass
from typing import Any, cast

from cleanrules import _cleanrules as _native
from cleanrules._rules import RuleSet

TOOLS: tuple[str, ...] = tuple(_native.TOOLS)
"""The tools' names, sorted, as the core defines them."""


class Tool(enum.StrEnum):
    """The tools of `spec/agents.md`."""

    EVALUATE = "evaluate"
    """The decision: the rules selected and their outputs."""
    EXPLAIN = "explain"
    """The decision and a compact explanation within a budget."""
    LIST_RULESETS = "list_rulesets"
    """The rule sets of the catalog."""
    SUMMARY = "summary"
    """What a rule set reads and decides, and the JSON Schema of its input."""
    VALIDATE = "validate"
    """Whether a document reads, and what it holds."""


@dataclass(frozen=True)
class ToolResult:
    """What a tool call gave.

    Attributes:
        is_error: Whether it is a tool error, which the model may correct.
        content: The result, or `{"error": {...}}` with the error's code,
            message, hint and location. An MCP server sends it as the
            structured content, and its JSON as the text.
    """

    is_error: bool
    content: Mapping[str, Any]


class Catalog:
    """Rule sets by name that answer tool calls, as the core's `Catalog`.

    Args:
        rulesets: The rule sets an agent may use, by name. Rules whose action is
            Python code are shown as having one in `summary`; actions never run.
        tools: The tools it offers (`Tool` or their names); every tool by default.

    Raises:
        TypeError: If a name is not a `str` or a rule set not a `RuleSet`.
        ValueError: If a tool is not one of `TOOLS`.
    """

    __slots__ = ("_native",)

    def __init__(
        self,
        rulesets: Mapping[str, RuleSet] | None = None,
        *,
        tools: Iterable[Tool | str] | None = None,
    ) -> None:
        given = dict(rulesets or {})
        natives: list[_native.RuleSet] = []
        actions: list[list[bool]] = []
        for name, ruleset in given.items():
            if not isinstance(name, str):  # pyright: ignore[reportUnnecessaryIsInstance]
                raise TypeError(f"rule set names are str, not {type(name).__name__}")
            if not isinstance(ruleset, RuleSet):  # pyright: ignore[reportUnnecessaryIsInstance]
                raise TypeError(f"rulesets[{name!r}] is a {type(ruleset).__name__}, not a RuleSet")
            natives.append(ruleset._native)  # pyright: ignore[reportPrivateUsage]
            actions.append([rule.do is not None for rule in ruleset])
        names = None if tools is None else [Tool(tool).value for tool in tools]
        self._native = _native.Catalog(list(given), natives, actions, names)

    def names(self) -> tuple[str, ...]:
        """The names of its rule sets, sorted by Unicode code point."""
        return tuple(self._native.names())

    @property
    def tools(self) -> tuple[Tool, ...]:
        """The tools it offers, sorted by name."""
        return tuple(Tool(name) for name in self._native.tools())

    def __len__(self) -> int:
        return len(self._native.names())

    def __contains__(self, name: object) -> bool:
        return name in self._native.names()

    def call(self, tool: Tool | str, arguments: Mapping[str, Any] | str) -> ToolResult:
        """Answer a call of one of its tools.

        Args:
            tool: The tool, or its name; one it does not offer is a tool error.
            arguments: The arguments: JSON text, or a mapping of data. Mappings,
                lists and tuples, `str`, `int`, `float`, `bool` and `None` are
                JSON as they are, so an object with a key that starts with `$`
                is a typed value of the JSON format (`{"$date": "2026-11-12"}`).
                Any other value is written in the format's encoding, as
                `RuleSet.evaluate` reads it: a `Decimal` as `{"$decimal": ...}`,
                a `date` or a `datetime` as `{"$date": ...}` or
                `{"$datetime": ...}`, an infinite float as `{"$float": "inf"}`.

        Returns:
            The result, or a tool error with a pointer into the arguments and a
            hint, which the model can correct.

        Raises:
            TypeError: If the arguments are neither text nor a mapping, a key is
                not a `str`, or a value has no JSON form (a set, an object).
            ValueError: For a NaN, a datetime finer than a microsecond, or a
                mapping or list that contains itself.
        """
        name = tool.value if isinstance(tool, Tool) else tool
        if isinstance(arguments, str):
            text = arguments
        elif isinstance(arguments, Mapping):  # pyright: ignore[reportUnnecessaryIsInstance]
            text = json.dumps(_encode(dict(arguments), set()), allow_nan=False, ensure_ascii=False)
        else:
            raise TypeError(f"arguments are a mapping or JSON text, not {type(arguments).__name__}")
        is_error, content = self._native.call(name, text)
        return ToolResult(is_error=is_error, content=json.loads(content))

    def __repr__(self) -> str:
        return f"<Catalog: {', '.join(self.names())}>"


def _encode(value: object, open_: set[int]) -> object:
    """`value` as JSON data, native scalars in the JSON format's encoding."""
    if value is None or isinstance(value, str | bool | int):
        return value
    if isinstance(value, float) and math.isfinite(value):
        return value
    identity = id(value)
    if not isinstance(value, Mapping | list | tuple):
        return json.loads(_native.scalar_json(value))
    if identity in open_:
        raise ValueError("the arguments contain themselves")
    open_.add(identity)
    try:
        if isinstance(value, Mapping):
            out: dict[str, object] = {}
            for key, item in cast(Mapping[object, object], value).items():
                if not isinstance(key, str):
                    raise TypeError(f"keys are str, not {type(key).__name__}: {key!r}")
                out[key] = _encode(item, open_)
            return out
        return [_encode(item, open_) for item in cast(Sequence[object], value)]
    finally:
        open_.discard(identity)
