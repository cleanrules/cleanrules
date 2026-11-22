"""Loading and dumping conditions, rules, rule sets, decision tables and bundles.

    rules = cleanrules.load("pricing.yaml")
    text = cleanrules.dumps(rules, format="yaml")
    vip = cleanrules.rule_from_text("RULE vip IF customer.years > 5 THEN discount = 0.15")

The documents are the JSON format of `spec/json-format.md`, the contract between
languages: a rule set written here is read the same way in Rust, Java or Go.
YAML files hold the same documents (`spec/json-format.md`, *YAML*), the text
syntax (`spec/text.md`) the same model, as people write rules on paper, and CSV
holds decision tables (`spec/tables.md`), as spreadsheets write them; bundles
(`spec/bundles.md`) are JSON or YAML documents too. Reading
is strict and bounded: unknown keys, duplicate keys, malformed values and syntax
errors are errors with a JSON Pointer or a line and column, and documents larger
or deeper than the limits are refused.
"""

from __future__ import annotations

import os
from collections.abc import Mapping
from pathlib import Path
from typing import IO, TYPE_CHECKING, Literal, TypeAlias

from cleanrules import _cleanrules as _native
from cleanrules._expr import Expr, _wrap  # pyright: ignore[reportPrivateUsage]
from cleanrules._rules import Action, Decision, Rule, RuleSet
from cleanrules._tables import DecisionTable
from cleanrules.errors import ExpressionError, FormatError, UnsupportedError

if TYPE_CHECKING:
    from cleanrules._bundles import Bundle

__all__ = [
    "Format",
    "canonical_bytes",
    "digest",
    "dump",
    "dumps",
    "load",
    "loads",
    "rule_from_text",
]

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
    max_path_segments: int | None = None,
    actions: Mapping[str, Action] | None = None,
) -> Expr | Rule | RuleSet | DecisionTable | Bundle:
    """Read a condition, rule, rule set, decision table or bundle document.

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
        max_path_segments: The most segments a field path may have (256), so
            that the data read along a path stays shallow; `F` checks the
            default.
        actions: An action registry, each name with its callable: a rule or
            rule set read is bound to it (`RuleSet.bind`). Without it, rules
            keep the names of their actions, unbound: they evaluate, explain
            and write, but `run` refuses them. Decision tables and bundles
            are not bound.

    Returns:
        An `Expr` for a condition document, a `Rule`, a `RuleSet`, a
        `DecisionTable` or a `Bundle` (JSON or YAML, `spec/bundles.md`). A
        rule's action is the name the document gives it, if any, never code.

    Raises:
        FormatError: For invalid JSON, YAML or CSV, an unknown or duplicate
            key, a wrong type or an unknown version, with a JSON Pointer or a
            line and column; or a document beyond the limits. With the code
            `decision_mismatch`, a bundle whose `ruleset_digest`, or whose
            contract when this engine made it, is not that of its rules.
        RuleSyntaxError: For a text that does not follow the syntax, with the
            line and column of the first token that cannot be read.
        ExpressionError: If a condition breaks a construction rule; read from
            text, with the line and column where the node starts.
        RuleDefinitionError: If a rule or rule set breaks a construction rule.
        RegistryError: For an unknown logic, or a named action that `actions`
            lacks.
        UnsupportedError: For a case file (`spec/tests.md`), which Python does
            not load yet.
    """
    text = _text(
        text,
        max_size=max_size,
        max_nesting=max_nesting,
        max_depth=max_depth,
        max_table_size=max_table_size,
        max_path_segments=max_path_segments,
    )
    kind, content = _native.load_document(
        text, format, max_size, max_nesting, max_depth, max_table_size, max_path_segments
    )
    if kind == "condition":
        return _wrap(content)
    if kind == "rule":
        rule = _rule(content)
        return rule if actions is None else RuleSet([rule]).bind(actions)[0]
    if kind == "decision_table":
        return DecisionTable._from_native(content)  # pyright: ignore[reportPrivateUsage]
    if kind == "bundle":
        from cleanrules._bundles import Bundle

        if actions is not None:
            raise TypeError(
                "a bundle is not bound to actions: bind its rules with bundle.ruleset.bind(actions)"
            )
        return Bundle._from_native(content)  # pyright: ignore[reportPrivateUsage]
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
        id=native.id,
        priority=native.priority,
        tags=native.tags,
        description=native.description,
        metadata=native.metadata,
        do=native.action_name,
    )


