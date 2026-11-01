"""Rules, rule sets, decisions, explanations and summaries.

The semantics are in `spec/rules.md` and `spec/explain.md`.

    rules = RuleSet(
        [
            Rule("vip", (F.customer.years > 5) & (F.order.total > 1000), {"discount": 0.15}),
            Rule("loyal", F.customer.years > 2, {"discount": 0.05}),
        ],
        hit_policy="first",
    )
    decision = rules.evaluate({"customer": {"years": 3}, "order": {"total": 1200}})
    decision.output  # {"discount": 0.05}
    decision.explain().for_rule("vip").status  # "not_fired"

The core evaluates and explains; this module builds the objects idiomatically
and reads the results.
"""

from __future__ import annotations

import enum
import json
from collections.abc import Callable, Iterable, Iterator, Mapping, Sequence
from dataclasses import dataclass
from types import MappingProxyType
from typing import Any, NoReturn, TypeAlias, overload

from cleanrules import _cleanrules as _native
from cleanrules._expr import Const, Expr, _wrap  # pyright: ignore[reportPrivateUsage]
from cleanrules.errors import (
    ActionError,
    CleanRulesError,
    EvaluationError,
    HitPolicyError,
    RuleDefinitionError,
    RuleNotFoundError,
)

__all__ = [
    "Action",
    "ActionContext",
    "Aggregate",
    "Cause",
    "CauseGroup",
    "Counterfactual",
    "Decision",
    "Explanation",
    "FieldUsage",
    "HitPolicy",
    "Note",
    "Reason",
    "Rule",
    "RuleExplanation",
    "RuleResult",
    "RuleSet",
    "RuleSummary",
    "Status",
    "Summary",
    "Truth",
]

I64_MIN, I64_MAX = -(2**63), 2**63 - 1


class Aggregate(enum.StrEnum):
    """How `COLLECT` combines the outputs of the rules that fired."""

    SUM = "sum"
    """The sum of the non-null values (decimals are added as floats)."""
    MIN = "min"
    """The smallest non-null value."""
    MAX = "max"
    """The largest non-null value."""
    COUNT = "count"
    """The number of non-null values."""


class HitPolicy(enum.StrEnum):
    """How a rule set turns the rules that fired into a decision (`spec/rules.md`)."""

    FIRST = "first"
    """The first rule that fired, in rule order."""
    PRIORITY = "priority"
    """The rule that fired with the highest priority; ties go to rule order."""
    UNIQUE = "unique"
    """The only rule that fired; more than one is a `HitPolicyError`."""
    ANY = "any"
    """The first rule that fired; the rules that fired must agree on their outputs."""
    RULE_ORDER = "rule_order"
    """Every rule that fired, in rule order."""
    COLLECT = "collect"
    """Every rule that fired, in rule order, optionally combined by an aggregate."""

    @property
    def single_hit(self) -> bool:
        """Whether at most one rule is selected."""
        return self not in (HitPolicy.RULE_ORDER, HitPolicy.COLLECT)


class Truth(enum.StrEnum):
    """A truth value of Kleene's three-valued logic."""

    TRUE = "true"
    FALSE = "false"
    UNKNOWN = "unknown"


class Status(enum.StrEnum):
    """What happened to a rule in a decision (`spec/explain.md`)."""

    SELECTED = "selected"
    """The rule fired and the hit policy selected it."""
    FIRED = "fired"
    """The rule fired but was not selected; its `note` says why."""
    NOT_FIRED = "not_fired"
    """The rule's condition was false."""
    UNKNOWN = "unknown"
    """The rule's condition was unknown: data was missing or of the wrong type."""


