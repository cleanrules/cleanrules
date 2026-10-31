"""Explanations: why each rule of a decision fired, was not selected, or did not fire.

    decision = rules.evaluate(data)
    print(decision.explain())                 # every rule, with its requirement
    decision.why_not("vip").requirement       # what would have to change

An explanation is built from the result trees the evaluator already keeps; nothing
is evaluated again. For each rule it gives a **requirement**: the elementary
conditions (`Cause`) behind the outcome, grouped as `all` (each one matters) or
`any` (one is enough):

- a rule that did not fire *would* fire if `all` / `any` of its causes changed;
- a rule that fired does so because `all` / `any` of its causes hold.

`not` is handled with De Morgan's laws, repeated conditions are merged, and
redundant groups are absorbed (`a and (a or b)` needs only `a`). For a rule that
did not fire, every way to fix it is checked by replaying the tree with the
changed conditions, so a fix that would break another occurrence of the same
condition is never offered; when no way remains, the explanation says the
conditions conflict. Different conditions are treated as independent, even on the
same field. Each cause names
the data values it read and, for a comparison between a field and a constant, a
`Counterfactual` (`short by 200`, `needs total <= 1000 (over by 300)`).

Explanations understand Kleene truth values (`Truth`) only; other logics raise
`UnsupportedOperation` until graded explanations arrive with phase 5.
"""

from __future__ import annotations

import datetime as dt
import math
import numbers
import re
import reprlib
from dataclasses import dataclass
from decimal import Decimal
from typing import TYPE_CHECKING, Any, Generic, Literal, TypeAlias, TypeVar

from rulepy.core.evaluate import NodeResult
from rulepy.core.expr import (
    And,
    Between,
    Compare,
    CompareOp,
    Const,
    Field,
    Not,
    Or,
    Scalar,
    format_scalar,
)
from rulepy.core.logic import Truth
from rulepy.errors import RuleNotFoundError, UnsupportedOperation, format_path

if TYPE_CHECKING:
    from rulepy.core.ruleset import Decision, RuleResult

__all__ = [
    "Cause",
    "CauseGroup",
    "Counterfactual",
    "Explanation",
    "Requirement",
    "RuleExplanation",
    "RuleStatus",
    "explain",
]

TruthT = TypeVar("TruthT")

RuleStatus: TypeAlias = Literal["selected", "fired", "not_fired", "unknown"]
"""How a rule ended up: chosen by the hit policy, fired but not chosen, false, unknown."""

_MARK = {"selected": "[x]", "fired": "[x]", "not_fired": "[ ]", "unknown": "[?]"}
_LABEL = {
    "selected": "selected",
    "fired": "fired, not selected",
    "not_fired": "not fired",
    "unknown": "unknown",
}
_WORD = {Truth.TRUE: "true", Truth.FALSE: "false", Truth.UNKNOWN: "unknown"}
_SYMBOL = {"eq": "==", "ne": "!=", "lt": "<", "le": "<=", "gt": ">", "ge": ">="}
_FLIP: dict[str, CompareOp] = {
    "lt": "gt",
    "le": "ge",
    "gt": "lt",
    "ge": "le",
    "eq": "eq",
    "ne": "ne",
}
_NEGATE: dict[str, CompareOp] = {
    "lt": "ge", "le": "gt", "gt": "le", "ge": "lt", "eq": "ne", "ne": "eq",
}  # fmt: skip
_NOT = {Truth.TRUE: Truth.FALSE, Truth.FALSE: Truth.TRUE, Truth.UNKNOWN: Truth.UNKNOWN}


