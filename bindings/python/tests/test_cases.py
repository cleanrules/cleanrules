"""Case files and the case runner in Python (`spec/tests.md`, task T.9b): the core runs."""

from __future__ import annotations

import json
import threading
from decimal import Decimal
from pathlib import Path

import cleanrules
import pytest
from cleanrules import (
    Bundle,
    Case,
    CaseFile,
    CaseResult,
    Cases,
    CaseStatus,
    DecisionTable,
    Expectation,
    ExpectationKey,
    ExpectedOutput,
    F,
    FormatError,
    Interrupted,
    Interruption,
    Mismatch,
    MissingFieldError,
    Rule,
    RuleSet,
    TablePolicy,
    TestRun,
    TestRunBundle,
    TestRunFile,
    TestRunLimits,
    TestRunSummary,
    UnsupportedError,
)

EXAMPLES = Path(__file__).resolve().parents[3] / "docs" / "examples"


def discounts() -> RuleSet:
    rules = cleanrules.load(EXAMPLES / "discounts.json")
    assert isinstance(rules, RuleSet)
    return rules


def example_cases() -> Cases:
    cases = cleanrules.load(EXAMPLES / "cases" / "discounts.json")
    assert isinstance(cases, Cases)
    return cases


def ages(*, strict: bool = False) -> RuleSet:
    return RuleSet(
        [
            Rule("adult", F.age >= 18, {"band": "adult"}),
            Rule("child", F.age < 18, {"band": "child"}, id="CHILD"),
        ],
        strict=strict,
    )


def three(path: str | None = None) -> CaseFile:
    cases = Cases(
        [
            Case("a", {"age": 30}, expect=Expectation(selected=["adult"])),
            Case("b", {"age": 3}, expect=Expectation(selected=["adult"])),
            Case("c", {}, expect=Expectation(output={"band": "adult"})),
        ]
    )
    return CaseFile(cases, path=path)


# --- Case files --------------------------------------------------------------------------


def test_a_case_file_loads_into_python_objects_and_dumps_back() -> None:
    cases = example_cases()
    assert len(cases) == 4
    assert cases.description is not None and cases.description.startswith("the cases")
    vip = cases["vip-gets-15"]
    assert vip is cases[0] and cases.get("nobody") is None
    assert vip.input == {"customer": {"years": 7}, "order": {"total": 1500}}
    assert vip.expect.selected == ("vip",)
    assert vip.expect.output == {"discount": Decimal("0.15")}
    assert vip.expect.unknown == ("blocked",)
    assert vip.expect.outputs is None and vip.expect.error is None
    assert cases[3].expect.output is ExpectedOutput.NOTHING_SELECTED
    text = cleanrules.dumps(cases)
    assert text.startswith('{"version":1,"cases":{"description":')
    assert cleanrules.loads(text) == cases
    assert cleanrules.loads(cleanrules.dumps(cases, format="yaml"), format="yaml") == cases
    assert cleanrules.digest(cases).startswith("sha256:")
    assert cleanrules.canonical_bytes(cases) == cleanrules.canonical_bytes(cleanrules.loads(text))
    assert [case.id for case in cases[1:3]] == ["blocked-gets-nothing", "newsletter-is-loyal"]
    with pytest.raises(KeyError, match="no case has the id"):
        cases["nobody"]


def test_cases_are_built_with_the_cores_checks() -> None:
    expect = Expectation(
        selected=["vip"],
        output={"discount": Decimal("0.15"), "rate": 1.0},
        outputs=[{"discount": Decimal("0.15")}],
        unknown=["z", "b"],
    )
    assert expect.unknown == ("b", "z"), "sorted by code point"
    case = Case("vip", {"years": 7}, expect=expect, description="a vip")
    cases = Cases([case], description="discounts", metadata={"owner": "risk"})
    document = json.loads(cleanrules.dumps(cases))["cases"]
    assert document["metadata"] == {"owner": "risk"}
    item = document["items"][0]
    # A float is written as one, so it reads back as a float.
    assert item["expect"]["output"]["rate"] == 1.0
    assert '"rate":1.0' in cleanrules.dumps(cases)
    assert item["description"] == "a vip"
    assert Expectation().is_empty and not expect.is_empty
    assert Case("x", None).expect == Expectation()


