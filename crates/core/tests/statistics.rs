//! The statistics and coverage of a run (`spec/tests.md`, *Statistics*):
//! their text, the JSON member of a test run, the owned cursor, and
//! properties: every count agrees with the decisions and with each test
//! evaluated on its own, and an interrupted run counts the cases it ran.

// A test: a panic is the failure report.
#![allow(clippy::unwrap_used, clippy::panic)]

use std::fmt::Write as _;

use cleanrules::cases::{
    Case, CaseFile, Cases, CoverageMeasure, Missed, RuleStatistics, Runner, Statistics,
    TestRunLimits, TestRunState,
};
use cleanrules::eval::{Options, evaluate_tree};
use cleanrules::format::{
    Document, DocumentFormat, read_document, read_node, write_ruleset, write_statistics,
    write_test_run, write_test_run_as,
};
use cleanrules::json::{Json, Limits, to_string};
use cleanrules::logic::{Kleene, Truth};
use cleanrules::rules::RuleSet;
use cleanrules::table::DecisionTable;
use cleanrules::{Map, Value};
use proptest::prelude::*;

fn parse(text: &str) -> RuleSet {
    cleanrules::text::parse(text, &Limits::default())
        .unwrap()
        .into_ruleset()
        .unwrap()
}

fn table(text: &str) -> DecisionTable {
    match read_document(text, &Limits::default()).unwrap() {
        Document::DecisionTable(table) => table,
        other => panic!("not a table: {other:?}"),
    }
}

fn file(path: &str, text: &str) -> CaseFile {
    CaseFile::read(path, text.as_bytes(), &Limits::default())
}

fn inputs(values: Vec<Value>) -> CaseFile {
    let cases = values
        .into_iter()
        .enumerate()
        .map(|(i, input)| Case::new(format!("c{i}"), input).unwrap());
    CaseFile::new(Cases::new(cases).unwrap())
}

fn map(pairs: &[(&str, Value)]) -> Value {
    let mut map = Map::new();
    for (key, value) in pairs {
        map.insert(*key, value.clone());
    }
    Value::Map(map)
}

const DISCOUNTS: &str = "HIT POLICY first
RULE vip ID 'VIP-1' IF years > 5 AND total > 1000 THEN discount = 0.15
RULE loyal IF years BETWEEN 2 AND 5 OR NOT country IN ('ES', 'PT') THEN discount = 0.05
RULE new IF years IS NULL THEN discount = 0";

const RISKS: &str = r#"{"version": 1, "decision_table": {
  "hit_policy": "output_order",
  "inputs": ["age", "country"],
  "outputs": [{"name": "risk", "values": ["very_high", "high", "medium", "low"]}, {"name": "note"}],
  "rows": [
    {"name": "young", "inputs": ["< 18", "-"], "outputs": ["'medium'", "'minor'"]},
    {"name": "abroad", "inputs": ["-", "NOT IN ('ES', 'PT')"], "outputs": ["'high'", null]},
    {"inputs": ["-", "-"], "outputs": ["'low'", null]},
    {"name": "blocked", "inputs": ["-", "'XX'"], "outputs": ["'very_high'", null]}
  ]}}"#;

const RISK_CASES: &str = r#"{"version": 1, "cases": {"items": [
  {"id": "child-es", "input": {"age": 10, "country": "ES"}, "expect": {"selected": ["young", "row_3"]}},
  {"id": "adult-fr", "input": {"age": 30, "country": "FR"}, "expect": {"selected": ["row_3"]}},
  {"id": "unknown-es", "input": {"country": "ES"}}
]}}"#;

fn discount_cases() -> CaseFile {
    file(
        "cases/discounts.json",
        r#"{"version": 1, "cases": {"items": [
          {"id": "es-vip", "input": {"years": 7, "total": 2000, "country": "ES"}, "expect": {"selected": ["VIP-1"]}},
          {"id": "fr-loyal", "input": {"years": 3, "total": 50, "country": "FR"}, "expect": {"selected": ["vip"]}},
          {"id": "unknown-years", "input": {"total": 10}},
          {"id": "es-new", "input": {"years": 1, "total": 5000, "country": "ES"}}
        ]}}"#,
    )
}

