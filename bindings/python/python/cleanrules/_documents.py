"""Loading and dumping conditions, rules and rule sets as JSON or YAML documents.

    rules = cleanrules.load("pricing.yaml")
    text = cleanrules.dumps(rules, format="yaml")

The documents are the JSON format of `spec/json-format.md`, the contract between
languages: a rule set written here is read the same way in Rust, Java or Go.
YAML files hold the same documents (`spec/json-format.md`, *YAML*). Reading is
strict and bounded: unknown keys, duplicate keys and malformed values are errors
with a JSON Pointer or a line and column, and documents larger or deeper than
the limits are refused.
"""

from __future__ import annotations

import os
from pathlib import Path
from typing import IO, Literal, TypeAlias

from cleanrules import _cleanrules as _native
from cleanrules._expr import Expr, _wrap  # pyright: ignore[reportPrivateUsage]
from cleanrules._rules import Rule, RuleSet
from cleanrules.errors import ExpressionError, FormatError, UnsupportedError

__all__ = ["Format", "dump", "dumps", "load", "loads"]

Format: TypeAlias = Literal["json", "yaml"]
"""A document format."""

_SUFFIXES: dict[str, Format] = {".json": "json", ".yaml": "yaml", ".yml": "yaml"}


def loads(
    text: str | bytes,
    *,
    format: Format = "json",
    max_size: int | None = None,
    max_nesting: int | None = None,
    max_depth: int | None = None,
) -> Expr | Rule | RuleSet:
    """Read a condition, rule or rule set document.

    Args:
        text: The document, in JSON or YAML; `bytes` are read as UTF-8.
        format: `"json"` or `"yaml"`.
        max_size: The largest document, in bytes: 16 MiB by default for JSON
            and 1 MiB for YAML, whose parser holds a flow collection whole.
        max_nesting: The deepest nesting of objects and arrays (1024).
        max_depth: The deepest condition or payload value, in nodes (256).

    Returns:
        An `Expr` for a condition document, a `Rule` or a `RuleSet`. Rules read
        from a document have no action.

    Raises:
        FormatError: For invalid JSON or YAML, an unknown or duplicate key, a
            wrong type or an unknown version, with a JSON Pointer or a line and
            column; or a document beyond the limits.
        ExpressionError: If a condition breaks a construction rule.
        RuleDefinitionError: If a rule or rule set breaks a construction rule.
    """
    for name, limit in (
        ("max_size", max_size),
        ("max_nesting", max_nesting),
        ("max_depth", max_depth),
    ):
        if limit is not None and limit < 1:
            raise ValueError(f"{name} must be at least 1, got {limit}")
    if isinstance(text, bytes):
        try:
            text = text.decode("utf-8")
        except UnicodeDecodeError as error:
            raise FormatError(f"the document is not valid UTF-8: {error.reason}") from error
    kind, content = _native.load_document(text, format, max_size, max_nesting, max_depth)
    if kind == "condition":
        return _wrap(content)
    if kind == "rule":
        return _rule(content)
    rules, hit_policy, aggregate, logic, strict = content
    return RuleSet(
        [_rule(rule) for rule in rules],
        hit_policy=hit_policy,
        aggregate=aggregate,
        logic=logic,
        strict=strict,
    )


def _rule(native: _native.Rule) -> Rule:
    return Rule(
        native.name,
        _wrap(native.when),
        {key: _wrap(node) for key, node in native.then},
        priority=native.priority,
        tags=native.tags,
        description=native.description,
        metadata=native.metadata,
    )


