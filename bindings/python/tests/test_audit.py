"""The static audit in Python (`spec/audit.md`, task T.9a): the core audits, Python reads."""

from __future__ import annotations

import datetime as dt
import json
import pickle
from decimal import Decimal

import cleanrules
import pytest
from cleanrules import (
    Audit,
    AuditCheck,
    AuditedRule,
    AuditLimit,
    AuditLimits,
    CheckStatus,
    DecisionTable,
    Diagnostic,
    DiagnosticCode,
    F,
    HitPolicyError,
    Rule,
    RuleSet,
    Severity,
    UndecidedReason,
    UnsupportedError,
)
from cleanrules._audit import _known  # pyright: ignore[reportPrivateUsage]


def ages(hit_policy: str = "unique") -> RuleSet:
    return RuleSet(
        [
            Rule("adult", F.age >= 18, {"band": "adult"}),
            Rule("senior", F.age >= 65, {"band": "senior"}, id="AGE-2"),
        ],
        hit_policy=hit_policy,
    )


def test_an_overlap_under_unique_is_an_error_whose_witness_raises() -> None:
    rules = ages()
    report = rules.audit()
    assert isinstance(report, Audit)
    overlap, gap = report.diagnostics
    assert overlap.code is DiagnosticCode.OVERLAP and overlap.severity is Severity.ERROR
    assert overlap.rules == ("adult", "senior")
    assert "hit_policy_error" in overlap.message
    with pytest.raises(HitPolicyError) as raised:
        rules.evaluate(overlap.witness)
    assert set(raised.value.rules or ()) >= {"adult", "senior"}
    assert gap.code == "gap" and gap.severity == "warning" and gap.rules == ()
    assert rules.evaluate(gap.witness).fired == ()
    assert report.max_severity is Severity.ERROR
    assert report.rules == (AuditedRule("adult"), AuditedRule("senior", "AGE-2"))
    assert report.source == "ruleset" and report.document is None
    assert report.hit_policy == "unique" and report.logic == "boolean" and not report.strict
    assert report.aggregate is None and report.ruleset_digest is None
    assert report.limits == AuditLimits(256, 1_000_000)
    assert [c.check for c in report.checks] == list(AuditCheck)
    assert report.status(AuditCheck.OVERLAP) is CheckStatus.COMPLETE
    assert report.status("fields") is CheckStatus.NOT_APPLICABLE
    assert report.status("a check of a later version") is CheckStatus.NOT_APPLICABLE
    assert report.not_modelled == () and report.undecided == ()


def test_the_witness_is_python_data_and_the_document_json() -> None:
    rules = RuleSet(
        [
            Rule("a", (F.price > Decimal("0.5")) & (F.day >= dt.date(2026, 1, 1))),
            Rule("b", F.price > Decimal("0.7")),
        ],
        hit_policy="first",
    )
    report = rules.audit()
    overlap, gap = report.diagnostics
    assert (overlap.code, overlap.severity) == ("overlap", Severity.INFO)
    assert "a takes precedence over b" in overlap.message
    assert set(rules.evaluate(overlap.witness).fired) == {"a", "b"}
    assert rules.evaluate(gap.witness).fired == ()
    # Python data, as `evaluate` takes it.
    assert isinstance(gap.witness["price"], Decimal)
    assert isinstance(gap.witness["day"], dt.date)
    document = report.to_dict()
    assert document["version"] == cleanrules.FORMAT_VERSION
    written = document["audit"]["diagnostics"][1]["witness"]
    # The document has the JSON encoding of values.
    assert written["price"] == {"$decimal": str(gap.witness["price"])}
    assert written["day"] == {"$date": gap.witness["day"].isoformat()}
    assert json.loads(json.dumps(document)) == document
    # A fresh copy each time.
    document["audit"]["rules"].clear()
    assert report.to_dict()["audit"]["rules"] != []


def test_text_is_the_cores() -> None:
    report = ages().audit()
    text = str(report)
    assert text.splitlines()[0] == "Audit of a rule set: 2 rules, hit policy unique, logic boolean"
    assert "error    overlap  adult, senior: rules adult and senior both fire" in text
    assert '           witness: {"age":' in text
    assert text.splitlines()[-1].startswith("Checks: types complete, fields not applicable")
    assert repr(report) == "<Audit of a rule set: 2 diagnostics, max severity error>"
    # Each diagnostic's text is the core's, as the audit's text has it.
    first = str(report.diagnostics[0])
    assert first.startswith("error    overlap  adult, senior: rules adult")
    assert first.endswith("\n") and '\n           witness: {"age":' in first
    assert all(str(d) in text for d in report.diagnostics)