#[test]
fn a_run_counts_rules_tests_and_coverage() {
    let ruleset = parse(DISCOUNTS);
    let files = [discount_cases()];
    let run = Runner::new(&ruleset)
        .with_limits(TestRunLimits::default().with_max_explained(0))
        .with_statistics(true)
        .run(&files);
    let statistics = run.statistics.as_ref().unwrap();
    assert_eq!((statistics.cases, statistics.decisions), (4, 4));
    let counts: Vec<(&str, [usize; 4])> = statistics
        .rules
        .iter()
        .map(|r| {
            (
                r.name.as_str(),
                [r.selected, r.fired, r.not_fired, r.unknown],
            )
        })
        .collect();
    assert_eq!(
        counts,
        [
            ("vip", [1, 1, 3, 0]),
            ("loyal", [1, 1, 2, 1]),
            ("new", [1, 1, 3, 0])
        ]
    );
    let loyal = &statistics.rules[1];
    assert_eq!(loyal.conditions[1].op, "in");
    assert_eq!(
        loyal.conditions[1].pointer,
        "/ruleset/rules/1/when/operands/1/operand"
    );
    assert_eq!(loyal.conditions[1].count(Truth::True), 2);
    assert_eq!(loyal.conditions[1].column(), None);
    assert_eq!(statistics.rules[0].id.as_deref(), Some("VIP-1"));
    let unknown = &statistics.coverage()[3];
    assert_eq!(unknown.measure, CoverageMeasure::RuleUnknown);
    assert_eq!((unknown.covered, unknown.total), (1, 3));
    assert!(
        matches!(&unknown.missed[0], Missed::Rule { rule, pointer } if rule == "vip" && pointer == "/ruleset/rules/0")
    );
    // The statistics end the test run's text, after its failure.
    insta::assert_snapshot!("a_rule_set_run_with_statistics", run.to_string());
}

#[test]
fn a_table_counts_cells_and_output_values() {
    let table = table(RISKS);
    let files = [file("cases/risks.json", RISK_CASES)];
    let run = Runner::for_table(&table)
        .with_limits(TestRunLimits::default().with_max_explained(0))
        .with_statistics(true)
        .run(&files);
    let statistics = run.statistics.as_ref().unwrap();
    let rows: Vec<(&str, Option<usize>)> = statistics
        .rules
        .iter()
        .map(|r| (r.name.as_str(), r.row))
        .collect();
    assert_eq!(
        rows,
        [
            ("blocked", Some(3)),
            ("abroad", Some(1)),
            ("young", Some(0)),
            ("row_3", Some(2))
        ]
    );
    // A NOT cell counts its test, as a rule set does, at the cell.
    let abroad = &statistics.rules[1].conditions[0];
    assert_eq!((abroad.op, abroad.column()), ("in", Some(1)));
    assert_eq!((abroad.true_count, abroad.false_count), (2, 1));
    assert_eq!(statistics.rules[1].pointer, "/decision_table/rows/1");
    // The catch-all row reads no field: it cannot be FALSE or UNKNOWN.
    let catch_all = &statistics.rules[3];
    assert_eq!(catch_all.constant(), Some(Truth::True));
    assert_eq!(catch_all.conditions.len(), 0);
    let outputs = statistics.outputs.as_ref().unwrap();
    let given: Vec<usize> = outputs[0].values.iter().map(|v| v.selected).collect();
    assert_eq!(given, [0, 1, 1, 3]);
    let coverage = statistics.coverage();
    let last = coverage.last().unwrap();
    assert_eq!(last.measure, CoverageMeasure::OutputValue);
    assert!(matches!(
        &last.missed[0],
        Missed::OutputValue { output, value: Value::String(s), pointer } if output == "risk" && s == "very_high" && pointer == "/decision_table/outputs/0/values/0"
    ));
    insta::assert_snapshot!("a_table_run_with_statistics", run.to_string());
}

