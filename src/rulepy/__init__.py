"""RulePy: rules engine for business rules, expert systems, data validation and ML explanations."""

from rulepy import errors
from rulepy.core.expr import F
from rulepy.errors import (
    EvaluationError,
    ExpressionError,
    RulePyError,
    RuleSyntaxError,
    UnsupportedOperation,
)

__version__ = "0.0.1.dev0"

__all__ = [
    "EvaluationError",
    "ExpressionError",
    "F",
    "RulePyError",
    "RuleSyntaxError",
    "UnsupportedOperation",
    "__version__",
    "errors",
]