class Rule:
    """A named condition and its payload.

    Rules are immutable and compare by identity (their conditions are nodes, whose
    `==` builds conditions).

    Args:
        name: Unique within a rule set; appears in decisions and errors.
        when: The condition; the rule fires when it is true.
        then: The payload: string keys whose values are constants or value
            expressions such as `F.total * 0.1`. Its order is kept.
        priority: Used by the `PRIORITY` hit policy; higher wins.
        tags: Labels for grouping and filtering.
        description: A human-readable description.
        metadata: Free-form data (owner, ticket, version, ...).
        do: An action: a callable that `RuleSet.run` calls with an
            `ActionContext` when the rule is selected (never `evaluate`).

    Rules pickle and copy when their action does.

    Raises:
        RuleDefinitionError: If an argument is invalid.
        ExpressionError: If a payload value is not a value (a condition, NaN, ...).
    """

    __slots__ = ("_description", "_do", "_native", "_priority", "_then", "_when", "name")

    name: str
    _when: Expr
    _then: dict[str, Expr]
    _priority: int
    _description: str | None
    _native: _native.Rule
    _do: Action | None

    def __init__(
        self,
        name: str,
        when: Expr,
        then: Mapping[str, object] | None = None,
        *,
        priority: int = 0,
        tags: Iterable[str] = (),
        description: str | None = None,
        metadata: Mapping[str, object] | None = None,
        do: Action | None = None,
    ) -> None:
        if not isinstance(name, str) or not name:  # pyright: ignore[reportUnnecessaryIsInstance]
            raise RuleDefinitionError(f"a rule name must be a non-empty string, got {name!r}")
        if not isinstance(when, Expr):  # pyright: ignore[reportUnnecessaryIsInstance]
            raise RuleDefinitionError(
                f"when must be a condition node, got {type(when).__name__} {when!r}",
                rule=name,
                hint="build conditions with F, e.g. F.total > 100",
            )
        if type(priority) is not int:
            raise RuleDefinitionError(
                f"priority must be an int, got {type(priority).__name__} {priority!r}", rule=name
            )
        if not I64_MIN <= priority <= I64_MAX:
            raise RuleDefinitionError(f"priority {priority} does not fit in 64 bits", rule=name)
        if isinstance(tags, str):
            raise RuleDefinitionError(
                f"tags must be a collection of strings, not the string {tags!r}",
                rule=name,
                hint=f"use tags=[{tags!r}]",
            )
        tag_list = list(tags)
        if not all(isinstance(tag, str) for tag in tag_list):  # pyright: ignore[reportUnnecessaryIsInstance]
            raise RuleDefinitionError("tags must be strings", rule=name)
        if description is not None and not isinstance(description, str):  # pyright: ignore[reportUnnecessaryIsInstance]
            raise RuleDefinitionError(
                f"description must be a string, got {type(description).__name__}", rule=name
            )
        if do is not None and not callable(do):
            raise RuleDefinitionError(
                f"do must be a callable, got {type(do).__name__} {do!r}",
                rule=name,
                hint="pass a function that takes an ActionContext",
            )
        payload = _payload(name, then)
        frozen_metadata = _mapping(name, "metadata", metadata)
        native = _native.rule(
            name,
            when._node,  # pyright: ignore[reportPrivateUsage]
            [(key, node._node) for key, node in payload.items()],  # pyright: ignore[reportPrivateUsage]
            priority,
            tag_list,
            description,
            frozen_metadata,
        )
        object.__setattr__(self, "name", name)
        object.__setattr__(self, "_when", when)
        object.__setattr__(self, "_then", payload)
        object.__setattr__(self, "_priority", priority)
        object.__setattr__(self, "_description", description)
        object.__setattr__(self, "_native", native)
        object.__setattr__(self, "_do", do)

    def __setattr__(self, name: str, value: object) -> None:
        raise AttributeError(f"rules are immutable: cannot set {name!r}")

    @property
    def when(self) -> Expr:
        """The condition."""
        return self._when

    @property
    def then(self) -> Mapping[str, Expr]:
        """The payload, in order; empty when the rule has none."""
        return dict(self._then)

    @property
    def priority(self) -> int:
        """The priority, for the `PRIORITY` hit policy."""
        return self._priority

    @property
    def tags(self) -> tuple[str, ...]:
        """The tags, in order and without repeats."""
        return tuple(self._native.tags)

    @property
    def description(self) -> str | None:
        """The description, if any."""
        return self._description

    @property
    def metadata(self) -> Mapping[str, object]:
        """The metadata, as the core holds it (read-only)."""
        return MappingProxyType(self._native.metadata)

    @property
    def do(self) -> Action | None:
        """The action, if any."""
        return self._do

    def __reduce__(self) -> tuple[Any, tuple[Any, ...]]:
        return (
            _make_rule,
            (
                self.name,
                self._when,
                dict(self._then),
                self._priority,
                self.tags,
                self._description,
                dict(self.metadata),
                self._do,
            ),
        )

    def __repr__(self) -> str:
        then = "" if not self._then else f", {dict(self._then)!r}"
        return f"Rule({self.name!r}, {self._when!r}{then})"


