"""Evaluating one node and reading its result tree (`spec/ast.md`, *Evaluating one node*).

tree = evaluate_tree((F.age >= 18) & F.country.is_in(["ES"]), {"age": 20})
tree.truth  # Truth.UNKNOWN: country is missing
tree.children[1].reason.code  # "missing_field"
"""

from __future__ import annotations

import json
from dataclasses import dataclass, field
from typing import Any

from cleanrules import _cleanrules as _native
from cleanrules._expr import Expr, _wrap  # pyright: ignore[reportPrivateUsage]
from cleanrules._rules import Reason, Truth
from cleanrules.errors import ExpressionError

__all__ = ["ResultTree", "evaluate_tree", "evaluate_value_tree"]


@dataclass(frozen=True, slots=True, eq=False)
class ResultTree:
    """The result of evaluating a node and each of its children (`spec/ast.md`).

    Attributes:
        node: The node.
        truth: Its truth value, for a condition; `None` for a value.
        value: Its value, for a value node (`None` when it is null).
        reason: Why it is unknown or null, if it is.
        children: The children's results, in order.
    """

    node: Expr
    truth: Truth | None
    value: object
    reason: Reason | None
    children: tuple[ResultTree, ...]
    _json: dict[str, Any] = field(init=False, repr=False, compare=False)

    def to_dict(self) -> dict[str, Any]:
        """The result tree in the JSON format (`spec/json-format.md`, *Result trees*)."""
        return json.loads(json.dumps(self._json))

    def __repr__(self) -> str:
        outcome = self.truth.value if self.truth is not None else repr(self.value)
        return f"<ResultTree {self.node!r}: {outcome}>"


def _tree(data: dict[str, Any], encoded: dict[str, Any]) -> ResultTree:
    reason = data["reason"]
    tree = ResultTree(
        node=_wrap(data["node"]),
        truth=Truth(data["truth"]) if "truth" in data else None,
        value=data.get("value"),
        reason=None
        if reason is None
        else Reason(reason[0], reason[1], tuple(reason[2]), reason[3]),
        children=tuple(
            _tree(child, sub)
            for child, sub in zip(data["children"], encoded.get("children", []), strict=True)
        ),
    )
    object.__setattr__(tree, "_json", encoded)
    return tree


def _node(node: object, what: str) -> Expr:
    if not isinstance(node, Expr):
        raise ExpressionError(
            f"{what} takes a node, got {type(node).__name__} {node!r}",
            hint="build it with F, e.g. F.total > 100",
        )
    return node


def evaluate_tree(
    condition: Expr, data: object, *, logic: str = "boolean", strict: bool = False
) -> ResultTree:
    """Evaluate a condition on `data` and return its result tree.

    Args:
        condition: A condition, or a field or boolean constant used as one.
        data: The input, read as `RuleSet.evaluate` reads it.
        logic: The logic, by name.
        strict: Raise an `EvaluationError` instead of returning unknown.

    Raises:
        ExpressionError: If `condition` is a value node.
        EvaluationError: In strict mode, for a missing field or a type mismatch.

    Example:
        >>> tree = evaluate_tree((F.age >= 18) & F.country.is_in(["ES"]), {"age": 20})
        >>> tree.truth, tree.children[1].reason.code
        (<Truth.UNKNOWN: 'unknown'>, 'missing_field')
    """
    node = _node(condition, "evaluate_tree")
    tree, encoded = _native.evaluate_tree(node._node, data, logic, strict)  # pyright: ignore[reportPrivateUsage]
    return _tree(tree, json.loads(encoded))


def evaluate_value_tree(value: Expr, data: object, *, strict: bool = False) -> ResultTree:
    """Evaluate a value node on `data` and return its result tree.

    Example:
        >>> evaluate_value_tree(F.price * F.quantity, {"price": 2.5, "quantity": 4}).value
        10.0

    Raises:
        ExpressionError: If `value` is a condition.
        EvaluationError: In strict mode, for a missing field or a type mismatch.
    """
    node = _node(value, "evaluate_value_tree")
    tree, encoded = _native.evaluate_value_tree(node._node, data, strict)  # pyright: ignore[reportPrivateUsage]
    return _tree(tree, json.loads(encoded))
