"""Sequential engine: evaluate each rule once, in order, then apply the hit policy.

This is the reference implementation for stateless business rules (semantics:
`docs/spec/rules.md`); a faster backend (Rust, phase 7) must produce the same
`Decision`. `decide` is pure; `execute` runs the selected rules' actions.
"""

from __future__ import annotations

import dataclasses
import math
import numbers
from collections.abc import Mapping
from decimal import Decimal
from typing import Any, assert_never

from rulepy.core._compat import I64_MAX, I64_MIN, is_numpy_bool
from rulepy.core.evaluate import NodeResult, evaluate_tree, evaluate_value_tree, type_group
from rulepy.core.rule import ActionContext, FrozenMap, Rule
from rulepy.core.ruleset import Decision, HitPolicy, RuleResult, RuleSet
from rulepy.errors import ActionError, HitPolicyError

__all__ = ["decide", "execute"]


def decide(ruleset: RuleSet, data: object) -> Decision[Any]:
    """Evaluate `ruleset` on `data` and apply its hit policy (see `RuleSet.evaluate`)."""
    results = [_evaluate_condition(ruleset, rule, data) for rule in ruleset.rules]
    fired = [i for i, r in enumerate(results) if r.fired]

    # Payloads are evaluated only where the policy needs them, so a discarded rule
    # can never raise in strict mode: ANY compares every fired payload, the other
    # policies only look at what they select.
    policy = ruleset.hit_policy
    if policy is HitPolicy.ANY:
        for i in fired:
            results[i] = _with_payload(ruleset, results[i], data)
    chosen = _select(ruleset, results, fired)
    for i in chosen:
        if results[i].output is None:
            results[i] = _with_payload(ruleset, results[i], data)
    if policy is HitPolicy.ANY:
        _check_any(results, fired)
    selected = tuple(results[i] for i in chosen)
    aggregated = _aggregate(ruleset, selected) if ruleset.aggregate is not None else None
    return Decision(tuple(results), selected, policy, ruleset.aggregate, aggregated, ruleset.logic)


def execute(decision: Decision[Any], data: object) -> None:
    """Run the actions of the selected rules in rule order; stop at the first failure.

    Raises:
        ActionError: With the decision and the names of the actions that had run.
    """
    executed: list[str] = []
    for result in decision.selected:
        action = result.rule.do
        if action is None:
            continue
        try:
            output = result.output if result.output is not None else FrozenMap[str, object]()
            action(ActionContext(result.rule, data, result.tree, output))
        except Exception as err:
            raise ActionError(
                f"the action failed: {type(err).__name__}: {err}",
                rule=result.rule.name,
                decision=decision,
                executed=tuple(executed),
            ) from err
        executed.append(result.rule.name)


def _evaluate_condition(ruleset: RuleSet, rule: Rule, data: object) -> RuleResult[Any]:
    tree = evaluate_tree(
        rule.when, data, logic=ruleset.logic, strict=ruleset.strict, rule=rule.name
    )
    return RuleResult(rule, tree.value, tree, ruleset.logic.is_true(tree.value))


def _with_payload(ruleset: RuleSet, result: RuleResult[Any], data: object) -> RuleResult[Any]:
    rule = result.rule
    if rule.then is None:
        return dataclasses.replace(result, output=FrozenMap[str, object]())
    # Keys are evaluated in definition order: in strict mode the first failing key raises.
    trees: dict[str, NodeResult] = {
        key: evaluate_value_tree(value, data, strict=ruleset.strict, rule=rule.name)
        for key, value in rule.then.items()
    }
    output = FrozenMap({key: tree.value for key, tree in trees.items()})
    return dataclasses.replace(result, output=output, output_trees=FrozenMap(trees))


def _select(ruleset: RuleSet, results: list[RuleResult[Any]], fired: list[int]) -> list[int]:
    match ruleset.hit_policy:
        case HitPolicy.FIRST | HitPolicy.ANY:
            return fired[:1]
        case HitPolicy.PRIORITY:
            if not fired:
                return []
            top = max(results[i].rule.priority for i in fired)
            return [next(i for i in fired if results[i].rule.priority == top)]
        case HitPolicy.UNIQUE:
            if len(fired) > 1:
                raise HitPolicyError(
                    f"{len(fired)} rules fired under the UNIQUE hit policy",
                    rules=tuple(results[i].rule.name for i in fired),
                    hint="make the conditions mutually exclusive, or use FIRST or PRIORITY",
                )
            return fired
        case HitPolicy.RULE_ORDER | HitPolicy.COLLECT:
            return fired
        case _:  # pragma: no cover - HitPolicy is a closed enum
            assert_never(ruleset.hit_policy)


def _check_any(results: list[RuleResult[Any]], fired: list[int]) -> None:
    if not fired:
        return
    first = results[fired[0]].output or {}
    if not all(_same_payload(first, results[i].output or {}) for i in fired[1:]):
        raise HitPolicyError(
            "rules fired with different outputs under the ANY hit policy",
            rules=tuple(results[i].rule.name for i in fired),
            hint="give them the same payload, or use FIRST, PRIORITY or COLLECT",
        )


def _same_payload(a: Mapping[str, object], b: Mapping[str, object]) -> bool:
    """Same keys and, per key, both null or same type group and equal values.

    Values are compared pairwise, never hashed, because a payload may carry any value
    read from the data (lists, dicts, arrays). `1` equals `1.0`; `1` is not `True`.
    """
    if a.keys() != b.keys():
        return False
    return all(_same_value(a[key], b[key]) for key in a)


def _same_value(x: object, y: object) -> bool:
    if x is None or y is None:
        return x is None and y is None
    if type_group(x) != type_group(y):
        return False
    try:
        equal: Any = x == y
    except Exception:  # values that cannot be compared are different
        return False
    return (type(equal) is bool or is_numpy_bool(equal)) and bool(equal)


def _aggregate(
    ruleset: RuleSet, selected: tuple[RuleResult[Any], ...]
) -> Mapping[str, object] | None:
    if not selected:
        return None  # as DMN: an aggregate over no matches is null
    combined: dict[str, list[Any]] = {}
    for result in selected:
        for key, value in (result.output or {}).items():
            if value is None:
                combined.setdefault(key, [])
                continue  # nulls are skipped, as in SQL aggregates
            if ruleset.aggregate != "count" and (
                isinstance(value, bool) or not isinstance(value, numbers.Real | Decimal)
            ):
                raise HitPolicyError(
                    f"cannot {ruleset.aggregate} non-numeric output {key!r}={value!r}",
                    rules=(result.rule.name,),
                    hint="aggregate only numeric payloads, or use COLLECT without aggregate",
                )
            combined.setdefault(key, []).append(value)
    return FrozenMap({key: _reduce(ruleset.aggregate, values) for key, values in combined.items()})


def _reduce(aggregate: str | None, values: list[Any]) -> object:
    """Aggregate one key's non-null values, normalized as arithmetic results are."""
    if aggregate == "count":
        return len(values)
    if not values:
        return None
    if any(isinstance(v, Decimal) for v in values):
        values = [float(v) for v in values]  # as in arithmetic: Decimal operands are float64
    if aggregate == "min":
        return min(values)
    if aggregate == "max":
        return max(values)
    return _as_arithmetic_result(sum(values))


def _as_arithmetic_result(total: object) -> object:
    """Null for an integer outside 64 bits or a NaN (e.g. inf + -inf), as in arithmetic."""
    if isinstance(total, int) and not I64_MIN <= total <= I64_MAX:
        return None
    if isinstance(total, float) and math.isnan(total):
        return None
    return total