# Pickles store these functions by name: keep the names.
def _make_rule(
    name: str,
    when: Expr,
    then: dict[str, Expr],
    priority: int,
    tags: tuple[str, ...],
    description: str | None,
    metadata: dict[str, object],
    do: Action | None,
) -> Rule:
    return Rule(
        name,
        when,
        then,
        priority=priority,
        tags=tags,
        description=description,
        metadata=metadata,
        do=do,
    )


def _make_ruleset(
    rules: tuple[Rule, ...],
    hit_policy: str,
    aggregate: str | None,
    logic: str,
    strict: bool,
) -> RuleSet:
    return RuleSet(rules, hit_policy=hit_policy, aggregate=aggregate, logic=logic, strict=strict)


@dataclass(frozen=True, slots=True, eq=False)
class ActionContext:
    """What an action receives (`spec/rules.md`, *Actions*).

    Attributes:
        rule: The rule whose action runs.
        data: The input, as given to `RuleSet.run`.
        result: The rule's result in the decision: its truth and its output.
        output: The rule's evaluated payload (empty when it has none).
        decision: The whole decision.
    """

    rule: Rule
    data: object
    result: RuleResult
    output: Mapping[str, object]
    decision: Decision


Action: TypeAlias = Callable[[ActionContext], object]
"""A callable that `RuleSet.run` calls when its rule is selected; its result is ignored."""


def _payload(rule: str, then: Mapping[str, object] | None) -> dict[str, Expr]:
    if then is None:
        return {}
    if not isinstance(then, Mapping):  # pyright: ignore[reportUnnecessaryIsInstance]
        raise RuleDefinitionError(
            f"then must be a mapping, got {type(then).__name__} {then!r}",
            rule=rule,
            hint='e.g. then={"discount": 0.1}',
        )
    payload: dict[str, Expr] = {}
    for key, value in then.items():
        if not isinstance(key, str):  # pyright: ignore[reportUnnecessaryIsInstance]
            raise RuleDefinitionError(f"payload keys must be strings, got {key!r}", rule=rule)
        if isinstance(value, Expr):
            payload[key] = value
            continue
        try:
            payload[key] = Const(value)
        except CleanRulesError as error:
            raise type(error)(
                f"then[{key!r}]: {error.message}", hint=error.hint, rule=rule
            ) from error
    return payload


def _mapping(rule: str, what: str, value: Mapping[str, object] | None) -> dict[str, object]:
    if value is None:
        return {}
    if not isinstance(value, Mapping):  # pyright: ignore[reportUnnecessaryIsInstance]
        raise RuleDefinitionError(
            f"{what} must be a mapping, got {type(value).__name__}", rule=rule
        )
    items = dict(value)
    if not all(isinstance(key, str) for key in items):  # pyright: ignore[reportUnnecessaryIsInstance]
        raise RuleDefinitionError(f"{what} keys must be strings", rule=rule)
    return items


