"""Loading and dumping conditions, rules, rule sets and decision tables.

    rules = cleanrules.load("pricing.yaml")
    text = cleanrules.dumps(rules, format="yaml")
    vip = cleanrules.rule_from_text("RULE vip IF customer.years > 5 THEN discount = 0.15")

The documents are the JSON format of `spec/json-format.md`, the contract between
languages: a rule set written here is read the same way in Rust, Java or Go.
YAML files hold the same documents (`spec/json-format.md`, *YAML*), the text
syntax (`spec/text.md`) the same model, as people write rules on paper, and CSV
holds decision tables (`spec/tables.md`), as spreadsheets write them. Reading
is strict and bounded: unknown keys, duplicate keys, malformed values and syntax
errors are errors with a JSON Pointer or a line and column, and documents larger
or deeper than the limits are refused.
"""

from __future__ import annotations

import os
from collections.abc import Mapping
from pathlib import Path
from typing import IO, Literal, TypeAlias

from cleanrules import _cleanrules as _native
from cleanrules._expr import Expr, _wrap  # pyright: ignore[reportPrivateUsage]
from cleanrules._rules import Action, Rule, RuleSet
from cleanrules._tables import DecisionTable
from cleanrules.errors import ExpressionError, FormatError, UnsupportedError

__all__ = ["Format", "dump", "dumps", "load", "loads", "rule_from_text"]

Format: TypeAlias = Literal["json", "yaml", "text", "csv"]
"""A document format: JSON, YAML, the text syntax of rules, or CSV for decision tables."""

_DEFAULT_SIZES: dict[Format, int] = {
    "json": 16 << 20,
    "yaml": 1 << 20,
    "text": 4 << 20,
    "csv": 4 << 20,
}

_SUFFIXES: dict[str, Format] = {
    ".json": "json",
    ".yaml": "yaml",
    ".yml": "yaml",
    ".rules": "text",
    ".csv": "csv",
}


def loads(
    text: str | bytes,
    *,
    format: Format = "json",
    max_size: int | None = None,
    max_nesting: int | None = None,
    max_depth: int | None = None,
    max_table_size: int | None = None,
    actions: Mapping[str, Action] | None = None,
) -> Expr | Rule | RuleSet | DecisionTable:
    """Read a condition, rule, rule set or decision table document.

    Args:
        text: The document, in JSON, YAML, the text syntax or CSV; `bytes` are
            read as UTF-8.
        format: `"json"`, `"yaml"`, `"text"` or `"csv"` (a decision table,
            `spec/tables.md`).
        max_size: The largest document, in bytes: 16 MiB by default for JSON,
            4 MiB for text and CSV, which hold up to a node in every two bytes,
            and 1 MiB for YAML, whose parser holds a flow collection whole.
        max_nesting: The deepest nesting of JSON or YAML objects and arrays
            (1024); text has none.
        max_depth: The deepest condition or payload value, in nodes (256).
        max_table_size: The most cells and condition nodes a decision table
            may hold, column copies included (2,097,152; `spec/tables.md`,
            *Limits*).
        actions: An action registry, each name with its callable: a rule or
            rule set read is bound to it (`RuleSet.bind`). Without it, rules
            keep the names of their actions, unbound: they evaluate, explain
            and write, but `run` refuses them.

    Returns:
        An `Expr` for a condition document, a `Rule`, a `RuleSet` or a
        `DecisionTable`. A rule's action is the name the document gives it, if
        any, never code.

    Raises:
        FormatError: For invalid JSON, YAML or CSV, an unknown or duplicate
            key, a wrong type or an unknown version, with a JSON Pointer or a
            line and column; or a document beyond the limits.
        RuleSyntaxError: For a text that does not follow the syntax, with the
            line and column of the first token that cannot be read.
        ExpressionError: If a condition breaks a construction rule; read from
            text, with the line and column where the node starts.
        RuleDefinitionError: If a rule or rule set breaks a construction rule.
        RegistryError: For an unknown logic, or a named action that `actions`
            lacks.
    """
    text = _text(
        text,
        max_size=max_size,
        max_nesting=max_nesting,
        max_depth=max_depth,
        max_table_size=max_table_size,
    )
    kind, content = _native.load_document(
        text, format, max_size, max_nesting, max_depth, max_table_size
    )
    if kind == "condition":
        return _wrap(content)
    if kind == "rule":
        rule = _rule(content)
        return rule if actions is None else RuleSet([rule]).bind(actions)[0]
    if kind == "decision_table":
        return DecisionTable._from_native(content)  # pyright: ignore[reportPrivateUsage]
    ruleset = _ruleset(content)
    return ruleset if actions is None else ruleset.bind(actions)


