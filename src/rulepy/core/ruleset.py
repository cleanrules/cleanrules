"""Rule sets, hit policies and decisions (semantics: `docs/spec/rules.md`).

    rules = RuleSet([vip, bulk], hit_policy="priority")
    decision = rules.evaluate({"years": 7, "total": 1500})   # pure: no actions
    decision.output                                          # payload of the winner
    rules.run(data)                                          # evaluate, then run actions

Hit policies follow DMN decision tables:

| Policy | Selected rules | `outputs` | `output` |
|---|---|---|---|
| `FIRST` (default) | first fired | its payload | that payload or `None` |
| `PRIORITY` | highest priority (ties: order) | its payload | that payload or `None` |
| `UNIQUE` | the only one; more is an error | its payload | that payload or `None` |
| `ANY` | first fired; payloads must agree | its payload | that payload or `None` |
| `RULE_ORDER` | all fired, in rule order | all payloads | error: use `outputs` |
| `COLLECT` | all fired, in rule order | all payloads | aggregate, or error |

`COLLECT` accepts `aggregate`: `"sum"`, `"min"`, `"max"` or `"count"`, applied per
payload key over non-null values; with nothing selected the aggregate is `None`.
"""

from __future__ import annotations

import enum
from collections.abc import Iterable, Iterator, Mapping, Sequence
from dataclasses import dataclass, field
from typing import TYPE_CHECKING, Any, Generic, Literal, TypeAlias, TypeVar, get_args, overload

from rulepy.core._compat import is_numpy_bool
from rulepy.core.logic import Logic, get_logic
from rulepy.core.rule import FrozenMap, Rule
from rulepy.errors import EvaluationError, HitPolicyError, RuleDefinitionError, RuleNotFoundError

if TYPE_CHECKING:
    from rulepy.core.evaluate import NodeResult
    from rulepy.core.explain import Explanation, RuleExplanation

__all__ = ["Aggregate", "Decision", "HitPolicy", "RuleResult", "RuleSet"]

TruthT = TypeVar("TruthT")

Aggregate: TypeAlias = Literal["sum", "min", "max", "count"]
_AGGREGATES = frozenset(get_args(Aggregate))


class HitPolicy(enum.StrEnum):
    """How a rule set turns the rules that fired into a decision (see the module docs)."""

    FIRST = "first"
    PRIORITY = "priority"
    UNIQUE = "unique"
    ANY = "any"
    RULE_ORDER = "rule_order"
    COLLECT = "collect"

    @property
    def single_hit(self) -> bool:
        """Whether at most one rule is selected."""
        return self not in (HitPolicy.RULE_ORDER, HitPolicy.COLLECT)


@dataclass(frozen=True, slots=True, eq=False)
class RuleResult(Generic[TruthT]):
    """How one rule evaluated.

    Attributes:
        rule: The rule.
        truth: Its condition's truth value.
        tree: The evaluation result of its condition (values and reasons).
        fired: Whether the truth value passed the activation test (Kleene: `TRUE`).
        output: Its evaluated payload, only for rules whose payload was evaluated
            (the selected ones, and every fired rule under `ANY`); else `None`.
        output_trees: The evaluation result of each payload value (why it is null).
    """

    rule: Rule
    truth: TruthT
    tree: NodeResult
    fired: bool
    output: Mapping[str, object] | None = None
    output_trees: Mapping[str, NodeResult] = field(default_factory=FrozenMap[str, "NodeResult"])