class RuleSet(Sequence[Rule]):
    """Rules evaluated together under a hit policy.

    A rule set is an immutable sequence of rules, indexed by position or by name.

    Args:
        rules: The rules, in order; names must be unique.
        hit_policy: How the rules that fired become a decision (`HitPolicy` or its
            name).
        aggregate: With `COLLECT`, combine the outputs: `"sum"`, `"min"`, `"max"`
            or `"count"`.
        logic: The logic, by name; `"boolean"` is Kleene's three-valued logic.
        strict: Raise an `EvaluationError` instead of returning unknown for
            missing fields and type mismatches.

    Raises:
        RuleDefinitionError: For repeated names, an aggregate without `COLLECT`, or
            an unknown hit policy or aggregate.
        RegistryError: For an unknown logic.
    """

    __slots__ = ("_aggregate", "_by_name", "_hit_policy", "_logic", "_native", "_rules", "_strict")

    def __init__(
        self,
        rules: Iterable[Rule],
        *,
        hit_policy: HitPolicy | str = HitPolicy.FIRST,
        aggregate: Aggregate | str | None = None,
        logic: str = "boolean",
        strict: bool = False,
    ) -> None:
        try:
            items = tuple(rules)
        except TypeError:
            raise RuleDefinitionError(
                f"RuleSet takes an iterable of rules, got {type(rules).__name__} {rules!r}"
            ) from None
        for rule in items:
            if not isinstance(rule, Rule):  # pyright: ignore[reportUnnecessaryIsInstance]
                raise RuleDefinitionError(
                    f"RuleSet takes Rule objects, got {type(rule).__name__} {rule!r}"
                )
        try:
            policy = HitPolicy(str(hit_policy).lower())
        except ValueError:
            raise RuleDefinitionError(
                f"unknown hit policy {hit_policy!r}",
                hint=f"use one of: {', '.join(p.value for p in HitPolicy)}",
            ) from None
        chosen: Aggregate | None = None
        if aggregate is not None:
            try:
                chosen = Aggregate(str(aggregate).lower())
            except ValueError:
                raise RuleDefinitionError(
                    f"unknown aggregate {aggregate!r}", hint="use one of: count, max, min, sum"
                ) from None
        if type(strict) is not bool:
            raise RuleDefinitionError(f"strict must be a bool, got {strict!r}")
        self._native = _native.ruleset(
            [rule._native for rule in items],  # pyright: ignore[reportPrivateUsage]
            policy.value,
            None if chosen is None else chosen.value,
            logic,
            strict,
        )
        self._rules = items
        self._by_name = {rule.name: rule for rule in items}
        self._hit_policy = policy
        self._aggregate = chosen
        self._logic = logic
        self._strict = strict

    @property
    def rules(self) -> tuple[Rule, ...]:
        """The rules, in order."""
        return self._rules

    @property
    def hit_policy(self) -> HitPolicy:
        """The hit policy."""
        return self._hit_policy

    @property
    def aggregate(self) -> Aggregate | None:
        """The `COLLECT` aggregate, if any."""
        return self._aggregate

    @property
    def logic(self) -> str:
        """The logic's name."""
        return self._logic

    @property
    def strict(self) -> bool:
        """Whether evaluation raises instead of returning unknown."""
        return self._strict

    def evaluate(self, data: object) -> Decision:
        """Evaluate the rule set on one input.

        Args:
            data: The input: dicts, lists and scalars (other objects arrive with a
                later version of the binding).

        Returns:
            The `Decision`.

        Raises:
            EvaluationError: In strict mode, for a missing field or a type
                mismatch; or a `HitPolicyError` when the policy cannot decide.
        """
        return Decision(self, self._native.evaluate(data))

    def run(self, data: object) -> Decision:
        """Evaluate like `evaluate`, then run the actions of the selected rules.

        Actions run in rule order, each with an `ActionContext`. `evaluate` never
        runs them, so a decision can be computed any number of times.

        Returns:
            The `Decision`.

        Raises:
            ActionError: If an action raised; the original exception is its
                `__cause__`, and it carries the `decision` and the rules whose
                actions had already run (`executed`). Later actions do not run.
            EvaluationError: As `evaluate`.
        """
        decision = self.evaluate(data)
        executed: list[str] = []
        for result in decision.selected:
            action = result.rule.do
            if action is None:
                continue
            context = ActionContext(
                rule=result.rule,
                data=data,
                result=result,
                output=result.output or MappingProxyType({}),
                decision=decision,
            )
            try:
                action(context)
            except Exception as error:
                raise ActionError(
                    f"the action of rule {result.rule.name!r} raised"
                    f" {type(error).__name__}: {error}",
                    rule=result.rule.name,
                    executed=executed,
                    decision=decision,
                    hint="later actions did not run; the decision is in error.decision",
                ) from error
            executed.append(result.rule.name)
        return decision

    def evaluate_many(self, rows: Iterable[object]) -> list[Decision]:
        """Evaluate the rule set on each input of a batch, in order.

        Raises:
            EvaluationError: As `evaluate`; the error's `row` is the input's position.
        """
        _check_rows(rows)
        return [Decision(self, native) for native in self._native.evaluate_many(list(rows))]

    def summary(self) -> Summary:
        """Describe the rule set without evaluating it: rules, fields read, outputs, tags."""
        return Summary(json.loads(self._native.summary_json()))

    def __len__(self) -> int:
        return len(self._rules)

    @overload
    def __getitem__(self, key: int) -> Rule: ...
    @overload
    def __getitem__(self, key: slice) -> Sequence[Rule]: ...
    @overload
    def __getitem__(self, key: str) -> Rule: ...
    def __getitem__(self, key: int | slice | str) -> Rule | Sequence[Rule]:
        if isinstance(key, str):
            try:
                return self._by_name[key]
            except KeyError:
                raise _not_found(key, self._by_name) from None
        return self._rules[key]

    def get(self, name: str, default: Rule | None = None) -> Rule | None:
        """Return the rule with that name, or `default`."""
        return self._by_name.get(name, default)

    def __contains__(self, item: object) -> bool:
        if isinstance(item, str):
            return item in self._by_name
        return any(rule is item for rule in self._rules)

    def __iter__(self) -> Iterator[Rule]:
        return iter(self._rules)

    def __reduce__(self) -> tuple[Any, tuple[Any, ...]]:
        aggregate = None if self._aggregate is None else self._aggregate.value
        return (
            _make_ruleset,
            (self._rules, self._hit_policy.value, aggregate, self._logic, self._strict),
        )

    def __repr__(self) -> str:
        names = ", ".join(rule.name for rule in self._rules)
        return f"RuleSet([{names}], hit_policy={self._hit_policy.value!r})"


