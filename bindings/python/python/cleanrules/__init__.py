"""cleanrules: an explainable rules engine with one Rust core.

This is the Python binding of the core. It shares the specification, the JSON
format and the conformance vectors with every other language.

    from cleanrules import F

    adult = (F.customer.age >= 18) & F.country.is_in(["ES", "PT"])
"""

from cleanrules import errors
from cleanrules._cleanrules import FORMAT_VERSION, __version__
from cleanrules._documents import Format, dump, dumps, load, loads, rule_from_text
from cleanrules._expr import Closed, Const, Expr, F, Field, Kind, StructuralKey
from cleanrules._rules import (
    Action,
    ActionContext,
    Aggregate,
    Cause,
    CauseGroup,
    Counterfactual,
    Decision,
    Explanation,
    FieldUsage,
    HitPolicy,
    Note,
    Reason,
    Rule,
    RuleExplanation,
    RuleResult,
    RuleSet,
    RuleSummary,
    Status,
    Summary,
    Truth,
)
from cleanrules._tables import DecisionTable, HeaderItem, TableOutput, TablePolicy, TableRow
from cleanrules._trees import ResultTree, evaluate_tree, evaluate_value_tree
from cleanrules.errors import (
    ActionError,
    CleanRulesError,
    EvaluationError,
    ExpressionError,
    FormatError,
    HitPolicyError,
    MissingFieldError,
    RegistryError,
    RuleDefinitionError,
    RuleNotFoundError,
    RuleSyntaxError,
    TypeMismatchError,
    UnsupportedError,
)

__all__ = [
    "FORMAT_VERSION",
    "Action",
    "ActionContext",
    "ActionError",
    "Aggregate",
    "Cause",
    "CauseGroup",
    "CleanRulesError",
    "Closed",
    "Const",
    "Counterfactual",
    "Decision",
    "DecisionTable",
    "EvaluationError",
    "Explanation",
    "Expr",
    "ExpressionError",
    "F",
    "Field",
    "FieldUsage",
    "Format",
    "FormatError",
    "HeaderItem",
    "HitPolicy",
    "HitPolicyError",
    "Kind",
    "MissingFieldError",
    "Note",
    "Reason",
    "RegistryError",
    "ResultTree",
    "Rule",
    "RuleDefinitionError",
    "RuleExplanation",
    "RuleNotFoundError",
    "RuleResult",
    "RuleSet",
    "RuleSummary",
    "RuleSyntaxError",
    "Status",
    "StructuralKey",
    "Summary",
    "TableOutput",
    "TablePolicy",
    "TableRow",
    "Truth",
    "TypeMismatchError",
    "UnsupportedError",
    "__version__",
    "dump",
    "dumps",
    "errors",
    "evaluate_tree",
    "evaluate_value_tree",
    "load",
    "loads",
    "rule_from_text",
]