@dataclass(frozen=True, slots=True)
class Counterfactual:
    """What a field would have to satisfy, and how far it is from it.

    Attributes:
        path: The field's path.
        op: The comparison the field must satisfy (`total > 1000`; under a `not`
            already negated, e.g. `total <= 1000`).
        target: The constant it is compared with.
        gap: How far the current value is from satisfying it (a number or a
            `timedelta`); `0` for a strict comparison exactly at the limit; `None`
            when no distance applies (equality).
        negated: Whether the comparison sits under a `not` in the rule.
    """

    path: tuple[str | int, ...]
    op: CompareOp
    target: Scalar
    gap: object = None
    negated: bool = False

    def __str__(self) -> str:
        needs = f"needs {format_path(self.path)} {_SYMBOL[self.op]} {format_scalar(self.target)}"
        if self.gap is None:
            return needs
        if _is_zero(self.gap):
            return f"at the limit ({needs})"
        distance = f"{'short' if self.op in ('gt', 'ge') else 'over'} by {_format_gap(self.gap)}"
        return f"{needs} ({distance})" if self.negated else distance


@dataclass(frozen=True, slots=True, eq=False)
class Cause:
    """One elementary condition behind a rule's outcome.

    Attributes:
        result: The condition's evaluation result (a comparison, a field used as a
            condition, a membership test...).
        negated: Whether it sits under an odd number of `not`s, i.e. the rule
            needs it **false**.
        values: Data values the condition read, as `(path, value)` pairs.
        hint: A counterfactual, for a comparison between one field and one
            constant that does not hold.
    """

    result: NodeResult
    negated: bool
    values: tuple[tuple[tuple[str | int, ...], object], ...]
    hint: Counterfactual | None = None

    @property
    def holds(self) -> bool:
        """Whether the condition has the value the rule needs from it."""
        return self.result.value is (Truth.FALSE if self.negated else Truth.TRUE)

    def __str__(self) -> str:
        node, value = self.result.node, self.result.value
        text = f"{node!r} is {_WORD.get(value, str(value))}"
        if self.values:
            shown = ", ".join(f"{format_path(p)} = {_format_value(v)}" for p, v in self.values)
            text = f"{text} ({shown})"
        if value is Truth.UNKNOWN and self.result.reason is not None:
            text = f"{text}: {self.result.reason}"
        elif not self.holds and self.negated:
            text = f"{text}, but the rule needs it false"
        if self.hint is not None:
            text = f"{text}; {self.hint}"
        return text


@dataclass(frozen=True, slots=True, eq=False)
class CauseGroup:
    """Several requirements combined: `all` of them matter, or `any` one is enough.

    Attributes:
        kind: `"all"` or `"any"`.
        items: The combined requirements (causes or nested groups), at least two.
    """

    kind: Literal["all", "any"]
    items: tuple[Requirement, ...]

    def causes(self) -> tuple[Cause, ...]:
        """The leaves of the group, in order."""
        return tuple(c for item in self.items for c in _leaves(item))


Requirement: TypeAlias = Cause | CauseGroup
"""A single cause, or a group of requirements."""


@dataclass(frozen=True, slots=True, eq=False)
class RuleExplanation(Generic[TruthT]):
    """Why one rule ended up as it did.

    Attributes:
        result: The rule's result in the decision.
        status: `"selected"`, `"fired"` (but not selected), `"not_fired"` or
            `"unknown"`.
        requirement: For a rule that fired, what makes it true; otherwise what
            would have to change for it to fire. Repeated occurrences of the same
            condition are treated as one; different conditions are treated as
            independent, even on the same field (`(F.x > 5) & (F.x < 3)` lists
            only the one that fails).
        note: For a rule that fired but was not selected, why.
        achievable: For a rule that did not fire, whether changing the listed
            conditions can make it fire. `False` when they conflict, e.g. the same
            condition is needed both true and false (`F.a & ~F.a`).
    """

    result: RuleResult[TruthT]
    status: RuleStatus
    requirement: Requirement
    note: str | None = None
    achievable: bool = True

    @property
    def name(self) -> str:
        """The rule's name."""
        return self.result.rule.name

    @property
    def causes(self) -> tuple[Cause, ...]:
        """Every cause in the requirement, flattened, in order."""
        return _leaves(self.requirement)

    def __str__(self) -> str:
        rule = self.result.rule
        lines = [
            f"{_MARK[self.status]} {_format_name(rule.name)} ({_LABEL[self.status]}): {rule.when!r}"
        ]
        if rule.description:
            lines.extend(f"    {line}" for line in rule.description.splitlines() or [""])
        if self.note:
            lines.append(f"    {self.note}")
        fired = self.status in ("selected", "fired")
        if not self.achievable:
            lines.append("    cannot fire by changing these conditions alone; they conflict:")
            lines.extend(_render(self.requirement, "    ", top=False, fired=fired))
        else:
            lines.extend(_render(self.requirement, "    ", top=True, fired=fired))
        return "\n".join(lines)


