"""Exception hierarchy for RulePy.

Every error raised by the library derives from `RulePyError`, so a single
`except RulePyError` catches them all. Messages name the rule or node involved and,
when there is something useful to say, carry a `hint` on how to fix the problem:

    try:
        ruleset.evaluate(data)
    except RulePyError as err:
        print(err)  # message, context and hint
        err.hint  # the hint alone, or None

All errors pickle with their attributes intact, so they can cross process boundaries
(`multiprocessing`, joblib) when rule sets are evaluated in parallel.
"""

from __future__ import annotations

from typing import Any

__all__ = [
    "EvaluationError",
    "ExpressionError",
    "MissingFieldError",
    "RegistryError",
    "RulePyError",
    "RuleSyntaxError",
    "TypeMismatchError",
    "UnsupportedOperation",
    "format_path",
]


class RulePyError(Exception):
    """Base class for every error raised by RulePy.

    Args:
        message: What went wrong.
        hint: Optional suggestion on how to fix it, rendered on its own line.
    """

    def __init__(self, message: str, *, hint: str | None = None) -> None:
        super().__init__(message)
        self.message = message
        self.hint = hint

    def _body(self) -> str:
        """Return the message without the hint; subclasses add their context here."""
        return self.message

    def __str__(self) -> str:
        body = self._body()
        return body if self.hint is None else f"{body}\nhint: {self.hint}"

    def __reduce__(self) -> tuple[Any, tuple[Any, ...]]:
        # The default reduce re-calls __init__ with ``self.args`` only, which drops
        # keyword-only arguments; rebuild without __init__ and restore the attributes.
        return (_rebuild, (type(self), self.args, dict(self.__dict__)))


# Pickles store this function by name: keep it here and keep the name stable, or
# errors pickled by an older version can no longer be loaded.
def _rebuild(cls: type[RulePyError], args: tuple[Any, ...], state: dict[str, Any]) -> RulePyError:
    err = cls.__new__(cls, *args)
    err.__dict__.update(state)
    return err


def _with_context(body: str, rule: str | None, node: object) -> str:
    if rule is not None:
        body = f"rule {rule!r}: {body}"
    if node is not None:
        body = f"{body} in `{node}`"
    return body


class UnsupportedOperation(RulePyError, NotImplementedError):
    """A backend cannot handle an operation that the reference evaluator supports.

    Args:
        operation: The node's operator tag as serialized in `docs/spec/ast.md`
            (e.g. `"matches"`), never a class name or free text, so every backend
            reports the same string.
        backend: Backend that rejected it, e.g. `"polars"` or `"rust"`.
        reason: Optional explanation of why it is unsupported.
        rule: Name of the rule containing the operation, if known.
        node: The offending node, or its rendered text, if known.
        hint: Optional suggestion, e.g. which backend to use instead.
    """

    def __init__(
        self,
        operation: str,
        backend: str,
        *,
        reason: str | None = None,
        rule: str | None = None,
        node: object = None,
        hint: str | None = None,
    ) -> None:
        message = f"{operation!r} is not supported by the {backend} backend"
        if reason is not None:
            message = f"{message}: {reason}"
        super().__init__(message, hint=hint)
        self.operation = operation
        self.backend = backend
        self.reason = reason
        self.rule = rule
        self.node = node

    def _body(self) -> str:
        return _with_context(self.message, self.rule, self.node)


class ExpressionError(RulePyError, TypeError, ValueError):
    """A condition tree is malformed, e.g. `and` with one operand or a non-scalar constant.

    Raised when a node is built, or when a node or truth value is misused as a Python
    `bool`; never as the result of evaluating data. It is both a `TypeError`
    (wrong kind of operand) and a `ValueError` (invalid value such as an empty path),
    so generic handlers for either catch it.

    Args:
        message: What is wrong with the node.
        rule: Name of the rule being built, if known.
        node: The offending node (or its rendered text, or a JSON path), if known.
        hint: Optional suggestion on how to build it correctly.
    """

    def __init__(
        self,
        message: str,
        *,
        rule: str | None = None,
        node: object = None,
        hint: str | None = None,
    ) -> None:
        super().__init__(message, hint=hint)
        self.rule = rule
        self.node = node

    def _body(self) -> str:
        return _with_context(self.message, self.rule, self.node)


