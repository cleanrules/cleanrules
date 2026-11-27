"""Generate and check the vectors of a run's statistics (`spec/tests.md`, *Statistics*).

The vectors run test cases against rules gathering statistics (`kind`
`statistics`): how many times each rule was selected, fired, did not fire or
was unknown, how many times each condition (a test of a rule set, a cell of a
decision table) was `TRUE`, `FALSE` or `UNKNOWN`, the output values of a
table, and the coverage those counts give. The expected statistics are
written by hand below, from the spec, case by case in the comments; the
script only lays them out and computes the digests of a bundle's case files
with the oracle of `canonical_vectors.py`.

    python3 conformance/tools/statistics_vectors.py --write   # tests/statistics.json
    python3 conformance/tools/statistics_vectors.py --check   # CI: the file is up to date
"""

from __future__ import annotations

import sys
from typing import Any

from canonical_vectors import VECTORS, IntLit, bundle, digest, dump, table_basic
from test_run_vectors import case, case_file, cmp, const, dec, field, numbers, rule, ruleset, text

STATISTICS = "tests.md#statistics"
RULES = "tests.md#rules"
CONDITIONS = "tests.md#conditions"
OUTPUTS = "tests.md#outputs"
COVERAGE = "tests.md#coverage"
JSON = "tests.md#json"
BUNDLES = "tests.md#case-files-of-a-bundle"
BATCHES = "tests.md#batches-and-interruption"

MEASURES = [
    "rule_selected",
    "rule_fired",
    "rule_not_fired",
    "rule_unknown",
    "condition_true",
    "condition_false",
    "condition_unknown",
]


def node(op: str, **keys: Any) -> dict[str, Any]:
    """A condition node."""
    return {"op": op, **keys}


def cond(
    pointer: str, op: str, t: int, f: int, u: int, *, constant: str | None = None
) -> dict[str, Any]:
    """A condition's counts; `constant` is the truth value of a test that reads no field."""
    out: dict[str, Any] = {"pointer": pointer, "op": op, "true": t, "false": f, "unknown": u}
    if constant is not None:
        out["_constant"] = constant
    return out


def stats_rule(
    name: str,
    counts: tuple[int, int, int, int],
    conditions: list[Any],
    *,
    id: str | None = None,
    row: int | None = None,
    constant: str | None = None,
) -> dict[str, Any]:
    """A rule's statistics: selected, fired, not fired and unknown, and its conditions.

    `constant` is the truth value of a condition that reads no field. The
    rule's pointer is added by `statistics`, from its place.
    """
    out: dict[str, Any] = {"name": name}
    if id is not None:
        out["id"] = id
    if row is not None:
        out["row"] = row
    selected, fired, not_fired, unknown = counts
    out.update(selected=selected, fired=fired, not_fired=not_fired, unknown=unknown)
    out["conditions"] = conditions
    if constant is not None:
        out["_constant"] = constant
    return out


def reaches(item: dict[str, Any], truth: str) -> bool:
    """Whether an item can have the truth value `truth` (*Coverage*: unreachable items)."""
    return item.get("_constant", truth) == truth


def coverage(rules: list[Any], outputs: list[Any] | None = None) -> list[Any]:
    """The coverage of the counts, as `tests.md` (*Coverage*) derives it.

    The vectors write every count by hand, and mark the items that read no
    field with their truth value; this only applies the table of measures to
    them, so that the expected coverage cannot disagree with the expected
    counts.
    """
    out = []
    counts = [
        ("selected", "true"),
        ("fired", "true"),
        ("not_fired", "false"),
        ("unknown", "unknown"),
    ]
    for measure, (key, truth) in zip(MEASURES[:4], counts, strict=True):
        items = [({"rule": r["name"], "pointer": r["pointer"]}, r, r[key]) for r in rules]
        out.append(measure_of(measure, items, truth))
    for measure, key in zip(MEASURES[4:], ["true", "false", "unknown"], strict=True):
        items = [
            ({"rule": r["name"], "pointer": c["pointer"]}, c, c[key])
            for r in rules
            for c in r["conditions"]
        ]
        out.append(measure_of(measure, items, key))
    if outputs:
        items = [
            (
                {"output": o["output"], "value": v["value"], "pointer": v["_pointer"]},
                v,
                v["selected"],
            )
            for o in outputs
            for v in o["values"]
        ]
        out.append(measure_of("output_value", items, None))
    return out