@dataclass(frozen=True, slots=True, eq=False)
class Explanation(Generic[TruthT]):
    """The explanation of a whole decision, one entry per rule in rule order.

    `str(explanation)` is the readable report (ASCII marks: `[x]` fired, `[ ]` not
    fired, `[?]` unknown); the attributes give the structure.
    """

    decision: Decision[TruthT]
    rules: tuple[RuleExplanation[TruthT], ...]

    def for_rule(self, name: str) -> RuleExplanation[TruthT]:
        """Return the explanation of the rule called `name`.

        Raises:
            RuleNotFoundError: If the decision has no rule with that name.
        """
        for entry in self.rules:
            if entry.name == name:
                return entry
        raise RuleNotFoundError(name, tuple(entry.name for entry in self.rules))

    def __str__(self) -> str:
        decision = self.decision
        chosen = ", ".join(repr(r.rule.name) for r in decision.selected) or "nothing"
        header = f"{decision.hit_policy.name} decision: selected {chosen}"
        return "\n".join([header, *(str(entry) for entry in self.rules)])


def explain(decision: Decision[TruthT]) -> Explanation[TruthT]:
    """Build the explanation of `decision` (also available as `decision.explain()`).

    Args:
        decision: A decision from `RuleSet.evaluate` or `RuleSet.run`.

    Returns:
        The `Explanation`, one entry per rule.

    Raises:
        UnsupportedOperation: If the decision was not made with Kleene truth
            values (`Truth`).
    """
    for result in decision.results:
        if not isinstance(result.truth, Truth):
            logic = decision.logic.name if decision.logic is not None else "this logic"
            raise UnsupportedOperation(
                "explain",
                f"{logic} logic",
                reason=f"rule {result.rule.name!r} has truth value {result.truth!r}",
                hint="explanations support Kleene truth values (Truth) until phase 5",
            )
    selected = {id(r) for r in decision.selected}
    entries = tuple(
        _explain_rule(decision, result, id(result) in selected) for result in decision.results
    )
    return Explanation(decision, entries)


# --- Requirements ----------------------------------------------------------------------


def _explain_rule(
    decision: Decision[TruthT], result: RuleResult[TruthT], selected: bool
) -> RuleExplanation[TruthT]:
    if selected:
        status: RuleStatus = "selected"
    elif result.fired:
        status = "fired"
    elif result.truth is Truth.UNKNOWN:
        status = "unknown"
    else:
        status = "not_fired"
    requirement = _simplify(_requirement(result.tree, negated=False))
    note = _selection_note(decision, result) if status == "fired" else None
    achievable = True
    if status in ("not_fired", "unknown"):
        requirement, achievable = _keep_options_that_fire(result.tree, requirement)
    return RuleExplanation(result, status, requirement, note, achievable)


_MAX_OPTIONS = 64