@dataclass(frozen=True, slots=True, eq=False)
class Decision(Generic[TruthT]):
    """The outcome of evaluating a rule set on one input.

    Attributes:
        results: Every rule's result, in rule order (including rules that did not
            fire, for explanations).
        selected: The results the hit policy selected.
        hit_policy: The policy that produced it.
        aggregate: The `COLLECT` aggregate, if any.
        aggregated: The aggregate's value (`None` when nothing was selected).
        logic: The logic the conditions were evaluated with.
    """

    results: tuple[RuleResult[TruthT], ...]
    selected: tuple[RuleResult[TruthT], ...]
    hit_policy: HitPolicy
    aggregate: Aggregate | None = None
    aggregated: Mapping[str, object] | None = None
    logic: Logic[Any] | None = None

    @property
    def outputs(self) -> tuple[Mapping[str, object], ...]:
        """The payloads of the selected rules, in rule order."""
        return tuple(r.output or {} for r in self.selected)

    @property
    def output(self) -> Mapping[str, object] | None:
        """The single output: the winner's payload, or the aggregate; `None` if none.

        Raises:
            HitPolicyError: For `RULE_ORDER` and `COLLECT` without an aggregate,
                which have several outputs (use `outputs`).
        """
        if self.aggregate is not None:
            return self.aggregated
        if not self.hit_policy.single_hit:
            raise HitPolicyError(
                f"{self.hit_policy.value.upper()} decisions have several outputs",
                hint="use decision.outputs, or COLLECT with an aggregate",
            )
        return self.outputs[0] if self.selected else None

    @property
    def fired(self) -> tuple[str, ...]:
        """Names of the rules that fired, in rule order (selected or not)."""
        return tuple(r.rule.name for r in self.results if r.fired)

    def explain(self) -> Explanation[TruthT]:
        """Explain every rule: selected, fired but not selected, not fired, or unknown.

        Nothing is evaluated again: the explanation is built from the result trees
        this decision already holds.

        Returns:
            An `Explanation`. `print()` it for a readable report, or use its
            structure: `rules`, each with `status`, `requirement` (causes grouped as
            `all` / `any`), `causes` and, for unselected rules, a `note`.

        Raises:
            UnsupportedOperation: If the decision was not made with Kleene truth
                values (graded explanations arrive with phase 5).

        Example:
            >>> print(rules.evaluate({"total": 800}).explain())  # doctest: +SKIP
            FIRST decision: selected nothing
            [ ] big (not fired): F.total > 1000
                - F.total > 1000 is false (total = 800); short by 200
        """
        from rulepy.core.explain import explain  # deferred: explain imports this module

        return explain(self)

    def why_not(self, name: str) -> RuleExplanation[TruthT]:
        """Explain one rule, typically one that did not fire or was not selected.

        Args:
            name: The rule's name.

        Returns:
            Its `RuleExplanation` (see `explain`).

        Raises:
            RuleNotFoundError: If the decision has no rule with that name.
            UnsupportedOperation: As `explain`.
        """
        return self.explain().for_rule(name)