def _ruleset(parts: tuple[list[_native.Rule], str, str | None, str, bool]) -> RuleSet:
    """A rule set from the core's parts: rules, policy, aggregate, logic, strict."""
    rules, hit_policy, aggregate, logic, strict = parts
    return RuleSet(
        [_rule(rule) for rule in rules],
        hit_policy=hit_policy,
        aggregate=aggregate,
        logic=logic,
        strict=strict,
    )


def _text(text: str | bytes, **limits: int | None) -> str:
    """The text as `str`, once the limits are checked."""
    for name, limit in limits.items():
        if limit is not None and limit < 1:
            raise ValueError(f"{name} must be at least 1, got {limit}")
    if isinstance(text, bytes):
        try:
            return text.decode("utf-8")
        except UnicodeDecodeError as error:
            raise FormatError(f"the document is not valid UTF-8: {error.reason}") from error
    return text


def _rule(native: _native.Rule) -> Rule:
    return Rule(
        native.name,
        _wrap(native.when),
        {key: _wrap(node) for key, node in native.then},
        priority=native.priority,
        tags=native.tags,
        description=native.description,
        metadata=native.metadata,
        do=native.action_name,
    )


def dumps(
    value: Expr | Rule | RuleSet | DecisionTable,
    *,
    format: Format = "json",
    omit_actions: bool = False,
) -> str:
    """Write a condition, rule, rule set or decision table, in its canonical form.

    Args:
        value: A condition, a rule, a rule set or a decision table.
        format: `"json"` (compact), `"yaml"` (block style), `"text"` (the
            canonical text, `IF … THEN …`) or `"csv"` (decision tables only).
        omit_actions: Write rules without their actions instead of refusing.
            Actions are code, which a document cannot hold.

    Returns:
        The document's text.

    Raises:
        ExpressionError: If `value` is a value node, not a condition.
        UnsupportedError: If a rule has an action and `omit_actions` is false;
            its `pointer` locates the rule. In text, also for a rule with
            metadata, which the text syntax cannot hold, and for a decision
            table.
        UnsupportedError: Also for CSV and anything but a decision table.
    """
    if isinstance(value, DecisionTable):
        return _native.dump_table(value._native, format)  # pyright: ignore[reportPrivateUsage]
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
    raise TypeError(
        "dumps takes a condition, a rule, a rule set or a decision table,"
        f" not {type(value).__name__}"
    )


def _check_actions(rules: list[Rule], pointer: str, omit_actions: bool) -> None:
    if omit_actions:
        return
    for position, rule in enumerate(rules):
        # A named action is written as its name; only code cannot be.
        if rule.do is not None and rule.action_name is None:
            raise UnsupportedError(
                f"the action of rule {rule.name!r} cannot be written: it is code without a name",
                hint='name it (do="notify") and bind it with RuleSet.bind, or pass'
                " omit_actions=True to write the rule without it",
                pointer=pointer.format(position) + "/do",
                rule=rule.name,
            )


