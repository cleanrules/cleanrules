"""A static overview of a rule set: its configuration, rules, fields and tags.

    print(rules.summary())

Nothing is evaluated: the summary reads the rules themselves. It answers "what
does this rule set need and produce?": which fields it reads (and where), which
outputs it can produce, how it decides.
"""

from __future__ import annotations

from collections.abc import Mapping
from dataclasses import dataclass
from typing import TYPE_CHECKING

from rulepy.errors import format_name, format_path

if TYPE_CHECKING:
    from rulepy.core.ruleset import RuleSet

__all__ = ["FieldUsage", "RuleSetSummary", "summarize"]

_MAX_CONDITION = 60


@dataclass(frozen=True, slots=True)
class FieldUsage:
    """Where one field is read.

    Attributes:
        path: The field's path. A path may be a prefix of another one listed
            (`customer` and `customer.age`).
        in_conditions: Names of the rules whose condition reads it, in rule order.
        in_payloads: `(rule, output key)` pairs whose payload value reads it, in
            rule order: which outputs depend on this field.
    """

    path: tuple[str | int, ...]
    in_conditions: tuple[str, ...]
    in_payloads: tuple[tuple[str, str], ...]


@dataclass(frozen=True, slots=True, eq=False)
class RuleSetSummary:
    """Overview of a rule set.

    `str(summary)` is a readable report. Its markup is ASCII; rule names, tags and
    output keys that are not plain identifiers are quoted with `repr()`, so they
    cannot contain line breaks or separators that would break the layout (they
    may contain other Unicode characters, such as accents).

    Attributes:
        ruleset: The summarized rule set.
        fields: Every field the rule set reads, in order of first appearance.
        outputs: Every payload key the rule set can produce, in order of appearance.
        tags: Each tag with the names of the rules carrying it, tags sorted.
    """

    ruleset: RuleSet
    fields: tuple[FieldUsage, ...]
    outputs: tuple[str, ...]
    tags: Mapping[str, tuple[str, ...]]

    def __str__(self) -> str:
        rs = self.ruleset
        count = f"{len(rs)} rule" + ("" if len(rs) == 1 else "s")
        config = [f"hit policy {rs.hit_policy.name}"]
        if rs.aggregate is not None:
            config.append(f"aggregate {rs.aggregate}")
        config.append(f"logic {format_name(rs.logic.name)}")
        if rs.strict:
            config.append("strict")
        lines = [f"RuleSet: {count}, {', '.join(config)}"]
        lines.extend(_rule_table(rs))
        if self.fields:
            lines.append(f"Fields read ({len(self.fields)}):")
            width = max(len(format_path(f.path)) for f in self.fields)
            for usage in self.fields:
                where: list[str] = []
                if usage.in_conditions:
                    where.append(f"conditions of {_names(usage.in_conditions)}")
                if usage.in_payloads:
                    outputs = ", ".join(
                        f"{format_name(rule)} ({format_name(key)})"
                        for rule, key in usage.in_payloads
                    )
                    where.append(f"payloads of {outputs}")
                lines.append(f"  {format_path(usage.path):<{width}}  {'; '.join(where)}")
        if self.outputs:
            lines.append(f"Outputs: {_names(self.outputs)}")
        if self.tags:
            shown = "; ".join(
                f"{format_name(tag)} ({_names(names)})" for tag, names in self.tags.items()
            )
            lines.append(f"Tags: {shown}")
        return "\n".join(lines)


def summarize(ruleset: RuleSet) -> RuleSetSummary:
    """Build the summary of `ruleset` (also available as `ruleset.summary()`)."""
    order: list[tuple[str | int, ...]] = []
    conditions: dict[tuple[str | int, ...], list[str]] = {}
    payloads: dict[tuple[str | int, ...], list[tuple[str, str]]] = {}
    outputs: list[str] = []
    tags: dict[str, list[str]] = {}
    for rule in ruleset:
        for path in rule.when.field_paths():
            _note(order, path)
            # field_paths() has no duplicates and rule names are unique.
            conditions.setdefault(path, []).append(rule.name)
        for key, value in (rule.then or {}).items():
            if key not in outputs:
                outputs.append(key)
            for path in value.field_paths():
                _note(order, path)
                payloads.setdefault(path, []).append((rule.name, key))
        for tag in sorted(rule.tags):
            tags.setdefault(tag, []).append(rule.name)
    fields = tuple(
        FieldUsage(path, tuple(conditions.get(path, ())), tuple(payloads.get(path, ())))
        for path in order
    )
    sorted_tags = {tag: tuple(tags[tag]) for tag in sorted(tags)}
    return RuleSetSummary(ruleset, fields, tuple(outputs), sorted_tags)


def _note(order: list[tuple[str | int, ...]], path: tuple[str | int, ...]) -> None:
    if path not in order:
        order.append(path)


def _names(names: tuple[str, ...]) -> str:
    return ", ".join(format_name(name) for name in names)


def _rule_table(rs: RuleSet) -> list[str]:
    if not len(rs):
        return ["  (no rules)"]
    rows = [("priority", "name", "condition", "outputs")]
    for rule in rs:
        condition = repr(rule.when)
        if len(condition) > _MAX_CONDITION:
            condition = condition[: _MAX_CONDITION - 3] + "..."
        outputs = _names(tuple(rule.then)) if rule.then else "-"
        if rule.do is not None:
            outputs += " +action"
        rows.append((str(rule.priority), format_name(rule.name), condition, outputs))
    widths = [max(len(row[i]) for row in rows) for i in range(3)]
    return [
        f"  {row[0]:>{widths[0]}}  {row[1]:<{widths[1]}}  {row[2]:<{widths[2]}}  {row[3]}".rstrip()
        for row in rows
    ]