class RuleSet(Sequence[Rule]):
    """An ordered, immutable collection of uniquely named rules and how to decide.

    Args:
        rules: The rules, in evaluation and tie-breaking order.
        hit_policy: A `HitPolicy` or its name (case-insensitive); default `FIRST`.
        aggregate: For `COLLECT` only: `"sum"`, `"min"`, `"max"` or `"count"`.
        logic: The logic or its registered name; default Kleene `"boolean"`.
        strict: Raise on missing fields and type mismatches instead of `UNKNOWN`.

    Raises:
        RuleDefinitionError: On duplicate names, non-rules, or an invalid policy
            or aggregate.
    """

    __slots__ = ("_aggregate", "_by_name", "_hit_policy", "_logic", "_rules", "_strict")

    def __init__(
        self,
        rules: Iterable[Rule],
        *,
        hit_policy: HitPolicy | str = HitPolicy.FIRST,
        aggregate: Aggregate | None = None,
        logic: str | Logic[Any] = "boolean",
        strict: bool = False,
    ) -> None:
        try:
            self._rules: tuple[Rule, ...] = tuple(rules)
        except TypeError:
            raise RuleDefinitionError(
                f"RuleSet takes an iterable of rules, got {type(rules).__name__} {rules!r}"
            ) from None
        self._by_name: dict[str, Rule] = {}
        for rule in self._rules:
            if not isinstance(rule, Rule):  # pyright: ignore[reportUnnecessaryIsInstance]
                raise RuleDefinitionError(
                    f"RuleSet takes Rule objects, got {type(rule).__name__} {rule!r}"
                )
            if rule.name in self._by_name:
                raise RuleDefinitionError(
                    f"duplicate rule name {rule.name!r}",
                    rule=rule.name,
                    hint="rule names must be unique",
                )
            self._by_name[rule.name] = rule
        self._hit_policy: HitPolicy = _parse_policy(hit_policy)
        if aggregate is not None:
            if self._hit_policy is not HitPolicy.COLLECT:
                raise RuleDefinitionError(
                    f"aggregate={aggregate!r} needs the COLLECT hit policy",
                    hint="pass hit_policy='collect'",
                )
            if not isinstance(aggregate, str) or aggregate not in _AGGREGATES:  # pyright: ignore[reportUnnecessaryIsInstance]
                raise RuleDefinitionError(
                    f"unknown aggregate {aggregate!r}",
                    hint=f"use one of: {', '.join(sorted(_AGGREGATES))}",
                )
        self._aggregate: Aggregate | None = aggregate
        self._logic: Logic[Any] = get_logic(logic)
        if not isinstance(self._logic, Logic):  # pyright: ignore[reportUnnecessaryIsInstance]
            raise RuleDefinitionError(
                f"logic must be a Logic or a registered logic name, got {logic!r}",
                hint="e.g. logic='boolean'",
            )
        if type(strict) is not bool and not is_numpy_bool(strict):
            raise RuleDefinitionError(
                f"strict must be a bool, got {type(strict).__name__} {strict!r}",
                hint="pass strict=True or strict=False",
            )
        self._strict = bool(strict)

    # --- read-only configuration -----------------------------------------------------

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
    def logic(self) -> Logic[Any]:
        """The logic conditions are evaluated with."""
        return self._logic

    @property
    def strict(self) -> bool:
        """Whether missing fields and type mismatches raise."""
        return self._strict

    # --- evaluation ------------------------------------------------------------------

    def evaluate(self, data: object) -> Decision[Any]:
        """Evaluate every rule on `data` and apply the hit policy. Never runs actions.

        Args:
            data: The input: a mapping, a sequence, a record or an object.

        Returns:
            The `Decision`.

        Raises:
            HitPolicyError: If the fired rules break the policy.
            EvaluationError: In strict mode, on missing fields or type mismatches.
        """
        from rulepy.engines.sequential import decide  # deferred: the engine imports this module

        return decide(self, data)

    def run(self, data: object) -> Decision[Any]:
        """Evaluate like `evaluate`, then run the actions of the selected rules in order.

        Raises:
            ActionError: If an action raised; it carries the decision and the names
                of the actions that had already run. Later actions do not run.
            HitPolicyError: If the fired rules break the policy.
            EvaluationError: In strict mode, on missing fields or type mismatches.
        """
        from rulepy.engines.sequential import decide, execute

        decision = decide(self, data)
        execute(decision, data)
        return decision

    def evaluate_many(self, rows: Iterable[object]) -> Iterator[Decision[Any]]:
        """Lazily evaluate the rule set on each input of `rows` (never runs actions).

        Args:
            rows: Any iterable of inputs; it is consumed as decisions are requested.

        Returns:
            An iterator of decisions, one per row.

        Raises:
            EvaluationError: As `evaluate`, with `row` set to the input's position;
                immediately (not on first iteration) if `rows` is a string, a
                mapping or a DataFrame, whose iteration would not yield rows.
        """
        _check_rows(rows)
        return self._iterate(rows)

    def _iterate(self, rows: Iterable[object]) -> Iterator[Decision[Any]]:
        for index, row in enumerate(rows):
            try:
                yield self.evaluate(row)
            except EvaluationError as err:
                err.row = index
                raise

    # --- Sequence protocol and lookup by name ----------------------------------------

    def __len__(self) -> int:
        return len(self._rules)

    @overload
    def __getitem__(self, key: int) -> Rule: ...
    @overload
    def __getitem__(self, key: slice) -> Sequence[Rule]: ...
    @overload
    def __getitem__(self, key: str) -> Rule: ...
    def __getitem__(self, key: int | slice | str) -> Rule | Sequence[Rule]:
        """Return a rule by position, a slice of rules, or a rule by name.

        Raises:
            RuleNotFoundError: If no rule has that name (also a `KeyError`).
        """
        if isinstance(key, str):
            try:
                return self._by_name[key]
            except KeyError:
                raise RuleNotFoundError(key, tuple(self._by_name)) from None
        return self._rules[key]

    def get(self, name: str, default: Rule | None = None) -> Rule | None:
        """Return the rule called `name`, or `default`."""
        return self._by_name.get(name, default)

    def __contains__(self, item: object) -> bool:
        """`"vip" in rules` checks names; `rule in rules` checks identity."""
        if isinstance(item, str):
            return item in self._by_name
        return any(rule is item for rule in self._rules)

    def __iter__(self) -> Iterator[Rule]:
        return iter(self._rules)

    def __repr__(self) -> str:
        names = ", ".join(repr(r.name) for r in self._rules)
        return f"RuleSet([{names}], hit_policy={self._hit_policy.value!r})"

    def __reduce__(self) -> tuple[Any, tuple[Any, ...]]:
        # Rebuilt through the constructor, which validates again.
        options = (self._hit_policy, self._aggregate, self._logic, self._strict)
        return (_rebuild_ruleset, (self._rules, *options))


def _rebuild_ruleset(
    rules: tuple[Rule, ...],
    hit_policy: HitPolicy,
    aggregate: Aggregate | None,
    logic: Logic[Any],
    strict: bool,
) -> RuleSet:
    return RuleSet(rules, hit_policy=hit_policy, aggregate=aggregate, logic=logic, strict=strict)


def _check_rows(rows: object) -> None:
    cls = type(rows)
    if cls.__module__.split(".")[0] in ("pandas", "polars", "pyarrow") and hasattr(rows, "columns"):
        raise EvaluationError(
            f"cannot evaluate a {cls.__name__} row by row: iterating it yields column labels",
            hint="pass df.to_dict('records') (pandas) or df.iter_rows(named=True) (polars);"
            " vectorized validation comes with phase 4",
        )
    if isinstance(rows, str | bytes | Mapping):
        raise EvaluationError(
            f"evaluate_many takes an iterable of inputs, got {cls.__name__}",
            hint="for a single input use evaluate(data); for several pass a list of them",
        )


def _parse_policy(policy: HitPolicy | str) -> HitPolicy:
    if isinstance(policy, HitPolicy):
        return policy
    try:
        return HitPolicy(str(policy).lower())
    except ValueError:
        raise RuleDefinitionError(
            f"unknown hit policy {policy!r}",
            hint=f"use one of: {', '.join(p.value for p in HitPolicy)}",
        ) from None
