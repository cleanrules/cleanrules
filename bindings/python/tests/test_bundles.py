"""Bundles: reproducible packages of rules (`spec/bundles.md`, task R.5)."""

import hashlib
import json
import pickle
from decimal import Decimal
from pathlib import Path
from typing import Any

import cleanrules
import pytest
from cleanrules import (
    Bundle,
    BundleComparison,
    BundleDifference,
    BundleEngine,
    BundleLimits,
    BundleRelation,
    CaseFileRef,
    DecisionTable,
    F,
    FormatError,
    Rule,
    RuleSet,
    UnsupportedError,
)

CASES = '[{"input": {"years": 9}, "expect": {"discount": "0.15"}}]'


def rules(threshold: int = 5) -> RuleSet:
    return RuleSet(
        [
            Rule("vip", F.years > threshold, {"discount": Decimal("0.15")}),
            Rule("loyal", F.years > 2, {"discount": Decimal("0.05")}),
        ]
    )


def table() -> DecisionTable:
    return DecisionTable(["RULE", "years", "THEN discount"], [["vip", "> 5", 0.15]])


def bundle(**overrides: Any) -> Bundle:
    arguments: dict[str, Any] = {
        "id": "acme/discounts",
        "revision": "2026-11-16.1",
        "cases": [("cases/discounts.json", CASES)],
    }
    arguments.update(overrides)
    return Bundle(arguments.pop("rules", rules()), **arguments)


def edited(source: Bundle, change: dict[str, Any]) -> Bundle:
    """A bundle read from `source`'s document with its engine changed."""
    document = json.loads(cleanrules.dumps(source))
    document["bundle"]["engine"].update(change)
    read = cleanrules.loads(json.dumps(document))
    assert isinstance(read, Bundle)
    return read


# --- Making ------------------------------------------------------------------------------


def test_a_bundle_records_rules_engine_contract_and_cases() -> None:
    made = bundle()
    assert made.id == "acme/discounts"
    assert made.revision == "2026-11-16.1"
    assert isinstance(made.rules, RuleSet)
    assert made.ruleset is made.rules
    assert [rule.name for rule in made.ruleset] == ["vip", "loyal"]
    assert made.ruleset_digest == cleanrules.digest(rules())
    assert made.engine == BundleEngine(
        "cleanrules",
        cleanrules.__version__,
        BundleLimits(
            max_size=16 << 20,
            max_nesting=1024,
            max_depth=256,
            max_path_segments=256,
            max_table_size=2_097_152,
            max_values=1_000_000,
        ),
        True,
    )
    assert list(made.engine.limits.to_dict()) == [
        "max_size",
        "max_nesting",
        "max_depth",
        "max_path_segments",
        "max_table_size",
        "max_values",
    ]
    assert made.contract["input"] == rules().input_schema()
    assert made.contract["output"] == {"keys": ["discount"]}
    assert made.cases == (
        CaseFileRef(
            "cases/discounts.json",
            "sha256:" + hashlib.sha256(CASES.encode()).hexdigest(),
        ),
    )


def test_making_is_reproducible() -> None:
    first, second = bundle(), bundle()
    assert first == second and hash(first) == hash(second)
    assert cleanrules.canonical_bytes(first) == cleanrules.canonical_bytes(second)
    assert cleanrules.digest(first) == cleanrules.digest(second)
    assert cleanrules.digest(first) != first.ruleset_digest


def test_a_table_bundle_gives_its_compiled_rule_set() -> None:
    made = bundle(rules=table())
    assert isinstance(made.rules, DecisionTable)
    assert made.ruleset_digest == cleanrules.digest(table().ruleset)
    assert made.ruleset.evaluate({"years": 9}).output == {"discount": 0.15}
    assert "decision_table" in json.loads(cleanrules.dumps(made))["bundle"]


def test_limits_are_recorded() -> None:
    made = bundle(max_depth=64, max_values=10)
    assert made.engine.limits.max_depth == 64
    assert made.engine.limits.max_values == 10
    assert made.engine.limits.max_size == 16 << 20


def test_cases_as_a_mapping_and_bytes() -> None:
    made = bundle(cases={"b.json": b"[]", "a/c.json": "x"})
    assert [case.path for case in made.cases] == ["b.json", "a/c.json"]
    assert bundle(cases=()).cases == ()
    assert "cases" not in json.loads(cleanrules.dumps(bundle(cases=())))["bundle"]