@pytest.mark.parametrize(
    ("make", "message"),
    [
        (lambda: Case("", {}), "cannot be empty"),
        (lambda: Cases([Case("a", {}), Case("a", {})]), 'two cases have the id "a"'),
        (lambda: Case("blob", {"x": object()}), 'input["x"] is not a data value'),
        (lambda: Case("nan", {"x": float("nan")}), "NaN"),
        (lambda: Expectation(selected=["a", "a"]), 'named twice in "selected"'),
        (lambda: Expectation(unknown=[""]), "cannot be an empty string"),
        (lambda: Expectation(error="evaluation_error"), "raises only"),
        (lambda: Expectation(error="missing_field", selected=["a"]), "given alone"),
        (lambda: Expectation(output={"n": 2**70}), 'output["n"] is out of range'),
        (lambda: Expectation(outputs=[{}, {"n": Decimal("1e40")}]), 'outputs[1]["n"]'),
        (lambda: Cases([], metadata={"blob": object()}), 'metadata["blob"]'),
    ],
)
def test_constructors_refuse_what_the_reader_refuses(make: object, message: str) -> None:
    with pytest.raises(FormatError, match=message.replace("[", r"\[").replace("]", r"\]")):
        make()  # type: ignore[operator]


def test_an_input_out_of_range_is_null_as_in_json() -> None:
    case = Case("big", {"n": 2**70})
    assert case.input == {"n": None}


def test_wrong_arguments_are_type_and_value_errors() -> None:
    with pytest.raises(TypeError, match="not a string"):
        Expectation(selected="vip")
    with pytest.raises(TypeError, match="rule ids as str"):
        Expectation(unknown=[1])  # type: ignore[list-item]
    with pytest.raises(TypeError, match="NOTHING_SELECTED"):
        Expectation(output=[1])  # type: ignore[arg-type]
    with pytest.raises(TypeError, match="list of dicts"):
        Expectation(outputs={"a": 1})  # type: ignore[arg-type]
    with pytest.raises(TypeError, match="holds dicts"):
        Expectation(outputs=[1])  # type: ignore[list-item]
    with pytest.raises(ValueError, match="unknown error code"):
        Expectation(error="no_such_code")
    with pytest.raises(TypeError, match="error must be a str"):
        Expectation(error=1)  # type: ignore[arg-type]
    with pytest.raises(TypeError, match="case id must be a str"):
        Case(1, {})  # type: ignore[arg-type]
    with pytest.raises(TypeError, match="must be an Expectation"):
        Case("a", {}, expect={})  # type: ignore[arg-type]
    with pytest.raises(TypeError, match="holds Case objects"):
        Cases([{}])  # type: ignore[list-item]
    with pytest.raises(TypeError, match="metadata must be a dict"):
        Cases([], metadata=[1])  # type: ignore[arg-type]
    with pytest.raises(TypeError, match="holds Cases"):
        CaseFile([])  # type: ignore[arg-type]
    with pytest.raises(TypeError, match="bytes or str"):
        CaseFile.read(1, path="a.json")  # type: ignore[arg-type]
    with pytest.raises(ValueError, match="JSON or YAML"):
        CaseFile.read("{}", path="a.json", format="csv")  # type: ignore[arg-type]


def test_case_objects_are_immutable_and_compare_by_value() -> None:
    cases = example_cases()
    for value in (cases, cases[0], cases[0].expect, CaseFile(cases)):
        with pytest.raises(AttributeError, match="immutable"):
            value.description = "x"  # type: ignore[misc]
        with pytest.raises(AttributeError, match="immutable"):
            del value.description  # type: ignore[misc]
    assert Case("a", {"x": Decimal("0.10")}) != Case("a", {"x": Decimal("0.1")}), "decimal scales"
    assert hash(Case("a", {"x": 1})) == hash(Case("a", {"x": 1}))
    assert Expectation(unknown=["b", "a"]) == Expectation(unknown=["a", "b"])
    assert len({cases, example_cases()}) == 1
    assert cases != "cases" and Case("a", {}) != 1 and Expectation() != None  # noqa: E711
    assert "4 cases" in repr(cases) and "vip-gets-15" in repr(cases[0])
    assert "selected=('vip',)" in repr(cases[0].expect)