def test_no_findings() -> None:
    rules = RuleSet(
        [Rule("minor", F.age < 18), Rule("adult", F.age >= 18)],
        hit_policy="unique",
    )
    report = rules.audit()
    assert report.diagnostics == () and report.max_severity is None
    assert "No findings." in str(report)
    assert repr(report) == "<Audit of a rule set: 0 diagnostics>"
    statuses = {c.check: c.status for c in report.checks}
    assert statuses == {
        "types": "complete",
        "fields": "not_applicable",
        "unsatisfiable": "complete",
        "shadowed": "not_applicable",
        "overlap": "complete",
        "gap": "complete",
    }


def test_a_decision_table_names_cells_and_rows() -> None:
    table = DecisionTable(
        ["RULE", "country", "THEN rate"],
        [["es", "ES", Decimal("0.21")], ["pt", "'PT'", Decimal("0.23")]],
        hit_policy="first",
    )
    report = table.audit(ruleset_digest=True)
    assert report.source == "decision_table"
    assert report.rules == (AuditedRule("es", None, 0), AuditedRule("pt", None, 1))
    unlisted = report.diagnostics[0]
    assert unlisted.code is DiagnosticCode.UNLISTED_FIELD
    assert unlisted.pointer == "/decision_table/rows/0/inputs/0"
    assert unlisted.path == ("ES",) and unlisted.witness is None
    assert "'ES'" in unlisted.message
    assert report.status(AuditCheck.FIELDS) is CheckStatus.COMPLETE
    assert report.ruleset_digest == cleanrules.digest(table.ruleset)
    assert report.to_dict()["audit"]["ruleset_digest"] == report.ruleset_digest
    assert repr(report).startswith("<Audit of a decision table: 2 diagnostics")
    # The gap's witness evaluates with the compiled rule set.
    gap = next(d for d in report.diagnostics if d.code == "gap")
    assert table.evaluate(gap.witness).fired == ()


def test_mixed_types_give_the_field_and_its_groups() -> None:
    report = RuleSet(
        [Rule("a", F.code == "x"), Rule("b", F.code > 3)], hit_policy="rule_order"
    ).audit()
    mixed = next(d for d in report.diagnostics if d.code == "mixed_types")
    assert mixed.path == ("code",) and mixed.groups == ("number", "string")
    assert mixed.rules == ("a", "b")


def test_what_was_not_decided() -> None:
    rules = RuleSet(
        [Rule("a", F.name.matches("^x") & (F.y > 0.5)), Rule("b", F.y < 2)], hit_policy="first"
    )
    report = rules.audit()
    (test,) = report.not_modelled
    assert (test.rule, test.op, test.pointer, test.limit) == (
        "a",
        "matches",
        "/ruleset/rules/0/when/operands/0",
        None,
    )
    assert report.status("unsatisfiable") is CheckStatus.PARTIAL
    assert [(u.check, u.rules, u.reason) for u in report.undecided] == [
        (AuditCheck.UNSATISFIABLE, ("a",), UndecidedReason.NOT_MODELLED),
        (AuditCheck.OVERLAP, ("a", "b"), UndecidedReason.NOT_MODELLED),
    ]
    assert "Not modelled (1):" in str(report) and "Undecided (2):" in str(report)


def test_limits() -> None:
    rules = RuleSet([Rule("a", F.y > 1), Rule("b", F.y < 2)], hit_policy="first")
    report = rules.audit(max_steps=0)
    assert report.limits == AuditLimits(256, 0)
    assert report.diagnostics == ()
    first = report.undecided[0]
    assert first.reason is UndecidedReason.LIMIT and first.limit is AuditLimit.MAX_STEPS
    assert first.rules == () and first.remaining == 2
    many = RuleSet([Rule("a", (F.x == 1) | (F.x == 3) | (F.x == 5))]).audit(max_terms=1)
    assert many.limits.max_terms == 1
    assert any(n.limit is AuditLimit.MAX_TERMS for n in many.not_modelled)