def test_case_matches() -> None:
    made = bundle()
    assert made.case_digest_matches("cases/discounts.json", CASES)
    assert made.case_digest_matches("cases/discounts.json", CASES.encode())
    assert not made.case_digest_matches("cases/discounts.json", CASES + " ")
    assert made.cases[0].digest_matches(CASES)
    assert not made.cases[0].digest_matches(b"")
    with pytest.raises(
        KeyError,
        match=r'no case file "cases/other.json"; its case files are "cases/discounts.json"',
    ):
        made.case_digest_matches("cases/other.json", CASES)
    with pytest.raises(KeyError, match="it has none"):
        bundle(cases=()).case_digest_matches("a.json", CASES)
    assert made.case_digest_matches(path="cases/discounts.json", content=CASES)
    assert made.cases[0].digest_matches(content=CASES)


def test_case_files_are_checked_when_made() -> None:
    digest = "sha256:" + "0" * 64
    assert CaseFileRef("cases/a.json", digest).path == "cases/a.json"
    with pytest.raises(FormatError):
        CaseFileRef("Cases/A.json", digest)
    with pytest.raises(FormatError):
        CaseFileRef("cases/a.json", "sha256:00")
    with pytest.raises(TypeError):
        CaseFileRef(b"a.json", digest)  # pyright: ignore[reportArgumentType]


def test_actions_given_as_code_are_refused_unless_omitted() -> None:
    def notify(context: cleanrules.ActionContext) -> None:
        del context

    with_code = RuleSet([Rule("a", F.x > 1), Rule("b", F.x > 2, do=notify)])
    with pytest.raises(UnsupportedError) as raised:
        Bundle(with_code, id="acme/a", revision="1")
    assert raised.value.pointer == "/bundle/ruleset/rules/1/do"
    assert raised.value.rule == "b"
    made = Bundle(with_code, id="acme/a", revision="1", omit_actions=True)
    assert made.ruleset_digest == cleanrules.digest(with_code, omit_actions=True)
    assert made.ruleset["b"].do is None


def test_named_actions_are_kept_as_names() -> None:
    named = RuleSet([Rule("a", F.x > 1, do="notify")]).bind({"notify": lambda _: None})
    made = Bundle(named, id="acme/a", revision="1")
    assert made.ruleset["a"].action_name == "notify"
    # Unbound, as `loads` gives it: bind it to run it.
    assert made.ruleset["a"].do is None


@pytest.mark.parametrize(
    ("overrides", "error"),
    [
        ({"id": "Acme/discounts"}, FormatError),
        ({"id": "/acme"}, FormatError),
        ({"revision": " 1"}, FormatError),
        ({"revision": ""}, FormatError),
        ({"cases": [("../a.json", "")]}, FormatError),
        ({"cases": [("a.json", ""), ("a.json", "x")]}, FormatError),
        ({"max_depth": 1 << 53}, UnsupportedError),
        ({"max_depth": 0}, UnsupportedError),
        ({"max_depth": -1}, ValueError),
        ({"cases": [("Cases/a.json", "")]}, FormatError),
        ({"rules": Rule("a", F.x > 1)}, TypeError),
        ({"id": 3}, TypeError),
        ({"cases": "a.json"}, TypeError),
        ({"cases": [("a.json", 3)]}, TypeError),
        ({"cases": [("a.json",)]}, TypeError),
    ],
)
def test_making_errors(overrides: dict[str, Any], error: type[Exception]) -> None:
    with pytest.raises(error):
        bundle(**overrides)


def test_format_errors_have_their_code() -> None:
    with pytest.raises(FormatError) as raised:
        bundle(id="acme//discounts")
    assert raised.value.code == "format_error"


# --- Reading and writing -----------------------------------------------------------------


@pytest.mark.parametrize("format", ["json", "yaml"])
def test_round_trip(format: cleanrules.Format) -> None:
    made = bundle(rules=table())
    back = cleanrules.loads(cleanrules.dumps(made, format=format), format=format)
    assert isinstance(back, Bundle)
    assert back == made
    assert back.engine.is_current
    assert cleanrules.dumps(back) == cleanrules.dumps(made)