def _not_found(name: str, names: Iterable[str]) -> RuleNotFoundError:
    available = sorted(names)
    return RuleNotFoundError(
        f"no rule named {name!r}",
        name=name,
        available=available,
        hint=f"the rules are: {', '.join(available)}" if available else None,
    )


def _check_rows(rows: object) -> None:
    """Reject what is not a collection of inputs (`spec/rules.md`, *Strict mode*)."""
    cls = type(rows)
    if cls.__module__.split(".")[0] in ("pandas", "polars", "pyarrow") and hasattr(rows, "columns"):
        raise EvaluationError(
            f"cannot evaluate a {cls.__name__} row by row: iterating it yields column labels",
            hint="pass df.to_dict('records') (pandas) or df.iter_rows(named=True) (polars);"
            " vectorized validation comes with phase I",
        )
    if isinstance(rows, str | bytes | Mapping):
        raise EvaluationError(
            f"evaluate_many takes an iterable of inputs, got {cls.__name__}",
            hint="for a single input use evaluate(data); for several pass a list of them",
        )


def _frozen(output: Mapping[str, object] | None) -> Mapping[str, object] | None:
    return None if output is None else MappingProxyType(dict(output))


@dataclass(frozen=True, slots=True, eq=False)
class RuleResult:
    """One rule's result in a decision.

    Attributes:
        rule: The rule.
        fired: Whether its condition was true.
        truth: Its condition's truth value.
        output: Its evaluated payload (read-only), when the hit policy needed it;
            else `None`.
    """

    rule: Rule
    fired: bool
    truth: Truth
    output: Mapping[str, object] | None