@pytest.mark.parametrize(
    ("options", "error", "message"),
    [
        ({"max_terms": -1}, ValueError, "max_terms must be at least 0, got -1"),
        ({"max_steps": 1.5}, TypeError, "max_steps must be an int, got float"),
        ({"max_steps": True}, TypeError, "max_steps must be an int, got bool"),
        ({"ruleset_digest": 1}, TypeError, "ruleset_digest must be a bool, got int"),
    ],
)
def test_bad_options(options: dict[str, object], error: type[Exception], message: str) -> None:
    with pytest.raises(error, match=message):
        ages().audit(**options)  # pyright: ignore[reportArgumentType]
    table = DecisionTable(["RULE", "x", "THEN y"], [["r", "> 1", 1]])
    with pytest.raises(error, match=message):
        table.audit(**options)  # pyright: ignore[reportArgumentType]


def test_graded_logics_are_not_audited() -> None:
    # A rule set takes Kleene logic only until rules fire to degrees, so the
    # core's `unsupported` for another logic is raised when it is made.
    with pytest.raises(UnsupportedError):
        RuleSet([Rule("a", F.x > 1)], logic="min")


def test_codes_a_later_version_adds_are_strings() -> None:
    assert _known(DiagnosticCode, "overlap") is DiagnosticCode.OVERLAP
    assert _known(DiagnosticCode, "redundant_condition") == "redundant_condition"
    assert type(_known(AuditCheck, "regions")) is str
    later = Diagnostic("redundant_condition", Severity.INFO, ("a",), "m")
    # Made in Python, a diagnostic has no text of the core's.
    assert str(later) == repr(later)
    # Severities are closed, from the least serious, as the core orders them.
    assert list(Severity) == ["info", "warning", "error"]
    assert [code.value for code in DiagnosticCode] == [
        "incompatible_types",
        "mixed_types",
        "unlisted_field",
        "unsatisfiable",
        "shadowed",
        "overlap",
        "gap",
    ]


def test_reports_are_values() -> None:
    report = ages().audit()
    assert report.diagnostics[0] == ages().audit().diagnostics[0]
    # The witness, a dict, takes no part in the hash.
    assert hash(report.diagnostics[0]) == hash(ages().audit().diagnostics[0])
    with pytest.raises(AttributeError):
        report.diagnostics[0].code = "gap"  # pyright: ignore[reportAttributeAccessIssue]
    with pytest.raises(AttributeError, match="audits are immutable: cannot set 'diagnostics'"):
        report.diagnostics = ()  # pyright: ignore[reportAttributeAccessIssue]
    with pytest.raises(AttributeError, match="audits are immutable: cannot delete 'rules'"):
        del report.rules  # pyright: ignore[reportAttributeAccessIssue]
    summary = ages().summary()
    with pytest.raises(AttributeError, match="summaries are immutable: cannot set 'rules'"):
        summary.rules = ()  # pyright: ignore[reportAttributeAccessIssue]
    with pytest.raises(AttributeError, match="summaries are immutable: cannot delete 'tags'"):
        del summary.tags  # pyright: ignore[reportAttributeAccessIssue]
    output_only = r"an audit is output only.*json\.dumps\(report\.to_dict\(\)\)"
    with pytest.raises(TypeError, match=output_only):
        cleanrules.dumps(report)  # pyright: ignore[reportArgumentType]


def test_at_least_follows_the_cores_order() -> None:
    errors = ages().audit()
    assert errors.at_least(Severity.ERROR) and errors.at_least("info")
    first = ages("first").audit()  # an overlap (info) and a gap (warning)
    assert first.max_severity is Severity.WARNING
    assert first.at_least(Severity.WARNING) and not first.at_least(Severity.ERROR)
    none = RuleSet([Rule("minor", F.age < 18), Rule("adult", F.age >= 18)], hit_policy="unique")
    assert not none.audit().at_least(Severity.INFO)
    with pytest.raises(ValueError, match="'fatal' is not a valid Severity"):
        errors.at_least("fatal")


def test_a_limit_beyond_the_core_is_no_limit() -> None:
    report = ages().audit(max_steps=2**80, max_terms=2**70)
    assert report.limits.max_steps == report.limits.max_terms >= 2**32 - 1
    assert report.diagnostics == ages().audit().diagnostics
    with pytest.raises((TypeError, pickle.PicklingError)):
        pickle.dumps(report)
