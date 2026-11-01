"""The exceptions of cleanrules (`spec/errors.md`).

Every error derives from `CleanRulesError`, so one `except CleanRulesError`
catches them all. Each has a stable `code`, the same in every language, a
`message` written by the core, an optional `hint` on how to fix the problem, and
the context fields that apply (`rule`, `path`, `pointer`, ...):

    try:
        ruleset.evaluate(data)
    except CleanRulesError as err:
        print(err)  # the message, the rule and the hint
        err.code  # "missing_field", ...

The classes keep the builtin bases of the Python v1, so existing `except`
clauses still work: `ExpressionError` is also a `TypeError` and a `ValueError`,
for example. Errors pickle with their attributes, so they can cross process
boundaries.
"""

from __future__ import annotations

from typing import Any, ClassVar

__all__ = [
    "ActionError",
    "CleanRulesError",
    "EvaluationError",
    "ExpressionError",
    "FormatError",
    "HitPolicyError",
    "MissingFieldError",
    "RegistryError",
    "RuleDefinitionError",
    "RuleNotFoundError",
    "RuleSyntaxError",
    "TypeMismatchError",
    "UnsupportedError",
]

_CONTEXT_FIELDS = (
    "pointer",
    "line",
    "column",
    "rule",
    "rules",
    "path",
    "types",
    "row",
    "name",
    "available",
    "executed",
    "node",
    "operation",
    "reason",
)


class CleanRulesError(Exception):
    """Base class of every error raised by cleanrules.

    Args:
        message: What went wrong.
        hint: How to fix it, if there is something useful to say.
        code: The stable error code; defaults to the class's own code.
        **context: The context fields that apply (`spec/errors.md`, *Codes*).

    Attributes:
        code: The stable error code, such as `"missing_field"`.
        message: What went wrong, as written by the core.
        hint: How to fix it, or `None`.
        pointer: A JSON Pointer into the document read, or `None`.
        line: The 1-based line of a syntax error, or `None`.
        column: The 1-based column, in code points, or `None`.
        rule: The rule concerned, or `None`.
        rules: The rules concerned, in rule order.
        path: The full path of the field being read, or `None`.
        types: The type groups involved.
        row: The position of the input in a batch, or `None`.
        name: The name looked up, or `None`.
        available: The names that exist, sorted.
        executed: The rules whose actions had run.
        node: The failing node, as an `Expr`: the operand that broke a
            construction rule, or the innermost node whose evaluation failed.
        operation: What was not supported, or `None`.
        reason: Why it was not supported, or `None`.
    """

    # Each subclass names its code; the base has none of its own.
    default_code: ClassVar[str] = ""

    code: str
    message: str
    hint: str | None
    pointer: str | None
    line: int | None
    column: int | None
    rule: str | None
    rules: tuple[str, ...]
    path: tuple[str | int, ...] | None
    types: tuple[str, ...]
    row: int | None
    name: str | None
    available: tuple[str, ...]
    executed: tuple[str, ...]
    node: Any
    operation: str | None
    reason: str | None

    def __init__(
        self, message: str, *, hint: str | None = None, code: str | None = None, **context: Any
    ) -> None:
        unknown = set(context) - set(_CONTEXT_FIELDS)
        if unknown:
            raise TypeError(f"unknown error context fields: {', '.join(sorted(unknown))}")
        super().__init__(message)
        self.code = code or self.default_code
        self.message = message
        self.hint = hint
        for field in _CONTEXT_FIELDS:
            value = context.get(field)
            if field in ("rules", "types", "available", "executed"):
                value = tuple(value or ())
            elif field == "path" and value is not None:
                value = tuple(value)
            elif field == "node" and value is not None:
                value = _as_expr(value)
            setattr(self, field, value)

    def __str__(self) -> str:
        text = self.message
        # Core messages often name the rule already (`two rules are named "a"`).
        if self.rule is not None and f'"{self.rule}"' not in text:
            text = f"rule {self.rule!r}: {text}"
        if self.node is not None:
            text = f"{text}, in `{self.node!r}`"
        if self.row is not None:
            text = f"{text} (input {self.row})"
        return text if self.hint is None else f"{text}\nhint: {self.hint}"

    def __reduce__(self) -> tuple[Any, tuple[Any, ...]]:
        # The default reduce calls __init__ again with `args` only, which drops the
        # keyword arguments; rebuild without __init__ and restore the attributes.
        return (_rebuild, (type(self), self.args, dict(self.__dict__)))


def _as_expr(node: object) -> object:
    """A native node from the core, as the `Expr` users build."""
    from cleanrules import _cleanrules
    from cleanrules._expr import _wrap  # pyright: ignore[reportPrivateUsage]

    return _wrap(node) if isinstance(node, _cleanrules.Node) else node


# Pickles store this function by name: keep the name, or errors pickled by an
# older version can no longer be loaded.
def _rebuild(
    cls: type[CleanRulesError], args: tuple[Any, ...], state: dict[str, Any]
) -> CleanRulesError:
    error = cls.__new__(cls, *args)
    Exception.__init__(error, *args)
    error.__dict__.update(state)
    return error


class FormatError(CleanRulesError):
    """A document is invalid.

    Its codes are `format_error`, `unsupported_version`, `limit_exceeded` and
    `decision_mismatch`.
    """

    default_code = "format_error"


class ExpressionError(CleanRulesError, TypeError, ValueError):
    """A condition node breaks a construction rule (`expression_error`)."""

    default_code = "expression_error"


class RuleDefinitionError(CleanRulesError, ValueError):
    """A rule or rule set breaks a construction rule (`rule_definition_error`)."""

    default_code = "rule_definition_error"


class RuleSyntaxError(CleanRulesError, ValueError):
    """The text DSL cannot be parsed (`rule_syntax_error`)."""

    default_code = "rule_syntax_error"


class RegistryError(CleanRulesError, LookupError):
    """A name is not registered (`registry_error`)."""

    default_code = "registry_error"


class RuleNotFoundError(CleanRulesError, KeyError):
    """A rule set has no rule by that name (`rule_not_found`)."""

    default_code = "rule_not_found"

    # KeyError would show the repr of the message.
    __str__ = CleanRulesError.__str__


class EvaluationError(CleanRulesError):
    """Strict evaluation cannot go on (`evaluation_error` and its children)."""

    default_code = "evaluation_error"


class MissingFieldError(EvaluationError):
    """Strict mode met a field that cannot be resolved (`missing_field`)."""

    default_code = "missing_field"


class TypeMismatchError(EvaluationError):
    """Strict mode met values of incompatible types (`type_mismatch`)."""

    default_code = "type_mismatch"


class HitPolicyError(EvaluationError):
    """The hit policy cannot decide (`hit_policy_error`)."""

    default_code = "hit_policy_error"


class ActionError(CleanRulesError):
    """An action raised while running a decision (`action_error`)."""

    default_code = "action_error"


class UnsupportedError(CleanRulesError, NotImplementedError):
    """This build, logic or format version cannot do the operation (`unsupported`)."""

    default_code = "unsupported"