#[test]
fn a_bundle_locates_its_cells_under_bundle() {
    use cleanrules::bundle::{Bundle, BundleId, CaseFileRef, Revision};
    let bytes = RISK_CASES.as_bytes().to_vec();
    let bundle = Bundle::new(
        table(RISKS),
        BundleId::new("acme/risks").unwrap(),
        Revision::new("1").unwrap(),
    )
    .with_cases([CaseFileRef::new("cases/risks.json", &bytes).unwrap()])
    .unwrap();
    let files = CaseFile::bundle_files(&bundle, |_| Ok::<_, String>(bytes.clone())).unwrap();
    let run = Runner::for_bundle(&bundle)
        .with_statistics(true)
        .run(&files);
    let statistics = run.statistics.unwrap();
    assert_eq!(
        statistics.rules[0].conditions[0].pointer,
        "/bundle/decision_table/rows/3/inputs/1"
    );
    assert!(
        statistics.to_string().contains(
            "\n    eq at row blocked, input country (/bundle/decision_table/rows/3/inputs/1)\n"
        ),
        "{statistics}"
    );
}

#[test]
fn a_run_not_asked_counts_nothing() {
    let ruleset = parse(DISCOUNTS);
    let files = [discount_cases()];
    let runner = Runner::new(&ruleset);
    assert!(!runner.gathers_statistics());
    let run = runner.run(&files);
    assert!(run.statistics.is_none());
    assert!(!to_string(&write_test_run(&run)).contains("\"statistics\""));
    assert!(!run.to_string().contains("Statistics"));
    assert!(runner.statistics(&TestRunState::new()).is_none());
    // Asked and then not.
    let runner = Runner::new(&ruleset)
        .with_statistics(true)
        .with_statistics(false);
    assert!(runner.run(&files).statistics.is_none());
}

#[test]
fn the_owned_cursor_counts_as_it_goes() {
    let ruleset = parse(DISCOUNTS);
    let files = [discount_cases()];
    let runner = Runner::new(&ruleset).with_statistics(true);
    let mut state = TestRunState::new();
    let before = runner.statistics(&state).unwrap();
    assert_eq!((before.cases, before.decisions), (0, 0));
    assert_eq!(before.rules.len(), 3);
    let mut results = Vec::new();
    while let Some(result) = runner.next_result(&files, &mut state, || false) {
        results.push(result);
        let so_far = runner.statistics(&state).unwrap();
        assert_eq!(so_far.cases, results.len());
    }
    let run = runner.test_run(&files, state, results);
    let full = runner.run(&files);
    assert_eq!(
        to_string(&write_statistics(run.statistics.as_ref().unwrap())),
        to_string(&write_statistics(full.statistics.as_ref().unwrap()))
    );
    // The iterator gives the same counts.
    let mut iter = runner.iter(&files);
    assert_eq!(iter.statistics().unwrap().cases, 0);
    let first = iter.next();
    assert!(first.is_some());
    assert_eq!(iter.statistics().unwrap().cases, 1);
}

#[test]
fn statistics_are_written_in_every_output_format() {
    let ruleset = parse(DISCOUNTS);
    let files = [discount_cases()];
    let run = Runner::new(&ruleset).with_statistics(true).run(&files);
    let json = write_test_run_as(&run, DocumentFormat::Json).unwrap();
    assert!(json.contains(
        r#""statistics":{"cases":4,"decisions":4,"rules":[{"name":"vip","id":"VIP-1","pointer":"/ruleset/rules/0","selected":1"#
    ));
    let text = write_test_run_as(&run, DocumentFormat::Text).unwrap();
    assert!(text.contains("Statistics of 4 cases: 4 decisions"));
    #[cfg(feature = "yaml")]
    assert!(
        write_test_run_as(&run, DocumentFormat::Yaml)
            .unwrap()
            .contains("statistics:")
    );
}