def _keep_options_that_fire(tree: NodeResult, requirement: Requirement) -> tuple[Requirement, bool]:
    """Drop the ways to fix a rule that would not actually make it fire.

    Each option is replayed with every occurrence of a forced condition changing
    together, so a condition that also appears under a `not` elsewhere cannot be
    promised as a fix. Very wide requirements are kept as they are.
    """
    options = _options(requirement)
    if options is None:
        return requirement, True
    valid = [option for option in options if _fires_with(tree, option)]
    if len(valid) == len(options):
        return requirement, True
    if not valid:
        return requirement, False
    parts = tuple(
        option[0] if len(option) == 1 else CauseGroup("all", tuple(option)) for option in valid
    )
    rebuilt = parts[0] if len(parts) == 1 else CauseGroup("any", parts)
    return _simplify(rebuilt), True


def _options(requirement: Requirement) -> list[tuple[Cause, ...]] | None:
    """Every minimal set of causes that satisfies `requirement`, or `None` if too many."""
    if isinstance(requirement, Cause):
        return [(requirement,)]
    parts = [_options(item) for item in requirement.items]
    if any(p is None for p in parts):
        return None
    if requirement.kind == "any":
        combined = [opt for p in parts for opt in p or []]
    else:
        combined: list[tuple[Cause, ...]] = [()]
        for p in parts:
            combined = [c + o for c in combined for o in p or []]
            if len(combined) > _MAX_OPTIONS:
                return None
    return combined if len(combined) <= _MAX_OPTIONS else None


def _fires_with(tree: NodeResult, option: tuple[Cause, ...]) -> bool:
    forced: dict[object, Truth] = {
        c.result.node.structural_key(): Truth.FALSE if c.negated else Truth.TRUE for c in option
    }
    if len(forced) < len({(c.result.node.structural_key(), c.negated) for c in option}):
        return False  # the same condition is needed both true and false
    return _replay(tree, forced) is Truth.TRUE


def _replay(result: NodeResult, forced: dict[object, Truth]) -> Truth:
    """Kleene re-combination of a result tree with some conditions forced."""
    node = result.node
    if isinstance(node, Not):
        return _NOT[_replay(result.children[0], forced)]
    if isinstance(node, And | Between):
        return min((_replay(c, forced) for c in result.children), default=Truth.TRUE)
    if isinstance(node, Or):
        return max((_replay(c, forced) for c in result.children), default=Truth.FALSE)
    value = forced.get(node.structural_key())
    return result.value if value is None else value


def _requirement(result: NodeResult, *, negated: bool) -> Requirement:
    """What decides `result`'s value, from the point of view of the rule."""
    node = result.node
    if isinstance(node, Not):
        return _requirement(result.children[0], negated=not negated)
    if not isinstance(node, And | Or | Between):
        return _cause(result, negated)
    # Under an odd number of `not`s, De Morgan swaps the connective's role.
    conjunctive = isinstance(node, And | Between) is not negated
    value = _NOT[result.value] if negated else result.value

    def effective(child: NodeResult) -> Truth:
        return _NOT[child.value] if negated else child.value

    children = result.children
    if conjunctive:
        if value is Truth.FALSE:
            # Every operand that is not true must change: fixing only the false ones
            # would leave the conjunction unknown if another operand is unknown.
            chosen, kind = [c for c in children if effective(c) is not Truth.TRUE], "all"
        elif value is Truth.UNKNOWN:  # each unknown operand must become true
            chosen, kind = [c for c in children if effective(c) is Truth.UNKNOWN], "all"
        else:  # true: every operand is needed
            chosen, kind = list(children), "all"
    elif value is Truth.TRUE:  # any true operand keeps it true
        chosen, kind = [c for c in children if effective(c) is Truth.TRUE], "any"
    elif value is Truth.UNKNOWN:  # any unknown operand becoming true is enough
        chosen, kind = [c for c in children if effective(c) is Truth.UNKNOWN], "any"
    else:  # false: any operand becoming true is enough
        chosen, kind = list(children), "any"
    items = tuple(_requirement(c, negated=negated) for c in chosen)
    return CauseGroup(kind, items)  # pyright: ignore[reportArgumentType]


