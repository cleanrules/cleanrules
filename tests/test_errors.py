import pickle
from importlib.resources import files

import pytest

import rulepy
from rulepy.errors import (
    ActionError,
    EvaluationError,
    ExpressionError,
    HitPolicyError,
    MissingFieldError,
    RegistryError,
    RuleDefinitionError,
    RuleNotFoundError,
    RulePyError,
    RuleSyntaxError,
    TypeMismatchError,
    UnsupportedOperation,
)

SAMPLES: list[RulePyError] = [
    RulePyError("something failed", hint="try again"),
    UnsupportedOperation(
        "matches", "polars", reason="lookaround is not portable", rule="r1", node="F.a", hint="x"
    ),
    RuleSyntaxError("unexpected token", source="IF x > THEN y", line=1, column=8),
    EvaluationError("cannot compare str with int", rule="vip", node="F.total > 1000"),
    ExpressionError("'and' needs at least two operands", hint="use the operand directly"),
    TypeMismatchError("cannot compare", types=("str", "int"), path=("code",), rule="r"),
    MissingFieldError(("customer", "age"), rule="adult"),
    RegistryError("unknown logic 'x'", hint="use one of: boolean"),
    RuleDefinitionError("duplicate rule name 'a'", rule="a"),
    HitPolicyError("2 rules fired", rules=("a", "b")),
    ActionError("the action failed", rule="notify", executed=("a",)),
    RuleNotFoundError("c", ("a", "b")),
]


@pytest.mark.parametrize(
    "cls", [UnsupportedOperation, RuleSyntaxError, EvaluationError, ExpressionError]
)
def test_every_error_derives_from_base(cls: type[Exception]) -> None:
    assert issubclass(cls, RulePyError)


def test_type_mismatch_error_is_an_evaluation_error_with_types_and_path() -> None:
    err = TypeMismatchError(
        "cannot compare str with int", types=("str", "int"), path=("code",), rule="vip"
    )
    assert isinstance(err, EvaluationError)
    assert (err.types, err.path) == (("str", "int"), ("code",))
    assert str(err) == "rule 'vip': cannot compare str with int (field code)"


def test_missing_field_error_names_the_path() -> None:
    err = MissingFieldError(("orders", 0, "total"), rule="big", hint="check the input")
    assert isinstance(err, EvaluationError)
    assert err.path == ("orders", 0, "total")
    assert str(err) == "rule 'big': missing field orders[0].total\nhint: check the input"


def test_rule_errors_render_their_context() -> None:
    assert str(RuleDefinitionError("bad priority", rule="r")) == "rule 'r': bad priority"
    assert str(HitPolicyError("2 rules fired", rules=("a", "b"))) == "2 rules fired (rules: a, b)"
    assert str(HitPolicyError("nothing")) == "nothing"
    assert str(ActionError("failed", rule="n")) == "rule 'n': failed"
    assert issubclass(HitPolicyError, EvaluationError)
    assert issubclass(RuleDefinitionError, ValueError)


def test_evaluation_errors_can_name_the_batch_row() -> None:
    err = MissingFieldError(("x",), rule="r")
    assert err.row is None
    err.row = 3
    assert str(err) == "row 3: rule 'r': missing field x"
    assert pickle.loads(pickle.dumps(err)).row == 3


def test_rule_not_found_is_a_key_error_listing_names() -> None:
    err = RuleNotFoundError("c", ("a", "b"))
    assert isinstance(err, KeyError)
    assert str(err) == "no rule named 'c'\nhint: rules in this set: 'a', 'b'"
    assert str(RuleNotFoundError("c", ())).endswith("rules in this set: none")


def test_registry_error_is_a_lookup_error() -> None:
    assert issubclass(RegistryError, LookupError)


def test_expression_error_names_rule_and_node() -> None:
    err = ExpressionError("'and' needs at least two operands", rule="vip", node="$.rules[0].when")
    assert str(err) == "rule 'vip': 'and' needs at least two operands in `$.rules[0].when`"


def test_expression_error_is_also_a_type_and_value_error() -> None:
    # Malformed trees are sometimes a wrong type (an int where a node is expected) and
    # sometimes a wrong value (an empty path); generic handlers for either catch it.
    assert issubclass(ExpressionError, TypeError)
    assert issubclass(ExpressionError, ValueError)


def test_syntax_error_is_also_a_value_error() -> None:
    assert issubclass(RuleSyntaxError, ValueError)


def test_unsupported_operation_is_also_not_implemented() -> None:
    assert issubclass(UnsupportedOperation, NotImplementedError)


def test_errors_are_exported_at_top_level() -> None:
    for name in (
        "RulePyError",
        "UnsupportedOperation",
        "RuleSyntaxError",
        "EvaluationError",
        "ExpressionError",
        "TypeMismatchError",
        "MissingFieldError",
        "RegistryError",
        "RuleDefinitionError",
        "HitPolicyError",
        "ActionError",
        "RuleNotFoundError",
    ):
        assert name in rulepy.__all__
        assert getattr(rulepy, name) is getattr(rulepy.errors, name)