def test_loads_takes_no_actions_and_dumps_refuses_test_runs() -> None:
    with pytest.raises(TypeError, match="never run them"):
        cleanrules.loads(cleanrules.dumps(example_cases()), actions={})
    with pytest.raises(UnsupportedError):
        cleanrules.dumps(example_cases(), format="text")
    run = discounts().test(example_cases())
    with pytest.raises(TypeError, match="output only"):
        cleanrules.dumps(run)  # type: ignore[arg-type]
    with pytest.raises(TypeError, match="case file"):
        cleanrules.digest(run)  # type: ignore[arg-type]


def test_a_case_file_keeps_its_path_digest_and_error(tmp_path: Path) -> None:
    text = (EXAMPLES / "cases" / "discounts.json").read_bytes()
    read = CaseFile.read(text, path="cases/discounts.json")
    assert read.path == "cases/discounts.json" and read.error is None
    assert read.digest is not None and read.digest.startswith("sha256:")
    assert read.cases == example_cases()
    assert read.case("vip-gets-15") is not None and read.case("nobody") is None
    built = CaseFile(example_cases())
    assert built.path is None and built.digest is None
    bad = CaseFile.read("[]", path="bad.json")
    assert bad.cases is None and bad.case("a") is None
    assert isinstance(bad.error, FormatError) and bad.error.pointer == ""
    yaml = CaseFile.read(cleanrules.dumps(example_cases(), format="yaml"), path="c.YML")
    assert yaml.cases == example_cases(), "read as YAML by its suffix"
    forced = CaseFile.read(cleanrules.dumps(example_cases(), format="yaml"), format="yaml")
    assert forced.cases == example_cases()
    rules = CaseFile.read(cleanrules.dumps(discounts()), path="rules.json")
    assert rules.error is not None and "not test cases" in rules.error.message
    path = tmp_path / "cases.json"
    path.write_bytes(text)
    loaded = CaseFile.load(path)
    assert loaded.path == path.as_posix() and loaded.digest == read.digest
    assert "4 cases" in repr(loaded) and "unreadable" in repr(bad)
    assert "without a path" in repr(built)


# --- Running ----------------------------------------------------------------------------


def test_the_example_cases_pass() -> None:
    run = discounts().test(CaseFile.read(_example_bytes(), path="cases/discounts.json"))
    assert isinstance(run, TestRun)
    assert run.all_passed
    assert run.summary == TestRunSummary(4, 4, 0, 0, False)
    assert run.source == "ruleset" and run.hit_policy == TablePolicy.PRIORITY
    assert run.logic == "boolean" and not run.strict and run.aggregate is None
    assert run.ruleset_digest is None and run.bundle is None
    assert run.engine.name == "cleanrules" and run.engine.is_current
    assert run.engine.limits.max_size == 16 << 20
    assert run.limits == TestRunLimits(5)
    assert run.files == (TestRunFile("cases/discounts.json", run.files[0].digest),)
    assert [r.status for r in run.results] == [CaseStatus.PASSED] * 4
    assert str(run).startswith("Test run of a rule set: 3 rules, hit policy priority")
    assert str(run).endswith("4 results: 4 passed, 0 failed, 0 errors\n")
    document = run.to_dict()
    assert document["version"] == 1 and document["test_run"]["summary"]["total"] == 4
    assert json.loads(json.dumps(document)) == document
    assert "4 passed" in repr(run)
    with pytest.raises(AttributeError, match="immutable"):
        run.summary = None  # type: ignore[misc,assignment]


def _example_bytes() -> bytes:
    return (EXAMPLES / "cases" / "discounts.json").read_bytes()


def test_paths_are_read_and_the_digest_is_an_option(tmp_path: Path) -> None:
    path = tmp_path / "discounts.json"
    path.write_bytes(_example_bytes())
    run = discounts().test(str(path), ruleset_digest=True)
    assert run.all_passed and run.files[0].path == path.as_posix()
    assert run.ruleset_digest == cleanrules.digest(discounts())
    with pytest.raises(OSError):
        discounts().test(tmp_path / "missing.json")