class Decision:
    """The outcome of evaluating a rule set on one input. Immutable.

    Attributes:
        ruleset: The rule set that decided.
        results: Every rule's result, in rule order.
        selected: The results the hit policy selected, in rule order.
    """

    __slots__ = ("_native", "_output", "results", "ruleset", "selected")

    ruleset: RuleSet
    results: tuple[RuleResult, ...]
    selected: tuple[RuleResult, ...]
    _native: _native.Decision
    _output: Mapping[str, object] | HitPolicyError | None

    def __init__(self, ruleset: RuleSet, native: _native.Decision) -> None:
        results = tuple(
            RuleResult(rule, fired, Truth(truth), _frozen(output))
            for rule, (fired, truth, output) in zip(ruleset.rules, native.results(), strict=True)
        )
        try:
            output: Mapping[str, object] | HitPolicyError | None = _frozen(native.output())
        except HitPolicyError as error:
            output = error
        set_ = object.__setattr__
        set_(self, "ruleset", ruleset)
        set_(self, "_native", native)
        set_(self, "results", results)
        set_(self, "selected", tuple(results[i] for i in native.selected()))
        set_(self, "_output", output)

    def __setattr__(self, name: str, value: object) -> NoReturn:
        raise AttributeError(f"decisions are immutable: cannot set {name!r}")

    @property
    def hit_policy(self) -> HitPolicy:
        """The hit policy that decided."""
        return self.ruleset.hit_policy

    @property
    def fired(self) -> tuple[str, ...]:
        """The names of the rules that fired, in rule order (selected or not)."""
        return tuple(result.rule.name for result in self.results if result.fired)

    @property
    def outputs(self) -> tuple[Mapping[str, object], ...]:
        """The payloads of the selected rules, in rule order (read-only)."""
        return tuple(result.output or MappingProxyType({}) for result in self.selected)

    @property
    def output(self) -> Mapping[str, object] | None:
        """The single output: the winner's payload or the aggregate; `None` if none.

        Raises:
            HitPolicyError: For `RULE_ORDER`, and `COLLECT` without an aggregate,
                which have several outputs (read `outputs`).
        """
        if isinstance(self._output, HitPolicyError):
            error = self._output
            raise type(error)(error.message, hint=error.hint, code=error.code)
        return self._output

    @property
    def aggregated(self) -> Mapping[str, object] | None:
        """With `COLLECT` and an aggregate, its values; `None` when nothing was selected."""
        if self.ruleset.aggregate is None:
            return None
        return self.output

    def explain(self) -> Explanation:
        """Explain every rule: selected, fired but not selected, not fired, or unknown.

        Returns:
            An `Explanation`: each rule's status, and its requirement, whose causes
            hold the conditions, the values read and counterfactuals.
        """
        return Explanation(self._native)

    def why_not(self, name: str) -> RuleExplanation:
        """Explain one rule, typically one that did not fire or was not selected.

        Raises:
            RuleNotFoundError: If the rule set has no rule with that name.
        """
        return self.explain().for_rule(name)

    def to_dict(self) -> dict[str, Any]:
        """The decision document of the JSON format (`spec/json-format.md`, *Decisions*).

        The `at` pointers of the explanation's JSON form point into this document.
        """
        return _document("decision", json.loads(self._native.to_json()))

    def __repr__(self) -> str:
        names = ", ".join(result.rule.name for result in self.selected)
        return f"<Decision {self.hit_policy.value}: selected [{names}]>"


# --- Explanations ---------------------------------------------------------------------


@dataclass(frozen=True, slots=True)
class Reason:
    """Why a result is unknown or null (`spec/ast.md`, *Results and reasons*).

    Attributes:
        code: The reason, such as `"missing_field"` or `"type_mismatch"`.
        paths: The field paths involved.
        types: The type groups involved, for type mismatches.
        detail: Informative text, if any.
    """

    code: str
    paths: tuple[tuple[str | int, ...], ...]
    types: tuple[str, ...]
    detail: str | None