def measure_of(measure: str, items: list[Any], truth: str | None) -> dict[str, Any]:
    """One measure: each item as (missed entry, item, count)."""
    reachable = [(m, n) for m, item, n in items if truth is None or reaches(item, truth)]
    missed = [m for m, n in reachable if n == 0]
    out: dict[str, Any] = {
        "measure": measure,
        "covered": len(reachable) - len(missed),
        "total": len(reachable),
    }
    if len(items) > len(reachable):
        out["unreachable"] = len(items) - len(reachable)
    out["missed"] = missed
    return out


def visible(value: Any) -> Any:
    """`value` without the marks of the hand-written data (keys that start with `_`)."""
    if isinstance(value, list):
        return [visible(v) for v in value]
    if isinstance(value, dict):
        return {k: visible(v) for k, v in value.items() if not k.startswith("_")}
    return value


def statistics(
    cases: int,
    decisions: int,
    rules: list[Any],
    outputs: list[Any] | None = None,
    prefix: str = "",
) -> dict[str, Any]:
    """The statistics member of a test run, with the pointers of rules and output values."""
    placed = []
    for i, r in enumerate(rules):
        at = (
            f"{prefix}/decision_table/rows/{r['row']}"
            if "row" in r
            else f"{prefix}/ruleset/rules/{i}"
        )
        rule: dict[str, Any] = {}
        for key, value in r.items():
            if key == "selected":
                rule["pointer"] = at
            rule[key] = value
        placed.append(rule)
    for o in outputs or []:
        for p, v in enumerate(o["values"]):
            v["_pointer"] = f"{prefix}/decision_table/outputs/{o['_column']}/values/{p}"
    out: dict[str, Any] = {"cases": cases, "decisions": decisions, "rules": placed}
    if outputs is not None:
        out["outputs"] = outputs
    out["coverage"] = coverage(placed, outputs)
    return visible(out)


def vector(
    name: str,
    spec: list[str],
    description: str,
    document: Any,
    expected: Any,
    *,
    cases: Any = None,
    files: list[tuple[str, str]] | None = None,
    limits: dict[str, int] | None = None,
) -> dict[str, Any]:
    """A `statistics` vector."""
    given: dict[str, Any] = {"kind": "statistics", "document": document}
    if cases is not None:
        given["cases"] = cases
    if files is not None:
        given["files"] = [{"path": p, "text": t} for p, t in files]
    if limits:
        given["limits"] = limits
    return {
        "id": f"tests/statistics/{name}",
        "spec": spec,
        "description": description,
        "given": given,
        "expect": {"statistics": expected},
    }


def years(low: int, high: int) -> dict[str, Any]:
    """`years BETWEEN low AND high`."""
    return node("between", operand=field("years"), low=const(low), high=const(high))