def test_failures_errors_and_mismatches() -> None:
    cases = Cases(
        [
            Case(
                "every-key",
                {"customer": {"years": 7}, "order": {"total": 1500}},
                expect=Expectation(
                    selected=["loyal"],
                    output={"discount": Decimal("0.05")},
                    outputs=[{"discount": Decimal("0.05")}],
                    unknown=[],
                ),
            ),
            Case(
                "no-error", {"customer": {"years": 1}}, expect=Expectation(error="hit_policy_error")
            ),
            Case("missing", {}, expect=Expectation(error="missing_field")),
        ]
    )
    run = discounts().test([cases, CaseFile.read("[]", path="bad.json")], max_explained=1)
    assert not run.all_passed
    assert (run.summary.passed, run.summary.failed, run.summary.error) == (0, 3, 1)
    every, no_error, missing, bad = run.results
    assert every.status == CaseStatus.FAILED and every.path is None and every.id == "every-key"
    assert [m.key for m in every.mismatches] == [
        ExpectationKey.SELECTED,
        ExpectationKey.OUTPUT,
        ExpectationKey.OUTPUTS,
        ExpectationKey.UNKNOWN,
    ]
    assert every.mismatches[0] == Mismatch(ExpectationKey.SELECTED, ["loyal"], ["vip"])
    assert every.mismatches[1].actual == {"discount": Decimal("0.15")}
    assert every.explanation_text is not None and "HIT POLICY priority" in every.explanation_text
    assert no_error.explanation_text is None, "max_explained=1"
    assert no_error.mismatches == (Mismatch(ExpectationKey.ERROR, "hit_policy_error", None),)
    # Not strict: a missing field is unknown, so no error is raised.
    assert missing.mismatches == (Mismatch(ExpectationKey.ERROR, "missing_field", None),)
    assert bad.status == CaseStatus.ERROR and bad.id is None and bad.path == "bad.json"
    assert isinstance(bad.error, FormatError)
    assert bad.to_dict() == run.to_dict()["test_run"]["results"][3]
    assert bad == run.results[3] and hash(bad) == hash(run.results[3]) and bad != every
    assert "bad.json" in repr(bad) and "(a case file without a path)" not in repr(bad)
    assert 'FAILED  "every-key"' in str(run) and "ERROR   bad.json" in str(run)


def test_an_unexpected_error_is_the_cases_result() -> None:
    run = ages(strict=True).test(Cases([Case("no-age", {}), Case("strict", {"age": 1})]))
    first = run.results[0]
    assert first.status == CaseStatus.ERROR and isinstance(first.error, MissingFieldError)
    assert run.results[1].status == CaseStatus.PASSED
    expected = ages(strict=True).test(
        Cases([Case("no-age", {}, expect=Expectation(error="missing_field"))])
    )
    assert expected.all_passed


def test_a_single_output_of_a_multi_hit_decision_is_a_reason() -> None:
    rules = RuleSet(
        [Rule("a", F.x > 0, {"n": 1}), Rule("b", F.x > 1, {"n": 2})], hit_policy="rule_order"
    )
    run = rules.test(Cases([Case("two", {"x": 2}, expect=Expectation(output={"n": 1}))]))
    (mismatch,) = run.results[0].mismatches
    assert mismatch.key == ExpectationKey.OUTPUT and mismatch.actual is None
    assert mismatch.reason == "the decision has no single output (rule_order)"
    nothing = rules.test(
        Cases([Case("none", {"x": 0}, expect=Expectation(outputs=[], selected=[]))])
    )
    assert nothing.all_passed