def test_files(tmp_path: Path) -> None:
    made = bundle()
    for name in ("b.json", "b.yaml"):
        cleanrules.dump(made, tmp_path / name)
        assert cleanrules.load(tmp_path / name) == made


def test_text_and_csv_cannot_hold_a_bundle() -> None:
    for format in ("text", "csv"):
        with pytest.raises(UnsupportedError):
            cleanrules.dumps(bundle(), format=format)  # pyright: ignore[reportArgumentType]


def test_reading_verifies_the_rule_set_digest() -> None:
    document = json.loads(cleanrules.dumps(bundle()))
    document["bundle"]["ruleset_digest"] = cleanrules.digest(rules(9))
    with pytest.raises(FormatError) as raised:
        cleanrules.loads(json.dumps(document))
    assert raised.value.code == "decision_mismatch"
    assert raised.value.pointer == "/bundle/ruleset_digest"


def test_reading_verifies_the_contract_of_this_engine_only() -> None:
    document = json.loads(cleanrules.dumps(bundle()))
    document["bundle"]["contract"]["output"]["keys"] = ["discount", "label"]
    with pytest.raises(FormatError) as raised:
        cleanrules.loads(json.dumps(document))
    assert raised.value.code == "decision_mismatch"
    assert raised.value.pointer == "/bundle/contract/output"
    document["bundle"]["engine"]["version"] = "1.0.0-other.1"
    other = cleanrules.loads(json.dumps(document))
    assert isinstance(other, Bundle)
    assert not other.engine.is_current
    assert other.contract["output"]["keys"] == ["discount", "label"]


def test_bundles_are_immutable_and_pickle() -> None:
    made = bundle()
    with pytest.raises(AttributeError):
        made.id = "acme/other"  # pyright: ignore[reportAttributeAccessIssue]
    contract = made.contract
    contract["output"]["keys"].append("x")
    assert made.contract["output"]["keys"] == ["discount"]
    with pytest.raises(AttributeError):
        made.engine.limits.max_depth = 1  # pyright: ignore[reportAttributeAccessIssue]
    assert pickle.loads(pickle.dumps(made)) == made
    # A bundle pickles under the limits it records, which its rules may need.
    path = {"op": "field", "path": ["k"] * 300}
    condition = {
        "version": 1,
        "condition": {"op": "gt", "left": path, "right": {"op": "const", "value": 1}},
    }
    when = cleanrules.loads(json.dumps(condition), max_path_segments=300)
    assert isinstance(when, cleanrules.Expr)
    deep = Bundle(
        RuleSet([Rule("deep", when)]),
        id="acme/deep",
        revision="1",
        max_path_segments=300,
    )
    assert pickle.loads(pickle.dumps(deep)) == deep
    assert hash(made.engine) == hash(bundle().engine)
    assert made != rules()
    assert repr(made) == (
        f"<Bundle 'acme/discounts' revision '2026-11-16.1': ruleset, 1 case file,"
        f" cleanrules {cleanrules.__version__}>"
    )


# --- Comparing ---------------------------------------------------------------------------


def test_identical() -> None:
    comparison = bundle().compare(bundle())
    assert comparison == BundleComparison(BundleRelation.SAME_RULES_SAME_ENGINE, True, ())
    assert comparison.to_dict() == {
        "relation": "same_rules_same_engine",
        "identical": True,
        "differences": [],
    }


def test_same_rules_same_engine_other_cases() -> None:
    comparison = bundle().compare(bundle(cases=(), revision="2"))
    assert comparison.relation == "same_rules_same_engine"
    assert comparison.differences == ("revision", "cases")
    assert not comparison.identical


def test_same_rules_other_engine() -> None:
    made = bundle()
    by_limits = made.compare(bundle(max_depth=100))
    assert by_limits.relation is BundleRelation.SAME_RULES_OTHER_ENGINE
    assert by_limits.differences == ("engine.limits.max_depth",)
    assert by_limits.differences == (BundleDifference.ENGINE_LIMITS_MAX_DEPTH,)
    assert all(isinstance(d, BundleDifference) for d in by_limits.differences)
    by_version = made.compare(edited(made, {"version": "1.0.0-other.1"}))
    assert by_version.relation is BundleRelation.SAME_RULES_OTHER_ENGINE
    assert by_version.differences == ("engine.version",)