def rule_set_vectors() -> list[Any]:
    """Rules, tests and coverage of a rule set."""
    discounts = ruleset(
        "first",
        rule(
            "vip",
            node("and", operands=[cmp("gt", "years", 5), cmp("gt", "total", 1000)]),
            {"discount": dec("0.15")},
            id="VIP-1",
        ),
        rule(
            "loyal",
            node(
                "or",
                operands=[
                    years(2, 5),
                    node("not", operand=node("in", operand=field("country"), values=["ES", "PT"])),
                ],
            ),
            {"discount": dec("0.05")},
        ),
        rule("new", node("is_null", operand=field("years")), {"discount": 0}),
    )
    inputs = case_file(
        case("es-vip", {"years": 7, "total": 2000, "country": "ES"}),
        case("fr-loyal", {"years": 3, "total": 50, "country": "FR"}),
        case("unknown-years", {"total": 10}),
        case("es-new", {"years": 1, "total": 5000, "country": "ES"}),
    )
    # es-vip:        vip T,T -> fired, selected; loyal between F, in T (not F) -> F; new F.
    # fr-loyal:      vip F,F -> F; loyal between T, in F (not T) -> fired, selected; new F.
    # unknown-years: vip U,F -> F; loyal between U, in U (not U) -> U; new T -> selected.
    # es-new:        vip F,T -> F; loyal between F, in T (not F) -> F; new F; nothing selected.
    counted = statistics(
        4,
        4,
        [
            stats_rule(
                "vip",
                (1, 1, 3, 0),
                [
                    cond("/ruleset/rules/0/when/operands/0", "gt", 1, 2, 1),
                    cond("/ruleset/rules/0/when/operands/1", "gt", 2, 2, 0),
                ],
                id="VIP-1",
            ),
            stats_rule(
                "loyal",
                (1, 1, 2, 1),
                [
                    cond("/ruleset/rules/1/when/operands/0", "between", 1, 2, 1),
                    cond("/ruleset/rules/1/when/operands/1/operand", "in", 2, 1, 1),
                ],
            ),
            stats_rule("new", (1, 1, 3, 0), [cond("/ruleset/rules/2/when", "is_null", 1, 3, 0)]),
        ],
    )
    nested = ruleset(
        "first",
        rule(
            "r",
            node(
                "and",
                operands=[
                    node(
                        "or",
                        operands=[
                            field("flag"),
                            node("starts_with", operand=field("name"), text="A"),
                        ],
                    ),
                    node(
                        "not",
                        operand=node("and", operands=[cmp("gt", "x", 1), cmp("lt", "x", 5)]),
                    ),
                ],
            ),
        ),
    )
    # flag T, starts_with F (or T); x > 1 T, x < 5 T (and T, not F); the root is F.
    at = "/ruleset/rules/0/when/operands"
    nested_counted = statistics(
        1,
        1,
        [
            stats_rule(
                "r",
                (0, 0, 1, 0),
                [
                    cond(f"{at}/0/operands/0", "field", 1, 0, 0),
                    cond(f"{at}/0/operands/1", "starts_with", 0, 1, 0),
                    cond(f"{at}/1/operand/operands/0", "gt", 1, 0, 0),
                    cond(f"{at}/1/operand/operands/1", "lt", 1, 0, 0),
                ],
            )
        ],
    )
    one = ruleset("first", rule("a", cmp("gt", "x", 0)))
    return [
        vector(
            "ruleset",
            [STATISTICS, RULES, CONDITIONS, COVERAGE, JSON],
            "each rule's selections and truth values, each test's truth values (between as one"
            " test, the test under a not), and the coverage they give, over cases without"
            " expectations",
            discounts,
            counted,
            cases=inputs,
        ),
        vector(
            "tests-in-pre-order",
            [CONDITIONS, JSON],
            "the tests of a condition under and, or and not, in depth-first pre-order, each"
            " located by its pointer and named by its op; connectives are not counted",
            nested,
            nested_counted,
            cases=case_file(case("one", {"flag": True, "name": "Bob", "x": 3})),
        ),
        vector(
            "no-cases",
            [STATISTICS, COVERAGE],
            "a run of no cases counts nothing, and its coverage misses every item",
            one,
            statistics(
                0,
                0,
                [stats_rule("a", (0, 0, 0, 0), [cond("/ruleset/rules/0/when", "gt", 0, 0, 0)])],
            ),
            cases=case_file(),
        ),
    ]


def error_vectors() -> list[Any]:
    """Cases without a decision, and files that cannot be read."""
    strict = ruleset(
        "first", rule("a", cmp("gt", "x", 0)), rule("b", cmp("lt", "x", 10)), strict=True
    )
    good = text(
        case_file(
            case("five", {"x": 5}),
            case("missing", {}, {"error": {"code": "missing_field"}}),
            case("minus-one", {"x": -1}),
        )
    )
    # five: a T (selected), b T; missing: strict missing_field, no decision;
    # minus-one: a F, b T (selected). The broken file counts nowhere.
    unique = ruleset("unique", rule("a", cmp("gt", "x", 0)), rule("b", cmp("gt", "x", 5)))
    # one: a only; seven: a and b fire, so unique raises hit_policy_error; zero: neither.
    return [
        vector(
            "errors",
            [STATISTICS, RULES],
            "a case whose evaluation raises an error counts as a case and in no rule, whether it"
            " expects the error or not, and a file that cannot be read counts nowhere",
            strict,
            statistics(
                3,
                2,
                [
                    stats_rule("a", (1, 1, 1, 0), [cond("/ruleset/rules/0/when", "gt", 1, 1, 0)]),
                    stats_rule("b", (1, 2, 0, 0), [cond("/ruleset/rules/1/when", "lt", 2, 0, 0)]),
                ],
            ),
            files=[("cases/good.json", good), ("cases/broken.json", "{")],
        ),
        vector(
            "hit-policy-error",
            [STATISTICS, RULES],
            "a case for which the hit policy raises hit_policy_error gives no decision, so the"
            " rules that fired in it are not counted",
            unique,
            statistics(
                3,
                2,
                [
                    stats_rule("a", (1, 1, 1, 0), [cond("/ruleset/rules/0/when", "gt", 1, 1, 0)]),
                    stats_rule("b", (0, 0, 2, 0), [cond("/ruleset/rules/1/when", "gt", 0, 2, 0)]),
                ],
            ),
            cases=case_file(case("one", {"x": 1}), case("seven", {"x": 7}), case("zero", {"x": 0})),
        ),
    ]