def test_a_decision_table_runs_through_its_rule_set() -> None:
    table = cleanrules.loads(
        json.dumps(
            {
                "version": 1,
                "decision_table": {
                    "hit_policy": "unique",
                    "inputs": ["age"],
                    "outputs": [{"name": "band"}],
                    "rows": [
                        {"inputs": [">= 18"], "outputs": ["'adult'"]},
                        {"inputs": ["< 18"], "outputs": ["'child'"]},
                    ],
                },
            }
        )
    )
    assert isinstance(table, DecisionTable)
    run = table.test(
        Cases([Case("adult", {"age": 30}, expect=Expectation(output={"band": "adult"}))]),
        ruleset_digest=True,
    )
    assert run.all_passed and run.source == "decision_table"
    assert run.hit_policy == TablePolicy.UNIQUE
    assert run.ruleset_digest == cleanrules.digest(table.ruleset)
    run = table.iter_test(Cases([Case("a", {"age": 1}), Case("b", {"age": 2})]), max_results=1)
    assert [r.id for r in run] == ["a"]
    assert run.test_run().summary.interrupted == Interrupted(Interruption.MAX_RESULTS, 1)


def test_leaving_the_loop_early_gives_a_partial_cancelled_run() -> None:
    run = ages().iter_test([three("one.json"), three("two.json")])
    assert not run.is_finished and run.remaining == 6
    for result in run:
        assert result.id == "a"
        break
    assert run.summary.total == 1 and run.remaining == 5
    report = run.test_run()
    assert report.summary.partial
    assert report.summary.interrupted == Interrupted(Interruption.CANCELLED, 5)
    assert len(report.results) == 1 and run.is_finished and run.remaining == 0
    assert run.test_run() is report, "made once"
    assert list(run) == []
    assert not report.all_passed
    assert "Partial run: interrupted by cancelled, 5 not run" in str(report)
    assert "partial" in repr(report) and "finished" in repr(run)


def test_cancel_is_asked_between_results() -> None:
    stop = threading.Event()
    asked: list[int] = []

    def cancel() -> bool:
        asked.append(1)
        return stop.is_set()

    run = ages().iter_test(three(), cancel=cancel)
    first = next(run)
    assert first.id == "a" and len(asked) == 1
    stop.set()
    assert list(run) == []
    report = run.test_run()
    assert report.summary.interrupted == Interrupted(Interruption.CANCELLED, 2)
    # A run that has given its last result is complete, whatever cancel says.
    done = ages().test(three(), cancel=lambda: len(asked) > 100)
    assert not done.summary.partial and done.summary.total == 3


def test_a_cancel_that_raises_stops_the_run() -> None:
    def cancel() -> bool:
        raise RuntimeError("deadline lost")

    run = ages().iter_test(three(), cancel=cancel)
    with pytest.raises(RuntimeError, match="deadline lost"):
        next(run)
    report = run.test_run()
    assert report.summary.total == 0
    assert report.summary.interrupted == Interrupted(Interruption.CANCELLED, 3)
    with pytest.raises(RuntimeError, match="deadline lost"):
        ages().test(three(), cancel=cancel)


def test_cancel_and_the_limits() -> None:
    run = ages().iter_test(three())
    run.cancel()
    assert run.is_finished and list(run) == []
    assert run.summary.interrupted == Interrupted(Interruption.CANCELLED, 3)
    steps = ages().test(three(), max_steps=5)
    assert steps.summary.total == 2
    assert steps.summary.interrupted == Interrupted(Interruption.MAX_STEPS, 1)
    assert steps.limits == TestRunLimits(5, None, 5)
    huge = ages().test(three(), max_results=2**80, max_steps=2**80)
    assert huge.summary.total == 3 and huge.limits.max_results is not None
    none = ages().test(three(), max_explained=0)
    assert all(r.explanation_text is None for r in none.results)
    assert none.limits == TestRunLimits(0)


@pytest.mark.parametrize(
    ("options", "error"),
    [
        ({"max_results": -1}, ValueError),
        ({"max_steps": 1.5}, TypeError),
        ({"max_explained": True}, TypeError),
        ({"cancel": 1}, TypeError),
        ({"ruleset_digest": 1}, TypeError),
    ],
)
def test_options_are_checked(options: dict[str, object], error: type[Exception]) -> None:
    with pytest.raises(error):
        ages().test(three(), **options)  # type: ignore[arg-type]


def test_files_are_case_files_cases_or_paths() -> None:
    with pytest.raises(TypeError, match="a case file is a CaseFile, Cases or a path"):
        ages().test([1])  # type: ignore[list-item]
    run = ages().test([three("a.json"), three("a.json"), three(), three()])
    errors = [r for r in run.results if r.status == CaseStatus.ERROR]
    assert [(r.path, r.id) for r in errors] == [("a.json", None), (None, None)]
    assert run.summary.total == 8


