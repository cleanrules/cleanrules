"""cleanrules: an explainable rules engine with one Rust core.

This is the Python binding of the core. It shares the specification, the JSON
format and the conformance vectors with every other language.

    from cleanrules import F

    adult = (F.customer.age >= 18) & F.country.is_in(["ES", "PT"])
"""

from cleanrules import errors
from cleanrules._cleanrules import FORMAT_VERSION, __version__
from cleanrules._expr import Const, Expr, F, Field
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
    "ActionError",
    "CleanRulesError",
    "Const",
    "EvaluationError",
    "Expr",
    "ExpressionError",
    "F",
    "Field",
    "FormatError",
    "HitPolicyError",
    "MissingFieldError",
    "RegistryError",
    "RuleDefinitionError",
    "RuleNotFoundError",
    "RuleSyntaxError",
    "TypeMismatchError",
    "UnsupportedError",
    "__version__",
    "errors",
]