def dumps(
    value: Expr | Rule | RuleSet,
    *,
    format: Format = "json",
    omit_actions: bool = False,
) -> str:
    """Write a condition, rule or rule set as a document, in its canonical form.

    Args:
        value: A condition, a rule or a rule set.
        format: `"json"` (compact) or `"yaml"` (block style).
        omit_actions: Write rules without their actions instead of refusing.
            Actions are code, which a document cannot hold.

    Returns:
        The document's text.

    Raises:
        ExpressionError: If `value` is a value node, not a condition.
        UnsupportedError: If a rule has an action and `omit_actions` is false;
            its `pointer` locates the rule.
    """
    if isinstance(value, Expr):
        if value.kind == "value":
            raise ExpressionError(
                f"only a condition can be a document, not the value {value!r}",
                hint="compare it first, e.g. F.x > 0",
            )
        return _native.dump_condition(value._node, format)  # pyright: ignore[reportPrivateUsage]
    if isinstance(value, Rule):
        _check_actions([value], "/rule", omit_actions)
        return _native.dump_rule(value._native, format)  # pyright: ignore[reportPrivateUsage]
    if isinstance(value, RuleSet):  # pyright: ignore[reportUnnecessaryIsInstance]
        _check_actions(list(value), "/ruleset/rules/{}", omit_actions)
        return _native.dump_ruleset(value._native, format)  # pyright: ignore[reportPrivateUsage]
    raise TypeError(f"dumps takes a condition, a rule or a rule set, not {type(value).__name__}")


def _check_actions(rules: list[Rule], pointer: str, omit_actions: bool) -> None:
    if omit_actions:
        return
    for position, rule in enumerate(rules):
        if rule.do is not None:
            raise UnsupportedError(
                f"the action of rule {rule.name!r} cannot be written: actions are code",
                hint="pass omit_actions=True to write the rule without it",
                pointer=pointer.format(position) + "/do",
                rule=rule.name,
            )


def load(
    source: str | os.PathLike[str] | IO[str],
    *,
    format: Format | None = None,
    max_size: int | None = None,
    max_nesting: int | None = None,
    max_depth: int | None = None,
) -> Expr | Rule | RuleSet:
    """Read a document from a file, as `loads` does.

    Args:
        source: A path (its suffix, `.json`, `.yaml` or `.yml`, gives the format)
            or an open text file, as `json.load` takes.
        format: The format, when the suffix does not say; required for a file
            object without a known name.
        max_size: As `loads`; a file larger than the limit is not read.
        max_nesting: As `loads`.
        max_depth: As `loads`.

    Raises:
        ValueError: If the format is not given and cannot be told.
        FormatError: As `loads`.
    """
    if isinstance(source, str | os.PathLike):
        file = Path(source)
        chosen = format or _format_of(file)
        limit = max_size or (1024 * 1024 if chosen == "yaml" else 16 * 1024 * 1024)
        size = file.stat().st_size
        if size > limit:
            raise FormatError(
                f"{file.name} has {size} bytes, more than the limit of {limit}",
                code="limit_exceeded",
                hint="raise max_size if the file is trusted",
            )
        text = file.read_text(encoding="utf-8")
    else:
        chosen = format or _format_of(Path(getattr(source, "name", "")))
        text = source.read()
    return loads(
        text,
        format=chosen,
        max_size=max_size,
        max_nesting=max_nesting,
        max_depth=max_depth,
    )


def dump(
    value: Expr | Rule | RuleSet,
    target: str | os.PathLike[str] | IO[str],
    *,
    format: Format | None = None,
    omit_actions: bool = False,
) -> None:
    """Write a document to a path or an open text file, as `dumps` does.

    A path's suffix (`.json`, `.yaml` or `.yml`) gives the format when it is not
    given.
    """
    if isinstance(target, str | os.PathLike):
        file = Path(target)
        text = dumps(value, format=format or _format_of(file), omit_actions=omit_actions)
        file.write_text(text, encoding="utf-8")
    else:
        chosen = format or _format_of(Path(getattr(target, "name", "")))
        target.write(dumps(value, format=chosen, omit_actions=omit_actions))


def _format_of(file: Path) -> Format:
    try:
        return _SUFFIXES[file.suffix.lower()]
    except KeyError:
        raise ValueError(
            f"cannot tell the format of {file.name!r} from its suffix; pass format='json' or"
            " format='yaml'"
        ) from None