def test_a_bundles_case_files_run_against_its_rules(tmp_path: Path) -> None:
    good = cleanrules.dumps(
        Cases([Case("adult", {"age": 30}, expect=Expectation(selected=["adult"]))])
    ).encode()
    bundle = Bundle(ages(), id="acme/ages", revision="1", cases=[("cases/good.json", good)])
    (tmp_path / "cases").mkdir()
    (tmp_path / "cases" / "good.json").write_bytes(good)
    run = bundle.test(tmp_path)
    assert run.all_passed and run.bundle == TestRunBundle("acme/ages", "1")
    assert run.results[0].path == "cases/good.json"
    read: list[str] = []

    def reader(ref: cleanrules.CaseFileRef) -> str:
        read.append(ref.path)
        return good.decode() + " "

    changed = bundle.test(reader)
    assert read == ["cases/good.json"]
    (result,) = changed.results
    assert result.status == CaseStatus.ERROR and result.error is not None
    assert result.error.code == "decision_mismatch"
    assert result.error.pointer == "/bundle/cases/0/digest"
    partial = bundle.iter_test(lambda ref: bytearray(good), max_results=0)  # pyright: ignore[reportArgumentType]
    assert list(partial) == []
    assert partial.test_run().summary.interrupted == Interrupted(Interruption.MAX_RESULTS, 1)
    with pytest.raises(TypeError, match="bytes or str"):
        bundle.test(lambda ref: 1)  # type: ignore[arg-type,return-value]
    with pytest.raises(TypeError, match="callable given each CaseFileRef"):
        bundle.test(1)  # type: ignore[arg-type]
    with pytest.raises(OSError):
        bundle.test(tmp_path / "nowhere")


def test_results_hold_errors_and_unknown_mismatch_keys() -> None:
    result = CaseResult(
        (
            "a.json",
            "x",
            "failed",
            [("future", None, None, False, None, False)],
            None,
            None,
            json.dumps(
                {
                    "path": "a.json",
                    "id": "x",
                    "status": "failed",
                    "mismatches": [{"key": "future", "expected": 1, "actual": 2}],
                }
            ),
        )
    )
    assert result.mismatches == (Mismatch("future", 1, 2),)


# --- From the API review --------------------------------------------------------------


def test_the_run_cannot_be_read_from_its_own_cancel() -> None:
    seen: list[str] = []
    run: cleanrules.CaseRun

    def reads() -> bool:
        for read in (
            lambda: run.summary,
            lambda: run.remaining,
            lambda: run.is_finished,
            lambda: run.test_run(),
            lambda: next(run),
        ):
            with pytest.raises(TypeError, match="cannot be read from its own cancel callback"):
                read()
            seen.append("refused")
        return False

    run = ages().iter_test(three(), cancel=reads)
    assert next(run).id == "a"
    assert seen == ["refused"] * 5
    assert repr(run).startswith("<CaseRun: 1 results")

    def stops() -> bool:
        run2.cancel()  # asks the run to stop, as returning True does
        return False

    run2 = ages().iter_test(three(), cancel=stops)
    assert list(run2) == []
    report = run2.test_run()
    assert report.summary.interrupted == Interrupted(Interruption.CANCELLED, 3)
    assert report.summary.total == 0


def test_breaking_a_python_loop_leaves_the_run_where_it_is() -> None:
    run = ages().iter_test(three())
    for _ in run:
        break
    assert not run.is_finished and run.remaining == 2, "Python tells an iterator nothing"
    assert [r.id for r in run] == ["b", "c"], "it goes on"
    assert not run.test_run().summary.partial