def table_vectors() -> list[Any]:
    """The rows, cells and output values of a decision table, and of a bundle's."""
    risks = {
        "version": 1,
        "decision_table": {
            "hit_policy": "output_order",
            "logic": "boolean",
            "inputs": ["age", "country"],
            "outputs": [
                {"name": "risk", "values": ["very_high", "high", "medium", "low"]},
                {"name": "note"},
            ],
            "rows": [
                {"name": "young", "inputs": ["< 18", "-"], "outputs": ["'medium'", "'minor'"]},
                {
                    "name": "abroad",
                    "inputs": ["-", "NOT IN ('ES', 'PT')"],
                    "outputs": ["'high'", None],
                },
                {"inputs": ["-", "-"], "outputs": ["'low'", None]},
                {"name": "blocked", "inputs": ["-", "'XX'"], "outputs": ["'very_high'", None]},
            ],
        },
    }
    # Output order: blocked (row 3), abroad (row 1), young (row 0), row_3 (row 2).
    # A cell's NOT is stripped: abroad counts its test `in`. row_3 reads no field
    # (every cell is -): its condition is TRUE, so never FALSE nor UNKNOWN.
    # child-es:   blocked F; abroad: in T, not F; young T; row_3 T. Selected young, row_3.
    # adult-fr:   blocked F; abroad: in F, not T; young F; row_3 T. Selected abroad, row_3.
    # unknown-es: blocked F; abroad F; young U; row_3 T. Selected row_3.
    cells = "/decision_table/rows"
    rules = [
        stats_rule("blocked", (0, 0, 3, 0), [cond(f"{cells}/3/inputs/1", "eq", 0, 3, 0)], row=3),
        stats_rule("abroad", (1, 1, 2, 0), [cond(f"{cells}/1/inputs/1", "in", 2, 1, 0)], row=1),
        stats_rule("young", (1, 1, 1, 1), [cond(f"{cells}/0/inputs/0", "lt", 1, 1, 1)], row=0),
        stats_rule("row_3", (3, 3, 0, 0), [], row=2, constant="true"),
    ]
    outputs = [
        {
            "output": "risk",
            "_column": 0,
            "values": [
                {"value": "very_high", "selected": 0},
                {"value": "high", "selected": 1},
                {"value": "medium", "selected": 1},
                {"value": "low", "selected": 3},
            ],
        }
    ]
    table, _ = table_basic()
    good = text(
        case_file(
            case("vip", {"customer": {"years": 7}, "order": {"total": 2000}}),
            case("new", {"customer": {"years": 1}}),
        )
    )
    packed = bundle(
        table=table["decision_table"], cases=[{"path": "cases/good.json", "digest": digest(good)}]
    )
    # vip: vip T,T (selected), loyal T; new: vip F,U -> F, loyal F.
    at = "/bundle/decision_table/rows"
    return [
        vector(
            "table",
            [CONDITIONS, OUTPUTS, COVERAGE, JSON],
            "a decision table's rules in rule order (output order here) with their rows, each"
            " input cell that is not '-' as one condition (the test under a NOT, at the cell),"
            " the output values each selection gave, and a catch-all row that cannot be FALSE"
            " or UNKNOWN left out of those measures",
            risks,
            statistics(3, 3, rules, outputs),
            cases=case_file(
                case("child-es", {"age": 10, "country": "ES"}),
                case("adult-fr", {"age": 30, "country": "FR"}),
                case("unknown-es", {"country": "ES"}),
            ),
        ),
        vector(
            "bundle",
            [CONDITIONS, BUNDLES, JSON],
            "a bundle's rules are located under /bundle, and a table without output values has"
            " no outputs to count and no output_value measure",
            packed,
            statistics(
                2,
                2,
                [
                    stats_rule(
                        "vip",
                        (1, 1, 1, 0),
                        [
                            cond(f"{at}/0/inputs/0", "gt", 1, 1, 0),
                            cond(f"{at}/0/inputs/1", "gt", 1, 0, 1),
                        ],
                        row=0,
                    ),
                    stats_rule(
                        "loyal", (0, 1, 1, 0), [cond(f"{at}/1/inputs/0", "gt", 1, 1, 0)], row=1
                    ),
                ],
                [],
                prefix="/bundle",
            ),
            files=[("cases/good.json", good)],
        ),
    ]