@dataclass(frozen=True, slots=True)
class Counterfactual:
    """What a comparison needed: `path op target`, and how far the value was.

    Attributes:
        path: The field compared.
        op: The comparison it needed, such as `"gt"`.
        target: The value it was compared with.
        gap: How far the value was from the target: a number, a `timedelta` for
            dates and datetimes, or `None` when there is no exact gap.
    """

    path: tuple[str | int, ...]
    op: str
    target: object
    gap: object | None


@dataclass(frozen=True, slots=True, eq=False)
class Cause:
    """A condition in a rule's requirement, with what was read (`spec/explain.md`).

    Attributes:
        node: The condition.
        truth: Its truth value.
        negated: Whether it appears under a negation, so it had to be false.
        holds: Whether it went the way the rule needed.
        values: The fields it read and their values, in order.
        reason: Why it is unknown, if it is.
        counterfactual: For a comparison that did not hold, what it needed.
    """

    node: Expr
    truth: Truth
    negated: bool
    holds: bool
    values: tuple[tuple[tuple[str | int, ...], object], ...]
    reason: Reason | None
    counterfactual: Counterfactual | None

    def __repr__(self) -> str:
        return f"<Cause {self.node!r}: {self.truth.value}>"


@dataclass(frozen=True, slots=True, eq=False)
class CauseGroup:
    """Requirements that must all hold (`kind == "all"`) or any of which suffices."""

    kind: str
    items: tuple[Cause | CauseGroup, ...]


@dataclass(frozen=True, slots=True)
class Note:
    """Why a rule that fired was not selected.

    Attributes:
        kind: `"lower_priority"`, `"same_priority"`, `"any_agrees"` or
            `"earlier_rule"`.
        winner: The rule selected instead.
        priority: This rule's priority, under `PRIORITY`.
        winner_priority: The winner's priority, under `PRIORITY`.
    """

    kind: str
    winner: str
    priority: int | None
    winner_priority: int | None


@dataclass(frozen=True, slots=True, eq=False)
class RuleExplanation:
    """The explanation of one rule.

    Attributes:
        rule: The rule's name.
        status: What happened to it.
        requirement: What it needed: a cause, or a group of them.
        note: Why it was not selected, when it fired.
        achievable: When it did not fire, whether some option could make it fire.
    """

    rule: str
    status: Status
    requirement: Cause | CauseGroup
    note: Note | None
    achievable: bool | None

    @property
    def causes(self) -> tuple[Cause, ...]:
        """The causes of the requirement, in order, without their grouping."""
        found: list[Cause] = []
        stack: list[Cause | CauseGroup] = [self.requirement]
        while stack:
            item = stack.pop()
            if isinstance(item, Cause):
                found.append(item)
            else:
                stack.extend(reversed(item.items))
        return tuple(found)


def _requirement(data: dict[str, Any]) -> Cause | CauseGroup:
    if data["kind"] != "cause":
        return CauseGroup(data["kind"], tuple(_requirement(item) for item in data["items"]))
    reason = data["reason"]
    counterfactual = data["counterfactual"]
    return Cause(
        node=_wrap(data["node"]),
        truth=Truth(data["truth"]),
        negated=data["negated"],
        holds=data["holds"],
        values=tuple((tuple(path), value) for path, value in data["values"]),
        reason=None
        if reason is None
        else Reason(reason[0], reason[1], tuple(reason[2]), reason[3]),
        counterfactual=None if counterfactual is None else Counterfactual(*counterfactual),
    )


