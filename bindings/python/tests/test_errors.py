"""The exception hierarchy of `spec/errors.md`, with its builtin bases and codes."""

import pickle

import cleanrules
import pytest
from cleanrules import errors
from cleanrules.errors import CleanRulesError

HIERARCHY = {
    "FormatError": ("format_error", ()),
    "ExpressionError": ("expression_error", (TypeError, ValueError)),
    "RuleDefinitionError": ("rule_definition_error", (ValueError,)),
    "RuleSyntaxError": ("rule_syntax_error", (ValueError,)),
    "RegistryError": ("registry_error", (LookupError,)),
    "RuleNotFoundError": ("rule_not_found", (KeyError,)),
    "EvaluationError": ("evaluation_error", ()),
    "MissingFieldError": ("missing_field", ()),
    "TypeMismatchError": ("type_mismatch", ()),
    "HitPolicyError": ("hit_policy_error", ()),
    "ActionError": ("action_error", ()),
    "UnsupportedError": ("unsupported", (NotImplementedError,)),
}


@pytest.mark.parametrize("name", sorted(HIERARCHY))
def test_classes_have_their_code_and_builtin_bases(name: str) -> None:
    code, bases = HIERARCHY[name]
    cls = getattr(errors, name)
    assert getattr(cleanrules, name) is cls
    assert issubclass(cls, CleanRulesError)
    assert all(issubclass(cls, base) for base in bases)
    assert cls("boom").code == code


def test_evaluation_errors_share_a_parent() -> None:
    for name in ("MissingFieldError", "TypeMismatchError", "HitPolicyError"):
        assert issubclass(getattr(errors, name), errors.EvaluationError)


def test_text_has_the_rule_the_row_and_the_hint() -> None:
    error = errors.MissingFieldError(
        "the field is missing", hint="check the input", rule="vip", path=["a", 0], row=3
    )
    assert str(error) == "rule 'vip': the field is missing (input 3)\nhint: check the input"
    assert error.path == ("a", 0)
    assert error.rules == ()
    assert errors.RuleNotFoundError("no rule 'x'").args == ("no rule 'x'",)
    assert str(errors.RuleNotFoundError("no rule 'x'")) == "no rule 'x'"


def test_unknown_context_fields_are_rejected() -> None:
    with pytest.raises(TypeError, match="colour"):
        errors.FormatError("bad", colour="red")


def test_errors_pickle_with_their_attributes() -> None:
    error = errors.TypeMismatchError("mismatch", hint="h", rule="r", types=["number", "string"])
    clone = pickle.loads(pickle.dumps(error))
    assert type(clone) is errors.TypeMismatchError
    assert (clone.code, clone.hint, clone.rule, clone.types) == (
        "type_mismatch",
        "h",
        "r",
        ("number", "string"),
    )
    assert str(clone) == str(error)


def test_core_errors_arrive_with_their_class_and_code() -> None:
    with pytest.raises(errors.ExpressionError) as info:
        cleanrules.F.x.is_in([])
    assert info.value.code == "expression_error"
    assert isinstance(info.value, ValueError)


def test_errors_name_the_failing_node() -> None:
    from cleanrules import F, Rule, RuleSet

    with pytest.raises(errors.ExpressionError) as built:
        _ = (F.a > 1) < F.b
    assert built.value.node.same_as(F.a > 1)
    assert str(built.value).endswith(", in `F.a > 1`")

    rules = RuleSet([Rule("r", (F.a > 1) & (F.b < 2))], strict=True)
    with pytest.raises(errors.MissingFieldError) as missing:
        rules.evaluate({"a": 5})
    assert missing.value.node.same_as(F.b)
    with pytest.raises(errors.TypeMismatchError) as mismatch:
        rules.evaluate({"a": "x"})
    assert mismatch.value.node.same_as(F.a > 1)
    clone = pickle.loads(pickle.dumps(mismatch.value))
    assert clone.node.same_as(F.a > 1)


def test_unsupported_errors_say_what_and_why() -> None:
    error = errors.UnsupportedError("no", operation="explain", reason="graded logic")
    assert (error.operation, error.reason) == ("explain", "graded logic")