#[test]
fn an_empty_run_misses_everything() {
    let ruleset = parse("HIT POLICY first\nRULE a IF x > 0");
    let files = [inputs(Vec::new())];
    let run = Runner::new(&ruleset).with_statistics(true).run(&files);
    let statistics = run.statistics.unwrap();
    for measure in statistics.coverage() {
        assert_eq!((measure.covered, measure.total), (0, 1), "{measure:?}");
    }
    insta::assert_snapshot!("an_empty_run", statistics.to_string());
}

#[test]
fn items_that_read_no_field_are_unreachable() {
    let ruleset = parse(
        "HIT POLICY first\nRULE a IF x > 0 AND TRUE\nRULE always IF TRUE\nRULE never IF 1 < 0",
    );
    let x = |n: i64| map(&[("x", Value::Int(n))]);
    let files = [inputs(vec![x(1), x(-1)])];
    let statistics = Runner::new(&ruleset)
        .with_statistics(true)
        .run(&files)
        .statistics
        .unwrap();
    let constants: Vec<Option<Truth>> = statistics
        .rules
        .iter()
        .map(RuleStatistics::constant)
        .collect();
    assert_eq!(constants, [None, Some(Truth::True), Some(Truth::False)]);
    let counts: Vec<(&str, usize, usize, usize)> = statistics
        .coverage()
        .iter()
        .map(|m| (m.measure.as_str(), m.covered, m.total, m.unreachable))
        .collect();
    assert_eq!(
        counts,
        [
            ("rule_selected", 2, 2, 1),
            ("rule_fired", 2, 2, 1),
            ("rule_not_fired", 2, 2, 1),
            ("rule_unknown", 0, 1, 2),
            ("condition_true", 3, 3, 1),
            ("condition_false", 2, 2, 2),
            ("condition_unknown", 0, 1, 3),
        ]
    );
    let text = statistics.to_string();
    assert!(
        text.contains("  rules unknown: 0 of 1, 2 unreachable\n    a\n"),
        "{text}"
    );
    let json = to_string(&write_statistics(&statistics));
    assert!(json.contains(r#"{"measure":"rule_unknown","covered":0,"total":1,"unreachable":2,"missed":[{"rule":"a","pointer":"/ruleset/rules/0"}]}"#), "{json}");
    assert!(
        json.contains(
            r#"{"measure":"rule_fired","covered":2,"total":2,"unreachable":1,"missed":[]}"#
        ),
        "{json}"
    );
}

/// A state moved by a runner that does not gather statistics has none to
/// give: a caller's mistake, which debug builds assert.
#[test]
#[cfg_attr(debug_assertions, should_panic(expected = "results were not counted"))]
fn a_state_moved_without_counting_has_no_statistics() {
    let ruleset = parse(DISCOUNTS);
    let files = [discount_cases()];
    let mut state = TestRunState::new();
    assert!(
        Runner::new(&ruleset)
            .next_result(&files, &mut state, || false)
            .is_some()
    );
    let counting = Runner::new(&ruleset).with_statistics(true);
    // It never starts counting midway.
    assert!(counting.next_result(&files, &mut state, || false).is_some());
    assert!(counting.statistics(&state).is_none());
}

#[test]
fn measures_have_names() {
    for measure in CoverageMeasure::ALL {
        assert_eq!(CoverageMeasure::from_name(measure.as_str()), Some(*measure));
    }
    assert_eq!(CoverageMeasure::from_name("rules_selected"), None);
    assert_eq!(CoverageMeasure::ALL.len(), 8);
}

// --- Properties ---------------------------------------------------------------------------

/// A rule set in the rule syntax over `x`, `y` and `s`, under any policy.
fn ruleset_text() -> impl Strategy<Value = String> {
    let atom = prop_oneof![
        (
            prop::sample::select(vec!["x", "y"]),
            prop::sample::select(vec!["=", "<", ">", ">="]),
            0..4i64
        )
            .prop_map(|(f, op, c)| format!("{f} {op} {c}")),
        prop::sample::select(vec!["x", "y"]).prop_map(|f| format!("{f} IS NULL")),
        Just("x > y".to_owned()),
        Just("x BETWEEN 1 AND 2".to_owned()),
        Just("y IN (0, 3)".to_owned()),
        Just("s STARTS WITH 'a'".to_owned()),
        Just("TRUE".to_owned()),
        Just("1 > 2".to_owned()),
    ];
    let condition = atom.prop_recursive(3, 8, 2, |inner| {
        prop_oneof![
            inner.clone().prop_map(|c| format!("NOT ({c})")),
            (inner.clone(), inner.clone()).prop_map(|(a, b)| format!("({a}) AND ({b})")),
            (inner.clone(), inner).prop_map(|(a, b)| format!("({a}) OR ({b})")),
        ]
    });
    let policy = prop::sample::select(vec![
        "first",
        "priority",
        "unique",
        "any",
        "rule_order",
        "collect",
    ]);
    let rule = (condition, 0..3i64, 0..2i64);
    (policy, any::<bool>(), prop::collection::vec(rule, 0..5)).prop_map(
        |(policy, strict, rules)| {
            let mut text = format!("HIT POLICY {policy}\n");
            if strict {
                text.push_str("STRICT\n");
            }
            for (i, (when, priority, out)) in rules.into_iter().enumerate() {
                let _ = writeln!(
                    text,
                    "RULE r{i} PRIORITY {priority}\nIF {when}\nTHEN v = {out}"
                );
            }
            text
        },
    )
}

fn input_value() -> impl Strategy<Value = Value> {
    let number = prop_oneof![
        Just(None),
        Just(Some(Value::Null(cleanrules::NullCause::Null))),
        (0..5i64).prop_map(|n| Some(Value::Int(n))),
        Just(Some(Value::String("a".into()))),
    ];
    let text = prop_oneof![
        Just(None),
        Just(Some(Value::String("abc".into()))),
        Just(Some(Value::String("b".into()))),
    ];
    (number.clone(), number, text).prop_map(|(x, y, s)| {
        let mut map = Map::new();
        for (key, value) in [("x", x), ("y", y), ("s", s)] {
            if let Some(value) = value {
                map.insert(key, value);
            }
        }
        Value::Map(map)
    })
}

/// The node at a JSON Pointer of a document.
fn at<'j>(json: &'j Json, pointer: &str) -> &'j Json {
    pointer
        .split('/')
        .skip(1)
        .fold(json, |json, step| match json {
            Json::Object(members) => &members.iter().find(|(k, _)| k == step).unwrap().1,
            Json::Array(items) => &items[step.parse::<usize>().unwrap()],
            other => panic!("no {step} in {other:?}"),
        })
}