class RuleSyntaxError(RulePyError, ValueError):
    """A rule written as text, YAML or JSON could not be parsed.

    Positions are 1-based; parsers with 0-based positions (e.g. PyYAML marks) must
    add 1. Only the offending line of `source` is kept (as `text`), so the error
    stays small when pickled.

    Args:
        message: What the parser expected or found.
        source: The full text being parsed; used to extract the offending line.
        line: Line of the error.
        column: Column of the error; a caret is drawn under it.
        hint: Optional suggestion on the correct syntax.

    Raises:
        ValueError: If `line` or `column` is smaller than 1.
    """

    def __init__(
        self,
        message: str,
        *,
        source: str | None = None,
        line: int | None = None,
        column: int | None = None,
        hint: str | None = None,
    ) -> None:
        if (line is not None and line < 1) or (column is not None and column < 1):
            raise ValueError(f"positions are 1-based, got line={line}, column={column}")
        super().__init__(message, hint=hint)
        self.line = line
        self.column = column
        self.text: str | None = None
        if source is not None and line is not None:
            lines = source.splitlines()
            if line <= len(lines):
                self.text = lines[line - 1]

    def _body(self) -> str:
        if self.line is None:
            return self.message
        position = f"line {self.line}"
        if self.column is not None:
            position = f"{position}, column {self.column}"
        parts = [f"{self.message} ({position})"]
        if self.text is not None:
            parts.append(f"  {self.text}")
            if self.column is not None:
                # Reuse the line's tabs so the caret lines up however tabs render.
                pad = "".join("\t" if c == "\t" else " " for c in self.text[: self.column - 1])
                parts.append(f"  {pad}^")
        return "\n".join(parts)


class EvaluationError(RulePyError):
    """A rule failed while being evaluated, e.g. comparing incompatible types.

    Args:
        message: What went wrong.
        rule: Name of the rule being evaluated, if known.
        node: The failing condition: the AST node in the Python evaluator, or its
            rendered text in other backends. Only `str(node)` is relied upon.
        hint: Optional suggestion on how to fix the rule or the data.
    """

    def __init__(
        self,
        message: str,
        *,
        rule: str | None = None,
        node: object = None,
        hint: str | None = None,
    ) -> None:
        super().__init__(message, hint=hint)
        self.rule = rule
        self.node = node

    def _body(self) -> str:
        return _with_context(self.message, self.rule, self.node)


def format_path(path: tuple[str | int, ...]) -> str:
    """Render a field path for messages, e.g. `("orders", 0, "total")` as `orders[0].total`.

    Segments that are not plain identifiers are quoted, so the rendering is
    unambiguous: `("a.b",)` is `['a.b']` and `("x y", 0)` is `['x y'][0]`.
    """
    out = ""
    for segment in path:
        if isinstance(segment, int):
            out += f"[{segment}]"
        elif segment.isascii() and segment.isidentifier():
            out += f".{segment}" if out else segment
        else:
            out += f"[{segment!r}]"
    return out


class TypeMismatchError(EvaluationError):
    """Values of incompatible types met during evaluation, e.g. comparing `str` with `int`.

    Only raised in strict mode; by default a mismatch makes the comparison `UNKNOWN`
    and the trace records why (see `docs/spec/ast.md`).

    Args:
        message: What could not be compared or converted.
        types: The type names involved, e.g. `("str", "int")`.
        path: Path of the field that held the offending value, if any.
        rule: Name of the rule being evaluated, if known.
        node: The failing node, or its rendered text, if known.
        hint: Optional suggestion on how to fix the rule or the data.
    """

    def __init__(
        self,
        message: str,
        *,
        types: tuple[str, ...] = (),
        path: tuple[str | int, ...] | None = None,
        rule: str | None = None,
        node: object = None,
        hint: str | None = None,
    ) -> None:
        super().__init__(message, rule=rule, node=node, hint=hint)
        self.types = types
        self.path = path

    def _body(self) -> str:
        message = self.message
        if self.path is not None:
            message = f"{message} (field {format_path(self.path)})"
        return _with_context(message, self.rule, self.node)


class MissingFieldError(EvaluationError):
    """A field path could not be resolved in the input data (strict mode only).

    Args:
        path: The field path that is missing.
        rule: Name of the rule being evaluated, if known.
        node: The failing node, or its rendered text, if known.
        hint: Optional suggestion, e.g. which keys the data has.
    """

    def __init__(
        self,
        path: tuple[str | int, ...],
        *,
        rule: str | None = None,
        node: object = None,
        hint: str | None = None,
    ) -> None:
        super().__init__(f"missing field {format_path(path)}", rule=rule, node=node, hint=hint)
        self.path = path


class RegistryError(RulePyError, LookupError):
    """A name was not found in, or clashes with, a registry (logics, actions, ...).

    Args:
        message: What was looked up or registered.
        hint: Optional suggestion, usually the available names.
    """