def test_ctrl_c_stops_a_run_between_cases() -> None:
    """Ctrl-C during `test()` (no `cancel`), without a timed race.

    With a long switch interval, the main thread keeps the GIL until the run
    releases it to evaluate a case; only then can the interrupting thread,
    woken before the run starts, take the GIL and trip SIGINT. So the signal
    always arrives while the run is in the core, after its first case, and
    the run sees it at its next check between cases. Only the OS scheduling
    the woken thread within the run's 300,000 cases is assumed.
    """
    import _thread
    import sys
    import threading

    items = [{"id": str(i), "input": {"age": i % 40}} for i in range(300_000)]
    big = CaseFile.read(json.dumps({"version": 1, "cases": {"items": items}}), path="big.json")
    run = ages().iter_test(big)
    go = threading.Event()

    def interrupt() -> None:
        go.wait()
        _thread.interrupt_main()

    thread = threading.Thread(target=interrupt)
    interval = sys.getswitchinterval()
    sys.setswitchinterval(1000.0)
    try:
        thread.start()
        go.set()
        with pytest.raises(KeyboardInterrupt):
            run._advance(True)  # pyright: ignore[reportPrivateUsage]  # as test() runs
    finally:
        sys.setswitchinterval(interval)
        thread.join()
    report = run.test_run()
    assert report.summary.partial and report.summary.interrupted is not None
    assert report.summary.interrupted.reason == Interruption.CANCELLED
    assert report.summary.total >= 1, "interrupted in the core, after a case"
    assert report.summary.total + report.summary.interrupted.remaining == 300_000


def test_an_output_mismatch_says_nothing_selected() -> None:
    rules = RuleSet([Rule("adult", F.age >= 18, {"band": "adult"})])
    run = rules.test(
        Cases(
            [
                Case("young", {"age": 3}, expect=Expectation(output={"band": "adult"})),
                Case(
                    "old",
                    {"age": 30},
                    expect=Expectation(output=ExpectedOutput.NOTHING_SELECTED),
                ),
            ]
        )
    )
    young, old = (r.mismatches[0] for r in run.results)
    assert young.actual is ExpectedOutput.NOTHING_SELECTED
    assert old.expected is ExpectedOutput.NOTHING_SELECTED
    assert old.actual == {"band": "adult"}
    with pytest.raises(AttributeError, match="immutable"):
        young.reason = "x"  # type: ignore[misc]
    with pytest.raises(TypeError):
        hash(young)
    assert "NOTHING_SELECTED" in repr(old) and old != 1


def test_values_are_fresh_copies() -> None:
    cases = Cases(
        [Case("a", {"x": [1]}, expect=Expectation(output={"n": 1}, outputs=[{"n": 1}]))],
        metadata={"owner": "risk"},
    )
    case = cases[0]
    case.input["x"].append(2)
    assert case.input == {"x": [1]}
    output = case.expect.output
    assert isinstance(output, dict)
    output["n"] = 2
    assert case.expect.output == {"n": 1}
    outputs = case.expect.outputs
    assert outputs is not None
    outputs[0]["n"] = 2
    assert case.expect.outputs == ({"n": 1},)
    cases.metadata["owner"] = "x"
    assert cases.metadata == {"owner": "risk"}
    run = ages().test(Cases([Case("a", {"age": 30}, expect=Expectation(selected=["child"]))]))
    mismatch = run.results[0].mismatches[0]
    mismatch.expected.append("x")
    assert mismatch.expected == ["child"]


def test_cases_contain_ids_and_cases() -> None:
    cases = example_cases()
    assert "vip-gets-15" in cases and cases[0] in cases
    assert "nobody" not in cases and Case("x", {}) not in cases


def test_a_bundle_directory_is_read_up_to_the_cores_bound(tmp_path: Path) -> None:
    good = cleanrules.dumps(Cases([Case("adult", {"age": 30})])).encode()
    bundle = Bundle(
        ages(), id="acme/ages", revision="1", cases=[("cases/good.json", good)], max_size=10
    )
    (tmp_path / "cases").mkdir()
    (tmp_path / "cases" / "good.json").write_bytes(good + b" " * (17 << 20))
    (result,) = bundle.test(tmp_path).results
    assert result.error is not None and result.error.code == "limit_exceeded"
    assert "more than the limit of 10" in result.error.message
    (tmp_path / "cases" / "good.json").write_bytes(good)
    (result,) = bundle.test(tmp_path).results
    # Within the bound: checked against its digest, then read under the bundle's limit.
    assert result.error is not None and result.error.code == "limit_exceeded"