def rule_from_text(
    text: str | bytes,
    *,
    do: Action | None = None,
    actions: Mapping[str, Action] | None = None,
    max_size: int | None = None,
    max_depth: int | None = None,
) -> Rule:
    """Read one rule written in the text syntax (`spec/text.md`).

        vip = rule_from_text("RULE vip IF customer.years > 5 THEN discount = 0.15")

    Line breaks are free, as in every text. Without a `RULE` line the rule is
    named `rule_1`. To read a condition or a rule set written as text, use
    `loads(text, format="text")`.

    Args:
        text: The rule; `bytes` are read as UTF-8.
        do: The rule's action, as `Rule` takes: text holds no code. When the
            text names an action (`DO notify`), `do` is bound to that name.
        actions: An action registry, as `loads` takes; not with `do`.
        max_size: As `loads`.
        max_depth: As `loads`.

    Raises:
        RuleSyntaxError: If the text does not follow the syntax, or holds a
            condition or a rule set instead of one rule.
        ExpressionError: As `loads`.
        RuleDefinitionError: As `loads`.
        RegistryError: As `loads`.
    """
    if do is not None and actions is not None:
        raise TypeError("rule_from_text takes do or actions, not both")
    text = _text(text, max_size=max_size, max_depth=max_depth)
    rule = _rule(_native.load_rule(text, max_size, max_depth))
    if actions is not None:
        return RuleSet([rule]).bind(actions)[0]
    if do is None:
        return rule
    if rule.action_name is not None:
        return rule._bound(do)  # pyright: ignore[reportPrivateUsage]
    return Rule(
        rule.name,
        rule.when,
        rule.then,
        priority=rule.priority,
        tags=rule.tags,
        description=rule.description,
        do=do,
    )


def load(
    source: str | os.PathLike[str] | IO[str],
    *,
    format: Format | None = None,
    max_size: int | None = None,
    max_nesting: int | None = None,
    max_depth: int | None = None,
    max_table_size: int | None = None,
    actions: Mapping[str, Action] | None = None,
) -> Expr | Rule | RuleSet | DecisionTable:
    """Read a document from a file, as `loads` does.

    Args:
        source: A path (its suffix, `.json`, `.yaml`, `.yml`, `.rules` or
            `.csv`, gives the format) or an open text file, as `json.load`
            takes.
        format: The format, when the suffix does not say; required for a file
            object without a known name.
        max_size: As `loads`; a file larger than the limit is not read.
        max_nesting: As `loads`.
        max_depth: As `loads`.
        max_table_size: As `loads`.
        actions: As `loads`.

    Raises:
        ValueError: If the format is not given and cannot be told.
        FormatError: As `loads`.
    """
    if isinstance(source, str | os.PathLike):
        file = Path(source)
        chosen = format or _format_of(file)
        limit = max_size or _DEFAULT_SIZES[chosen]
        size = file.stat().st_size
        if size > limit:
            raise FormatError(
                f"{file.name} has {size} bytes, more than the limit of {limit}",
                code="limit_exceeded",
                hint="raise max_size if the file is trusted",
            )
        # Line breaks are read as written: inside a quoted CSV field they are text.
        with file.open(encoding="utf-8", newline="") as opened:
            text = opened.read()
    else:
        chosen = format or _format_of(Path(getattr(source, "name", "")))
        text = source.read()
    return loads(
        text,
        format=chosen,
        max_size=max_size,
        max_nesting=max_nesting,
        max_depth=max_depth,
        max_table_size=max_table_size,
        actions=actions,
    )


def dump(
    value: Expr | Rule | RuleSet | DecisionTable,
    target: str | os.PathLike[str] | IO[str],
    *,
    format: Format | None = None,
    omit_actions: bool = False,
) -> None:
    """Write a document to a path or an open text file, as `dumps` does.

    A path's suffix (`.json`, `.yaml`, `.yml`, `.rules` or `.csv`) gives the
    format when it is not given. A text file ends with a line break.
    """
    if isinstance(target, str | os.PathLike):
        file = Path(target)
        text = _document_text(value, format or _format_of(file), omit_actions)
        # Line breaks are written as the document has them, on every platform.
        file.write_text(text, encoding="utf-8", newline="")
    else:
        chosen = format or _format_of(Path(getattr(target, "name", "")))
        target.write(_document_text(value, chosen, omit_actions))


def _document_text(
    value: Expr | Rule | RuleSet | DecisionTable, format: Format, omit_actions: bool
) -> str:
    """The text of a file: a rule text ends with a line break, as CSV does."""
    text = dumps(value, format=format, omit_actions=omit_actions)
    return text + "\n" if format == "text" else text


def _format_of(file: Path) -> Format:
    try:
        return _SUFFIXES[file.suffix.lower()]
    except KeyError:
        raise ValueError(
            f"cannot tell the format of {file.name!r} from its suffix; pass format='json',"
            " format='yaml', format='text' or format='csv'"
        ) from None