/// The counts the statistics must give, from the decisions and from each
/// test evaluated on its own at its pointer.
fn check_against_decisions(ruleset: &RuleSet, values: &[Value], statistics: &Statistics) {
    let document = Json::Object(vec![("ruleset".into(), write_ruleset(ruleset))]);
    let decisions: Vec<_> = values
        .iter()
        .filter_map(|v| ruleset.evaluate(v).ok().map(|d| (v, d)))
        .collect();
    assert_eq!(statistics.cases, values.len());
    assert_eq!(statistics.decisions, decisions.len());
    assert_eq!(statistics.rules.len(), ruleset.rules().len());
    for (i, rule) in statistics.rules.iter().enumerate() {
        let mut expected = [0; 4];
        for (_, decision) in &decisions {
            let result = &decision.results()[i];
            if decision.selected_positions().contains(&i) {
                expected[0] += 1;
            }
            match (result.fired, result.when.truth()) {
                (true, _) => expected[1] += 1,
                (false, Some(Truth::False)) => expected[2] += 1,
                _ => expected[3] += 1,
            }
        }
        assert_eq!(
            [rule.selected, rule.fired, rule.not_fired, rule.unknown],
            expected,
            "rule {}",
            rule.name
        );
        assert!(rule.selected <= rule.fired);
        for condition in &rule.conditions {
            let json = at(&document, &condition.pointer);
            let node = read_node(json, "", &Limits::default()).unwrap();
            assert_eq!(node.tag(), condition.op);
            assert!(!matches!(condition.op, "and" | "or" | "not"));
            let mut counts = [0; 3];
            for (input, _) in &decisions {
                let options = Options {
                    strict: ruleset.strict(),
                    rule: None,
                };
                let truth = evaluate_tree(&node, input, &Kleene, options)
                    .unwrap()
                    .truth()
                    .unwrap_or(Truth::Unknown);
                counts[match truth {
                    Truth::True => 0,
                    Truth::False => 1,
                    Truth::Unknown => 2,
                }] += 1;
            }
            assert_eq!(
                [
                    condition.true_count,
                    condition.false_count,
                    condition.unknown_count
                ],
                counts,
                "{}",
                condition.pointer
            );
        }
    }
    let tests: usize = statistics.rules.iter().map(|r| r.conditions.len()).sum();
    for measure in statistics.coverage() {
        assert_eq!(measure.covered + measure.missed.len(), measure.total);
        let items = if measure.measure.as_str().starts_with("rule_") {
            statistics.rules.len()
        } else {
            tests
        };
        assert_eq!(measure.total + measure.unreachable, items, "{measure:?}");
    }
    // A rule or test that reads no field has one truth value: the one it is
    // counted with, in every decision.
    for rule in &statistics.rules {
        if let Some(truth) = rule.constant() {
            let count = match truth {
                Truth::True => rule.fired,
                Truth::False => rule.not_fired,
                Truth::Unknown => rule.unknown,
            };
            assert_eq!(count, statistics.decisions, "{}", rule.name);
        }
        for condition in &rule.conditions {
            if let Some(truth) = condition.constant() {
                assert_eq!(condition.count(truth), statistics.decisions);
            }
        }
    }
}