def dumps(
    value: Expr | Rule | RuleSet | DecisionTable | Bundle | Decision,
    *,
    format: Format = "json",
    omit_actions: bool = False,
    ruleset_digest: bool = False,
) -> str:
    """Write a condition, rule, rule set, decision table, bundle or decision, in its canonical form.

        text = dumps(rules, format="yaml")
        record = dumps(decision, ruleset_digest=True)

    Args:
        value: A condition, a rule, a rule set, a decision table, a bundle or
            a decision.
        format: `"json"` (compact), `"yaml"` (block style), `"text"` (the
            canonical text, `IF … THEN …`) or `"csv"` (decision tables only).
            Bundles and decisions are JSON or YAML.
        omit_actions: Write rules without their actions instead of refusing.
            Actions are code, which a document cannot hold.
        ruleset_digest: Decisions only: write the rule set digest of the rule
            set that made the decision, as `Decision.to_dict` does
            (`spec/json-format.md`, *In decisions*).

    Returns:
        The document's text.

    Raises:
        ExpressionError: If `value` is a value node, not a condition.
        UnsupportedError: If a rule has an action and `omit_actions` is false;
            its `pointer` locates the rule. In text, also for a rule with
            metadata, which the text syntax cannot hold, for a decision
            table, a bundle and a decision.
        UnsupportedError: Also for CSV and anything but a decision table.
        TypeError: For `ruleset_digest` with anything but a decision.
    """
    from cleanrules._bundles import Bundle

    if isinstance(value, Decision):
        return value._native.dump(format, ruleset_digest)  # pyright: ignore[reportPrivateUsage]
    if ruleset_digest:
        raise TypeError(f"ruleset_digest is an option of decisions, not of {type(value).__name__}")
    if isinstance(value, DecisionTable):
        return _native.dump_table(value._native, format)  # pyright: ignore[reportPrivateUsage]
    if isinstance(value, Bundle):
        return _native.dump_bundle(value._native, format)  # pyright: ignore[reportPrivateUsage]
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
        "dumps takes a condition, a rule, a rule set, a decision table, a bundle or a decision,"
        f" not {type(value).__name__}"
    )


def canonical_bytes(
    value: Expr | Rule | RuleSet | DecisionTable | Bundle, *, omit_actions: bool = False
) -> bytes:
    """The canonical bytes of a condition, rule, rule set, decision table or bundle.

    They are the UTF-8 JSON text of the document in its canonical form, every
    object's members sorted by key (by Unicode code point), without whitespace,
    with one layout for each number (`spec/json-format.md`, *Canonical bytes*).
    The same document written with another layout, member order or format has
    the same bytes.

        canonical_bytes(rule_from_text("RULE r IF x > 1"))
        # b'{"rule":{"name":"r","when":{"left":...,"op":"gt",...}},"version":1}'

    Args:
        value: A condition, a rule, a rule set, a decision table or a bundle.
        omit_actions: Give the bytes of the document written without the
            rules' actions given as code, instead of refusing, as `dumps`.

    Returns:
        The canonical bytes.

    Raises:
        ExpressionError: If `value` is a value node, not a condition.
        UnsupportedError: If a rule has an action given as code and
            `omit_actions` is false, as `dumps` does.
    """
    return _native.canonical_bytes(_native_document(value, "canonical_bytes", omit_actions))


def digest(
    value: Expr | Rule | RuleSet | DecisionTable | Bundle, *, omit_actions: bool = False
) -> str:
    """The digest of a condition, rule, rule set, decision table or bundle.

    The SHA-256 hash of its canonical bytes, written `sha256:` and 64 lowercase
    hexadecimal digits (`spec/json-format.md`, *Digests*). A digest identifies
    one concrete canonical artifact: it does not say that two rule sets decide
    alike (`x > 5` and `5 < x`, or the decimals `1.0` and `1.00`, give different
    digests). The digest of a rule set is its rule set digest, which a decision
    can carry; a decision table's rule set digest is `digest(table.ruleset)`,
    and `digest(table)` that of the table's own document. A bundle's digest
    identifies the whole package (`spec/bundles.md`, *Digest of a bundle*);
    its rule set digest is `bundle.ruleset_digest`.

    Args:
        value: A condition, a rule, a rule set, a decision table or a bundle.
        omit_actions: As in `canonical_bytes`.

    Returns:
        The digest, such as `"sha256:5a0e…f312"`.

    Raises:
        ExpressionError: If `value` is a value node, not a condition.
        UnsupportedError: If a rule has an action given as code and
            `omit_actions` is false.
    """
    return _native.digest(_native_document(value, "digest", omit_actions))