def constant_vectors() -> list[Any]:
    """Rules and tests that read no field, which some measures cannot reach."""
    rules = ruleset(
        "first",
        rule("a", node("and", operands=[cmp("gt", "x", 0), const(True)])),
        rule("always", const(True)),
        rule("never", node("lt", left=const(1), right=const(0))),
    )
    # x = 1: a T (selected), always T, never F; x = -1: a F, always T (selected), never F.
    # always is TRUE and never FALSE for every input, and so are their tests.
    return [
        vector(
            "constants",
            [COVERAGE, JSON],
            "a rule or a test that reads no field has one truth value, so it is left out of the"
            " total and the missed items of the measures of the other ones, and counted as"
            " unreachable there",
            rules,
            statistics(
                2,
                2,
                [
                    stats_rule(
                        "a",
                        (1, 1, 1, 0),
                        [
                            cond("/ruleset/rules/0/when/operands/0", "gt", 1, 1, 0),
                            cond(
                                "/ruleset/rules/0/when/operands/1",
                                "const",
                                2,
                                0,
                                0,
                                constant="true",
                            ),
                        ],
                    ),
                    stats_rule(
                        "always",
                        (1, 2, 0, 0),
                        [cond("/ruleset/rules/1/when", "const", 2, 0, 0, constant="true")],
                        constant="true",
                    ),
                    stats_rule(
                        "never",
                        (0, 0, 2, 0),
                        [cond("/ruleset/rules/2/when", "lt", 0, 2, 0, constant="false")],
                        constant="false",
                    ),
                ],
            ),
            cases=case_file(case("one", {"x": 1}), case("minus-one", {"x": -1})),
        ),
    ]


def partial_vectors() -> list[Any]:
    """A partial run counts the cases it ran."""
    two = ruleset("first", rule("a", cmp("gt", "x", 0)), rule("b", cmp("gt", "x", 1)))
    # x = 2: a T (selected), b T; x = 1: a T (selected), b F. The others are not run.
    return [
        vector(
            "partial",
            [STATISTICS, BATCHES],
            "a run interrupted by max_results counts only the cases it ran",
            two,
            statistics(
                2,
                2,
                [
                    stats_rule("a", (2, 2, 0, 0), [cond("/ruleset/rules/0/when", "gt", 2, 0, 0)]),
                    stats_rule("b", (0, 1, 1, 0), [cond("/ruleset/rules/1/when", "gt", 1, 1, 0)]),
                ],
            ),
            cases=case_file(*(case(f"x{x}", {"x": x}) for x in (2, 1, 0, -1))),
            limits={"max_results": 2},
        ),
    ]


def build() -> str:
    """The vector file's text."""
    vectors = [
        numbers(v)
        for v in [
            *rule_set_vectors(),
            *error_vectors(),
            *table_vectors(),
            *constant_vectors(),
            *partial_vectors(),
        ]
    ]
    return dump({"version": IntLit("1"), "area": "tests", "vectors": vectors}) + "\n"


def main() -> int:
    """Write or check the file; return the exit code."""
    mode = sys.argv[1] if len(sys.argv) > 1 else "--check"
    path = VECTORS / "tests" / "statistics.json"
    written = build()
    if mode == "--write":
        path.write_text(written, encoding="utf-8")
        print(f"wrote {path}")
        return 0
    if not path.exists() or path.read_text(encoding="utf-8") != written:
        print(f"{path}: out of date; run with --write", file=sys.stderr)
        return 1
    print("1 file, 0 problems")
    return 0


if __name__ == "__main__":
    sys.exit(main())