class Explanation:
    """The explanation of a decision (`spec/explain.md`).

    Attributes:
        rules: Each rule's explanation, in rule order.
        selected: The names of the selected rules.
    """

    __slots__ = ("_native", "rules", "selected")

    rules: tuple[RuleExplanation, ...]
    selected: tuple[str, ...]

    def __init__(self, native: _native.Decision) -> None:
        self._native = native
        self.rules = tuple(
            RuleExplanation(
                rule=entry["rule"],
                status=Status(entry["status"]),
                requirement=_requirement(entry["requirement"]),
                note=None if entry["note"] is None else Note(*entry["note"]),
                achievable=entry["achievable"],
            )
            for entry in native.explanation()
        )
        self.selected = tuple(e.rule for e in self.rules if e.status is Status.SELECTED)

    def for_rule(self, name: str) -> RuleExplanation:
        """Return one rule's explanation.

        Raises:
            RuleNotFoundError: If there is no rule with that name.
        """
        for entry in self.rules:
            if entry.rule == name:
                return entry
        raise _not_found(name, (entry.rule for entry in self.rules))

    def to_dict(self) -> dict[str, Any]:
        """The explanation document of the JSON format (`spec/json-format.md`)."""
        return _document("explanation", json.loads(self._native.explanation_json()))

    def __repr__(self) -> str:
        statuses = ", ".join(f"{entry.rule}: {entry.status.value}" for entry in self.rules)
        return f"<Explanation: {statuses}>"


# --- Summaries ------------------------------------------------------------------------


@dataclass(frozen=True, slots=True)
class FieldUsage:
    """A field a rule set reads, and where.

    Attributes:
        path: The field's path.
        conditions: The rules whose conditions read it, in rule order.
        payloads: The `(rule, key)` payload values that read it, in order.
    """

    path: tuple[str | int, ...]
    conditions: tuple[str, ...]
    payloads: tuple[tuple[str, str], ...]


@dataclass(frozen=True, slots=True)
class RuleSummary:
    """A rule in a summary: its name, priority and payload keys."""

    name: str
    priority: int
    outputs: tuple[str, ...]


class Summary:
    """What a rule set reads and produces, without evaluating it (`spec/explain.md`).

    Attributes:
        hit_policy: The hit policy.
        aggregate: The `COLLECT` aggregate, if any.
        logic: The logic's name.
        strict: Whether evaluation is strict.
        rules: Each rule's name, priority and payload keys, in rule order.
        fields: Every field read, in first-appearance order, with where it is read.
        outputs: The payload keys, in first-appearance order.
        tags: Each tag with the names of its rules.
    """

    __slots__ = (
        "_data",
        "aggregate",
        "fields",
        "hit_policy",
        "logic",
        "outputs",
        "rules",
        "strict",
        "tags",
    )

    hit_policy: HitPolicy
    aggregate: Aggregate | None
    logic: str
    strict: bool
    rules: tuple[RuleSummary, ...]
    fields: tuple[FieldUsage, ...]
    outputs: tuple[str, ...]
    tags: Mapping[str, tuple[str, ...]]

    def __init__(self, data: dict[str, Any]) -> None:
        self._data = data
        self.hit_policy = HitPolicy(data["hit_policy"])
        self.aggregate = Aggregate(data["aggregate"]) if "aggregate" in data else None
        self.logic = data["logic"]
        self.strict = bool(data.get("strict", False))
        self.rules = tuple(
            RuleSummary(rule["name"], _int(rule["priority"]), tuple(rule["outputs"]))
            for rule in data["rules"]
        )
        self.fields = tuple(
            FieldUsage(
                tuple(field["path"]),
                tuple(field["conditions"]),
                tuple((use["rule"], use["key"]) for use in field["payloads"]),
            )
            for field in data["fields"]
        )
        self.outputs = tuple(data["outputs"])
        self.tags = MappingProxyType({e["tag"]: tuple(e["rules"]) for e in data["tags"]})

    def to_dict(self) -> dict[str, Any]:
        """The summary document of the JSON format (`spec/json-format.md`)."""
        return _document("summary", json.loads(json.dumps(self._data)))

    def __repr__(self) -> str:
        return f"<Summary: {len(self.rules)} rules, {len(self.fields)} fields>"


def _document(key: str, content: dict[str, Any]) -> dict[str, Any]:
    return {"version": _native.FORMAT_VERSION, key: content}


def _int(value: int | dict[str, str]) -> int:
    """An integer of the JSON format: beyond 2**53 it is written `{"$int": "..."}`."""
    return int(value["$int"]) if isinstance(value, dict) else value