def _native_document(
    value: Expr | Rule | RuleSet | DecisionTable | Bundle, function: str, omit_actions: bool
) -> _native.Node | _native.Rule | _native.RuleSet | _native.DecisionTable | _native.Bundle:
    """The native object whose document `value` is, checked as `dumps` checks it."""
    from cleanrules._bundles import Bundle

    if isinstance(value, DecisionTable | Bundle):
        return value._native  # pyright: ignore[reportPrivateUsage]
    if isinstance(value, Expr):
        if value.kind == "value":
            raise ExpressionError(
                f"only a condition can be a document, not the value {value!r}",
                hint="compare it first, e.g. F.x > 0",
            )
        return value._node  # pyright: ignore[reportPrivateUsage]
    if isinstance(value, Rule):
        _check_actions([value], "/rule", omit_actions)
        return value._native  # pyright: ignore[reportPrivateUsage]
    if isinstance(value, RuleSet):  # pyright: ignore[reportUnnecessaryIsInstance]
        _check_actions(list(value), "/ruleset/rules/{}", omit_actions)
        return value._native  # pyright: ignore[reportPrivateUsage]
    raise TypeError(
        f"{function} takes a condition, a rule, a rule set, a decision table or a bundle,"
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
    max_path_segments: int | None = None,
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
        max_path_segments: As `loads`.

    Raises:
        RuleSyntaxError: If the text does not follow the syntax, or holds a
            condition or a rule set instead of one rule.
        ExpressionError: As `loads`.
        RuleDefinitionError: As `loads`.
        RegistryError: As `loads`.
    """
    if do is not None and actions is not None:
        raise TypeError("rule_from_text takes do or actions, not both")
    text = _text(text, max_size=max_size, max_depth=max_depth, max_path_segments=max_path_segments)
    rule = _rule(_native.load_rule(text, max_size, max_depth, max_path_segments))
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
        id=rule.id,
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
    max_path_segments: int | None = None,
    actions: Mapping[str, Action] | None = None,
) -> Expr | Rule | RuleSet | DecisionTable | Bundle:
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
        max_path_segments: As `loads`.
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
        max_path_segments=max_path_segments,
        actions=actions,
    )


def dump(
    value: Expr | Rule | RuleSet | DecisionTable | Bundle | Decision,
    target: str | os.PathLike[str] | IO[str],
    *,
    format: Format | None = None,
    omit_actions: bool = False,
    ruleset_digest: bool = False,
) -> None:
    """Write a document to a path or an open text file, as `dumps` does.

    A path's suffix (`.json`, `.yaml`, `.yml`, `.rules` or `.csv`) gives the
    format when it is not given. A text file ends with a line break.
    """
    if isinstance(target, str | os.PathLike):
        file = Path(target)
        text = _document_text(value, format or _format_of(file), omit_actions, ruleset_digest)
        # Line breaks are written as the document has them, on every platform.
        file.write_text(text, encoding="utf-8", newline="")
    else:
        chosen = format or _format_of(Path(getattr(target, "name", "")))
        target.write(_document_text(value, chosen, omit_actions, ruleset_digest))


def _document_text(
    value: Expr | Rule | RuleSet | DecisionTable | Bundle | Decision,
    format: Format,
    omit_actions: bool,
    ruleset_digest: bool,
) -> str:
    """The text of a file: a rule text ends with a line break, as CSV does."""
    text = dumps(value, format=format, omit_actions=omit_actions, ruleset_digest=ruleset_digest)
    return text + "\n" if format == "text" else text


def _format_of(file: Path) -> Format:
    try:
        return _SUFFIXES[file.suffix.lower()]
    except KeyError:
        raise ValueError(
            f"cannot tell the format of {file.name!r} from its suffix; pass format='json',"
            " format='yaml', format='text' or format='csv'"
        ) from None