def _simplify(requirement: Requirement) -> Requirement:
    """Flatten same-kind nesting, merge duplicates, absorb redundant groups."""
    if isinstance(requirement, Cause):
        return requirement
    flat: list[Requirement] = []
    for item in (_simplify(i) for i in requirement.items):
        if isinstance(item, CauseGroup) and item.kind == requirement.kind:
            flat.extend(item.items)
        else:
            flat.append(item)
    unique: list[Requirement] = []
    seen: set[object] = set()
    for item in flat:
        key = _key(item)
        if key not in seen:
            seen.add(key)
            unique.append(item)
    # Absorption: in `all`, an `any` group containing one of the `all`'s own causes is
    # already satisfied by it (a and (a or b) == a); symmetrically for `any`.
    direct = {_key(i) for i in unique if isinstance(i, Cause)}
    kept = [
        i
        for i in unique
        if not (isinstance(i, CauseGroup) and any(_key(c) in direct for c in i.items))
    ]
    if len(kept) == 1:
        return kept[0]
    return CauseGroup(requirement.kind, tuple(kept))


def _key(requirement: Requirement) -> object:
    if isinstance(requirement, Cause):
        return ("cause", requirement.result.node.structural_key(), requirement.negated)
    return (requirement.kind, frozenset(_key(i) for i in requirement.items))


def _leaves(requirement: Requirement) -> tuple[Cause, ...]:
    if isinstance(requirement, Cause):
        return (requirement,)
    return requirement.causes()


def _cause(result: NodeResult, negated: bool) -> Cause:
    needed = Truth.FALSE if negated else Truth.TRUE
    hint = _counterfactual(result, negated) if result.value is not needed else None
    return Cause(result, negated, tuple(_field_values(result)), hint)


def _field_values(result: NodeResult) -> list[tuple[tuple[str | int, ...], object]]:
    """The data values read by fields under `result`, in reading order, deduplicated."""
    found: list[tuple[tuple[str | int, ...], object]] = []
    stack = [result]
    while stack:
        current = stack.pop()
        # Value results only: a field used as a condition also has a truth result.
        is_value = not isinstance(current.value, Truth)
        if isinstance(current.node, Field) and is_value and current.reason is None:
            entry = (current.node.path, current.value)
            if all(entry[0] != seen[0] for seen in found):
                found.append(entry)
        stack.extend(reversed(current.children))
    return found


# --- Counterfactuals -------------------------------------------------------------------


def _counterfactual(result: NodeResult, negated: bool) -> Counterfactual | None:
    """For a field compared with a constant: the comparison it must satisfy."""
    node = result.node
    if not isinstance(node, Compare) or result.value is Truth.UNKNOWN:
        return None
    left, right = result.children
    if isinstance(node.left, Field) and isinstance(node.right, Const):
        field, current, op, target = node.left, left.value, node.op, node.right.value
    elif isinstance(node.right, Field) and isinstance(node.left, Const):
        field, current, op, target = node.right, right.value, _FLIP[node.op], node.left.value
    else:
        return None
    if negated:
        op = _NEGATE[op]
    if op in ("eq", "ne"):
        return Counterfactual(field.path, op, target, None, negated) if negated else None
    gap = _gap(op, current, target)
    if gap is _NO_GAP:
        # Without a distance, the needed comparison is still news under a `not`.
        return Counterfactual(field.path, op, target, None, negated) if negated else None
    return Counterfactual(field.path, op, target, gap, negated)


_NO_GAP: Any = object()


def _gap(op: str, current: object, target: object) -> object:
    """How far `current` is from satisfying `op target`, or `_NO_GAP`."""
    if not _same_kind(current, target):
        return _NO_GAP
    if not (_finite(current) and _finite(target)):
        return _NO_GAP  # infinities: no meaningful distance
    a: Any = current
    b: Any = target
    try:
        diff: object = b - a if op in ("gt", "ge") else a - b
    except ArithmeticError:  # e.g. decimal.Overflow on huge exponents
        return _NO_GAP
    return diff if _finite(diff) else _NO_GAP