/// The number of tests of a rule set, counted on its JSON form.
fn tests_in(json: &Json) -> usize {
    match json {
        Json::Object(members) => {
            let op = members.iter().find(|(k, _)| k == "op").map(|(_, v)| v);
            match op {
                Some(Json::String(op)) if op == "and" || op == "or" => members
                    .iter()
                    .find(|(k, _)| k == "operands")
                    .map_or(0, |(_, v)| tests_in(v)),
                Some(Json::String(op)) if op == "not" => members
                    .iter()
                    .find(|(k, _)| k == "operand")
                    .map_or(0, |(_, v)| tests_in(v)),
                _ => 1,
            }
        }
        Json::Array(items) => items.iter().map(tests_in).sum(),
        _ => 0,
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(300))]

    #[test]
    fn counts_agree_with_the_decisions_and_each_test(
        text in ruleset_text(),
        values in prop::collection::vec(input_value(), 0..8),
        stop in 0..9usize,
    ) {
        let ruleset = parse(&text);
        let files = [inputs(values.clone())];
        let runner = Runner::new(&ruleset).with_statistics(true);
        let statistics = runner.run(&files).statistics.unwrap();
        check_against_decisions(&ruleset, &values, &statistics);
        // Every test of every rule is listed.
        let listed: usize = statistics.rules.iter().map(|r| r.conditions.len()).sum();
        let written = write_ruleset(&ruleset);
        let rules = at(&written, "/rules");
        let whens: Vec<&Json> = match rules {
            Json::Array(items) => items.iter().map(|r| at(r, "/when")).collect(),
            _ => Vec::new(),
        };
        prop_assert_eq!(listed, whens.iter().map(|w| tests_in(w)).sum::<usize>());
        // A run interrupted after `stop` results counts the cases it ran.
        let partial = runner
            .clone()
            .with_limits(TestRunLimits::default().with_max_results(stop))
            .run(&files)
            .statistics
            .unwrap();
        let ran = stop.min(values.len());
        check_against_decisions(&ruleset, &values[..ran], &partial);
    }
}