def test_hint_is_shown_on_its_own_line() -> None:
    err = RulePyError("bad rule", hint="use & instead of 'and'")
    assert str(err) == "bad rule\nhint: use & instead of 'and'"


def test_message_without_hint_is_unchanged() -> None:
    assert str(RulePyError("bad rule")) == "bad rule"


def test_unsupported_operation_names_operation_backend_and_reason() -> None:
    err = UnsupportedOperation("matches", "polars", reason="lookaround is not portable")
    assert (
        str(err) == "'matches' is not supported by the polars backend: lookaround is not portable"
    )
    assert (err.operation, err.backend, err.reason) == (
        "matches",
        "polars",
        "lookaround is not portable",
    )


def test_unsupported_operation_without_reason() -> None:
    assert str(UnsupportedOperation("between", "rust")) == (
        "'between' is not supported by the rust backend"
    )


def test_unsupported_operation_names_rule_and_node() -> None:
    err = UnsupportedOperation("matches", "rust", rule="email_ok", node="F.email.matches('(?=x)')")
    assert str(err) == (
        "rule 'email_ok': 'matches' is not supported by the rust backend"
        " in `F.email.matches('(?=x)')`"
    )
    assert (err.rule, err.node) == ("email_ok", "F.email.matches('(?=x)')")


def test_syntax_error_points_at_the_column() -> None:
    err = RuleSyntaxError(
        "expected a value after '>'",
        source="IF total > THEN discount = 0.1",
        line=1,
        column=12,
        hint="write a number or a field, e.g. total > 1000",
    )
    assert str(err) == (
        "expected a value after '>' (line 1, column 12)\n"
        "  IF total > THEN discount = 0.1\n"
        "             ^\n"
        "hint: write a number or a field, e.g. total > 1000"
    )


def test_syntax_error_on_a_later_line_shows_that_line() -> None:
    err = RuleSyntaxError("unknown operator '=>'", source="IF a > 1\nAND b => 2", line=2, column=7)
    assert str(err).splitlines()[1:] == ["  AND b => 2", "        ^"]


def test_syntax_error_keeps_only_the_offending_line() -> None:
    err = RuleSyntaxError("bad", source="line one\nline two\nline three", line=2, column=1)
    assert err.text == "line two"
    assert "source" not in vars(err)


def test_syntax_error_caret_follows_tabs() -> None:
    err = RuleSyntaxError("bad", source="\tIF a ?? 1", line=1, column=6)
    assert str(err).splitlines()[1:] == ["  \tIF a ?? 1", "  \t    ^"]


@pytest.mark.parametrize(("line", "column"), [(0, 1), (1, 0), (-1, None)])
def test_syntax_error_rejects_positions_that_are_not_1_based(line: int, column: int | None) -> None:
    with pytest.raises(ValueError, match="1-based"):
        RuleSyntaxError("bad", source="IF a", line=line, column=column)


def test_syntax_error_without_position_is_just_the_message() -> None:
    err = RuleSyntaxError("empty rule file", hint="add at least one rule")
    assert str(err) == "empty rule file\nhint: add at least one rule"


def test_syntax_error_without_column_shows_line_but_no_caret() -> None:
    err = RuleSyntaxError("rule has no THEN", source="IF a > 1\nAND b < 2", line=2)
    assert str(err) == "rule has no THEN (line 2)\n  AND b < 2"


def test_syntax_error_without_source_only_reports_position() -> None:
    assert str(RuleSyntaxError("unexpected end", line=3, column=1)) == (
        "unexpected end (line 3, column 1)"
    )


def test_syntax_error_with_line_out_of_range_skips_snippet() -> None:
    assert str(RuleSyntaxError("oops", source="IF a", line=5, column=1)) == (
        "oops (line 5, column 1)"
    )


def test_evaluation_error_names_rule_and_node() -> None:
    err = EvaluationError("cannot compare str with int", rule="vip", node="F.total > 1000")
    assert str(err) == "rule 'vip': cannot compare str with int in `F.total > 1000`"
    assert (err.rule, err.node) == ("vip", "F.total > 1000")


def test_evaluation_error_without_context() -> None:
    assert str(EvaluationError("missing field")) == "missing field"


@pytest.mark.parametrize("err", SAMPLES, ids=lambda e: type(e).__name__)
def test_errors_survive_pickling(err: RulePyError) -> None:
    clone = pickle.loads(pickle.dumps(err))
    assert type(clone) is type(err)
    assert str(clone) == str(err)
    assert vars(clone) == vars(err)
    assert clone.args == err.args


def test_package_ships_py_typed_marker() -> None:
    assert files("rulepy").joinpath("py.typed").is_file()


def test_rule_not_found_lists_at_most_twenty_names() -> None:
    err = RuleNotFoundError("c", tuple(f"r{i}" for i in range(30)))
    assert "'r19'" in (err.hint or "")
    assert "'r20'" not in (err.hint or "")