def _finite(value: object) -> bool:
    if isinstance(value, Decimal):
        return value.is_finite()
    return not isinstance(value, float) or math.isfinite(value)


def _same_kind(a: object, b: object) -> bool:
    def number(x: object) -> bool:
        return type(x) is not bool and isinstance(x, numbers.Real | Decimal)

    if number(a) and number(b):
        # Decimal and int subtract exactly; Decimal and float do not mix.
        return not any(isinstance(x, Decimal) and isinstance(y, float) for x, y in ((a, b), (b, a)))
    same_dates = isinstance(a, dt.datetime) == isinstance(b, dt.datetime)
    return isinstance(a, dt.date) and isinstance(b, dt.date) and same_dates


def _is_zero(gap: object) -> bool:
    if isinstance(gap, dt.timedelta):
        return gap == dt.timedelta(0)
    return isinstance(gap, numbers.Real | Decimal) and gap == 0


def _format_gap(gap: object) -> str:
    if isinstance(gap, dt.timedelta):
        if gap.seconds == 0 and gap.microseconds == 0:
            return f"{gap.days} day" + ("" if abs(gap.days) == 1 else "s")
        return str(gap)
    if isinstance(gap, float):
        return format(gap, ".12g")  # no float noise such as 199.89999999999998
    return str(gap)


# --- Rendering -------------------------------------------------------------------------

_SHORT = reprlib.Repr()
_SHORT.maxstring = 60
_SHORT.maxother = 60
_SHORT.maxlist = _SHORT.maxtuple = _SHORT.maxdict = _SHORT.maxset = 6
_ADDRESS = re.compile(r" (object )?at 0x[0-9a-fA-F]+")


def _format_value(value: object) -> str:
    """A bounded, deterministic rendering of a data value."""
    if value is None or isinstance(value, bool | int | float | Decimal | str | dt.date):
        return _SHORT.repr(value) if isinstance(value, str) else format_scalar(value)
    return _ADDRESS.sub("", _SHORT.repr(value))


def _format_name(name: str) -> str:
    return name if name.isascii() and name.isidentifier() else repr(name)


def _render(requirement: Requirement, indent: str, *, top: bool, fired: bool) -> list[str]:
    if isinstance(requirement, Cause):
        return [f"{indent}- {requirement}"]
    if top:
        lead = "because" if fired else "would fire if"
        verb = "of these hold:" if fired else "of these changed:"
        header = f"{indent}{lead} {requirement.kind} {verb}"
    else:
        header = f"{indent}- {requirement.kind} of:"
    inner = indent if top else indent + "  "
    lines = [header]
    for item in requirement.items:
        lines.extend(_render(item, inner, top=False, fired=fired))
    return lines


def _selection_note(decision: Decision[Any], result: RuleResult[Any]) -> str:
    from rulepy.core.ruleset import HitPolicy  # deferred: ruleset imports this module

    winner = decision.selected[0] if decision.selected else None
    if winner is None:  # pragma: no cover - a fired rule with nothing selected is impossible
        return "fired, but the hit policy selected no rule"
    if decision.hit_policy is HitPolicy.PRIORITY:
        if winner.rule.priority > result.rule.priority:
            return (
                f"fired, but {winner.rule.name!r} has a higher priority"
                f" ({winner.rule.priority} > {result.rule.priority})"
            )
        return f"fired, but {winner.rule.name!r} has the same priority and comes first"
    if decision.hit_policy is HitPolicy.ANY:
        return f"fired; its output agrees with {winner.rule.name!r} (ANY reports the first)"
    return f"fired, but {decision.hit_policy.name} selected {winner.rule.name!r}, which comes first"