def test_other_rules_same_engine() -> None:
    comparison = bundle().compare(bundle(rules=rules(9), revision="2"))
    assert comparison.relation is BundleRelation.OTHER_RULES_SAME_ENGINE
    # The rules read the same fields: the input contract stays the same.
    assert comparison.differences == ("revision", "rules")
    labelled = RuleSet([*rules(), Rule("new", F.age > 1, {"label": "x"})])
    wider = bundle().compare(bundle(rules=labelled, revision="3"))
    assert wider.differences == ("revision", "rules", "contract.input", "contract.output")


def test_other_rules_other_engine() -> None:
    other = edited(bundle(rules=rules(9), revision="2"), {"name": "another"})
    comparison = bundle().compare(other)
    assert comparison.relation is BundleRelation.OTHER_RULES_OTHER_ENGINE
    assert comparison.differences[:3] == ("revision", "rules", "engine.name")


def test_conflict() -> None:
    comparison = bundle().compare(bundle(rules=rules(9)))
    assert comparison.relation is BundleRelation.CONFLICT
    assert "rules" in comparison.differences


def test_a_table_and_its_rule_set_differ_in_source() -> None:
    from_table = bundle(rules=table())
    comparison = from_table.compare(bundle(rules=table().ruleset))
    assert comparison.relation is BundleRelation.SAME_RULES_SAME_ENGINE
    assert comparison.differences == ("source",)


def test_compare_takes_a_bundle() -> None:
    with pytest.raises(TypeError):
        bundle().compare(rules())  # pyright: ignore[reportArgumentType]


# --- Decisions with their rule set digest ------------------------------------------------


def test_decisions_carry_the_rule_set_digest_on_request() -> None:
    made = bundle()
    decision = made.ruleset.evaluate({"years": 9})
    assert "ruleset_digest" not in decision.to_dict()["decision"]
    record = decision.to_dict(ruleset_digest=True)
    assert record["decision"]["ruleset_digest"] == made.ruleset_digest
    # Read back with the same rules, it keeps the digest either way.
    read = made.ruleset.read_decision(json.dumps(record))
    assert read.to_dict()["decision"]["ruleset_digest"] == made.ruleset_digest
    assert read.to_dict(ruleset_digest=True) == record
    # Other rules refuse it.
    with pytest.raises(FormatError) as raised:
        rules(9).read_decision(json.dumps(record))
    assert raised.value.code == "decision_mismatch"
    assert raised.value.pointer == "/decision/ruleset_digest"


def test_loads_does_not_bind_a_bundle() -> None:
    text = cleanrules.dumps(bundle())
    with pytest.raises(TypeError, match=r"bundle\.ruleset\.bind"):
        cleanrules.loads(text, actions={})


def test_differences_are_named_as_in_json_and_in_order() -> None:
    names = [difference.value for difference in BundleDifference]
    assert names == [
        "id",
        "revision",
        "rules",
        "source",
        "engine.name",
        "engine.version",
        *(f"engine.limits.{name}" for name in BundleLimits.__dataclass_fields__),
        "contract.input",
        "contract.output",
        "cases",
    ]


def test_decisions_are_dumped_with_their_rule_set_digest(tmp_path: Path) -> None:
    decision = rules().evaluate({"years": 9})
    record = json.loads(cleanrules.dumps(decision, ruleset_digest=True))
    assert record == decision.to_dict(ruleset_digest=True)
    assert record["decision"]["ruleset_digest"] == cleanrules.digest(rules())
    assert "ruleset_digest" not in json.loads(cleanrules.dumps(decision))["decision"]
    yaml = cleanrules.dumps(decision, format="yaml", ruleset_digest=True)
    assert "ruleset_digest: " in yaml
    with pytest.raises(UnsupportedError):
        cleanrules.dumps(decision, format="text")
    with pytest.raises(TypeError, match="ruleset_digest is an option of decisions"):
        cleanrules.dumps(rules(), ruleset_digest=True)
    cleanrules.dump(decision, tmp_path / "decision.json", ruleset_digest=True)
    assert json.loads((tmp_path / "decision.json").read_text()) == record
