//! The case runner (`spec/tests.md`, *Running cases*, *Test runs*): the
//! example case file against the example rules, every kind of result, the
//! case files of a bundle, interruptions, the text and JSON forms, and
//! properties: a case built from what a decision gives passes, and a run
//! interrupted anywhere gives a prefix of the full run, marked partial.

// A test: a panic is the failure report.
#![allow(clippy::unwrap_used, clippy::panic)]

use std::cell::Cell;
use std::fmt::Write as _;

use cleanrules::bundle::{Bundle, BundleId, Revision};
use cleanrules::cases::{
    Case, CaseFile, CaseStatus, Cases, Expectation, ExpectationKey, ExpectedOutput, Interruption,
    Mismatch, RulesSource, Runner, TestRun, TestRunLimits, TestRunState,
};
use cleanrules::format::{
    Document, DocumentFormat, read_document, write_case_result, write_test_run, write_test_run_as,
};
use cleanrules::json::{Limits, to_string};
use cleanrules::rules::RuleSet;
use cleanrules::table::DecisionTable;
use cleanrules::{ErrorCode, Map, Value};
use proptest::prelude::*;

const RULES: &str = include_str!("../../../docs/examples/discounts.json");
const CASES: &str = include_str!("../../../docs/examples/cases/discounts.json");

fn rules(text: &str) -> RuleSet {
    match read_document(text, &Limits::default()).unwrap() {
        Document::RuleSet(ruleset) => ruleset,
        other => panic!("not a rule set: {other:?}"),
    }
}

fn parse(text: &str) -> RuleSet {
    cleanrules::text::parse(text, &Limits::default())
        .unwrap()
        .into_ruleset()
        .unwrap()
}

fn cases(text: &str) -> Cases {
    match read_document(text, &Limits::default()).unwrap() {
        Document::Cases(cases) => cases,
        other => panic!("not a case file: {other:?}"),
    }
}

fn file(path: &str, text: &str) -> CaseFile {
    CaseFile::read(path, text.as_bytes(), &Limits::default())
}

/// The JSON of a test run, with this engine's version as `<version>`, so
/// that it does not change with each release.
fn json(run: &TestRun) -> String {
    versionless(&to_string(&write_test_run(run)))
}

fn versionless(text: &str) -> String {
    let version = format!(r#""version":"{}""#, cleanrules::bundle::ENGINE_VERSION);
    text.replace(&version, r#""version":"<version>""#)
}

#[test]
fn the_example_cases_pass_against_the_example_rules() {
    let ruleset = rules(RULES);
    let files = [file("cases/discounts.json", CASES)];
    let run = Runner::new(&ruleset).run(&files);
    assert!(run.all_passed(), "{run}");
    assert_eq!(run.summary.total(), 4);
    assert_eq!(run.source, RulesSource::RuleSet);
    let ids: Vec<&str> = run.results.iter().filter_map(|r| r.id.as_deref()).collect();
    assert_eq!(
        ids,
        [
            "vip-gets-15",
            "blocked-gets-nothing",
            "newsletter-is-loyal",
            "new-customer-gets-nothing"
        ]
    );
    assert!(
        run.results
            .iter()
            .all(|r| r.path.as_deref() == Some("cases/discounts.json"))
    );
    insta::assert_snapshot!("the_example_run", run.to_string());
}

/// The example rules with a different vip discount, and cases that fail in
/// every way.
const FAILING: &str = r#"{"version":1,"cases":{"items":[
  {"id":"every-key","input":{"customer":{"years":7},"order":{"total":1500}},
   "expect":{"selected":["loyal"],"output":{"discount":{"$decimal":"0.05"}},"outputs":[{"discount":{"$decimal":"0.05"}}],"unknown":[]}},
  {"id":"no-error","input":{"customer":{"years":1}},"expect":{"error":{"code":"hit_policy_error"}}},
  {"id":"passes","input":{"customer":{"years":7,"blocked":true}},"expect":{"selected":["blocked"]}},
  {"id":"nothing-expected","input":{},"expect":{"output":{}}}
]}}"#;

#[test]
fn a_failure_lists_every_key_in_order_and_is_explained() {
    let ruleset = rules(RULES);
    let run = Runner::new(&ruleset).run(&[CaseFile::new(cases(FAILING))]);
    assert_eq!(
        (run.summary.passed, run.summary.failed, run.summary.error),
        (1, 3, 0)
    );
    let keys: Vec<ExpectationKey> = run.results[0]
        .mismatches
        .iter()
        .map(Mismatch::key)
        .collect();
    assert_eq!(
        keys,
        [
            ExpectationKey::Selected,
            ExpectationKey::Output,
            ExpectationKey::Outputs,
            ExpectationKey::Unknown
        ]
    );
    assert!(run.results[0].explanation_text.is_some());
    assert!(run.results[0].path.is_none());
    assert!(matches!(
        &run.results[1].mismatches[..],
        [Mismatch::Error {
            expected: ErrorCode::HitPolicyError,
            actual: None
        }]
    ));
    // `{}` fails when no rule is selected: the output is null.
    assert!(matches!(
        &run.results[3].mismatches[..],
        [Mismatch::Output { actual: None, .. }]
    ));
    insta::assert_snapshot!("a_failing_run", run.to_string());
    insta::assert_snapshot!(
        "a_failing_run_json",
        versionless(&write_test_run_as(&run, DocumentFormat::Json).unwrap())
    );
    assert!(!run.all_passed());
}

#[test]
fn failures_are_explained_up_to_the_limit() {
    let ruleset = rules(RULES);
    let files = [CaseFile::new(cases(FAILING))];
    let explained = |max: usize| {
        let limits = TestRunLimits::default().with_max_explained(max);
        let run = Runner::new(&ruleset).with_limits(limits).run(&files);
        run.results
            .iter()
            .filter(|r| r.explanation_text.is_some())
            .count()
    };
    assert_eq!(explained(0), 0);
    assert_eq!(explained(1), 1);
    assert_eq!(explained(5), 3);
    let first = Runner::new(&ruleset)
        .with_limits(TestRunLimits::default().with_max_explained(1))
        .run(&files);
    assert!(
        first.results[0].explanation_text.is_some(),
        "the first in order"
    );
    // Explaining later evaluates the case again, and gives the same text.
    let case = &cases(FAILING).items()[0].clone();
    assert_eq!(
        Runner::new(&ruleset).explain(case).unwrap(),
        first.results[0].explanation_text.clone().unwrap()
    );
}

#[test]
fn errors_are_results_and_expected_errors_pass() {
    let strict =
        parse("HIT POLICY unique\nSTRICT\nRULE a IF x > 1 THEN v = 1\nRULE b IF x > 2 THEN v = 2");
    let text = r#"{"version":1,"cases":{"items":[
      {"id":"missing","input":{}},
      {"id":"expected-missing","input":{},"expect":{"error":{"code":"missing_field"}}},
      {"id":"other-code","input":{},"expect":{"error":{"code":"type_mismatch"}}},
      {"id":"both","input":{"x":3},"expect":{"error":{"code":"hit_policy_error"}}},
      {"id":"one","input":{"x":2},"expect":{"selected":["a"],"output":{"v":1}}}
    ]}}"#;
    let run = Runner::new(&strict).run(&[CaseFile::new(cases(text))]);
    let statuses: Vec<CaseStatus> = run.results.iter().map(|r| r.status).collect();
    assert_eq!(
        statuses,
        [
            CaseStatus::Error,
            CaseStatus::Passed,
            CaseStatus::Failed,
            CaseStatus::Passed,
            CaseStatus::Passed
        ]
    );
    assert_eq!(
        run.results[0].error.as_ref().map(cleanrules::Error::code),
        Some(ErrorCode::MissingField)
    );
    assert!(matches!(
        &run.results[2].mismatches[..],
        [Mismatch::Error {
            expected: ErrorCode::TypeMismatch,
            actual: Some(_)
        }]
    ));
    assert!(
        run.results[2].explanation_text.is_none(),
        "no decision to explain"
    );
    assert!(run.strict);
    insta::assert_snapshot!("a_run_with_errors", run.to_string());
}

#[test]
fn a_decision_without_a_single_output_fails_output() {
    let collect = parse("HIT POLICY rule_order\nRULE a IF x > 1 THEN v = 1");
    let text = r#"{"version":1,"cases":{"items":[
      {"id":"one","input":{"x":2},"expect":{"output":{"v":1},"outputs":[{"v":1}]}}
    ]}}"#;
    let run = Runner::new(&collect).run(&[CaseFile::new(cases(text))]);
    let mismatch = &run.results[0].mismatches[0];
    assert_eq!(
        mismatch.reason().as_deref(),
        Some("the decision has no single output (rule_order)")
    );
    assert_eq!(run.results[0].mismatches.len(), 1, "outputs matches");
    let sum = parse(
        "HIT POLICY collect\nAGGREGATE sum\nRULE a IF x > 1 THEN v = 1\nRULE b IF x > 0 THEN v = 2",
    );
    let text = r#"{"version":1,"cases":{"items":[
      {"id":"sum","input":{"x":2},"expect":{"output":{"v":3},"outputs":[{"v":1},{"v":2}]}},
      {"id":"none","input":{"x":0},"expect":{"output":null,"outputs":[]}}
    ]}}"#;
    assert!(
        Runner::new(&sum)
            .run(&[CaseFile::new(cases(text))])
            .all_passed()
    );
}

#[test]
fn values_match_by_canonical_bytes() {
    let ruleset = parse(
        "RULE r IF TRUE THEN i = 1, f = f, d = 0.150, z = z, n = x, l = l, dt = DATETIME '2026-01-01T00:00:00+01:00'",
    );
    let expect = |output: &str| {
        let text = format!(
            r#"{{"version":1,"cases":{{"items":[{{"id":"c","input":{{"f":1.0,"z":-0.0,"l":[1,"a"]}},"expect":{{"output":{output}}}}}]}}}}"#
        );
        Runner::new(&ruleset)
            .run(&[CaseFile::new(cases(&text))])
            .all_passed()
    };
    assert!(expect(
        r#"{"i":1,"f":1.0,"d":{"$decimal":"0.150"},"z":-0.0}"#
    ));
    assert!(expect(r#"{"f":{"$float":1}}"#));
    assert!(
        expect(r#"{"n":null,"l":[1,"a"]}"#),
        "nulls match whatever their reason"
    );
    assert!(expect(
        r#"{"dt":{"$datetime":"2026-01-01T00:00:00+01:00"}}"#
    ));
    assert!(!expect(r#"{"i":1.0}"#), "an int is not a float");
    assert!(!expect(r#"{"f":1}"#), "a float is not an int");
    assert!(
        !expect(r#"{"d":{"$decimal":"0.15"}}"#),
        "decimals keep their scale"
    );
    assert!(!expect(r#"{"z":0.0}"#), "the sign of zero");
    assert!(
        !expect(r#"{"dt":{"$datetime":"2025-12-31T23:00:00Z"}}"#),
        "the offset"
    );
    assert!(!expect(r#"{"l":["a",1]}"#), "lists in order");
    assert!(!expect(r#"{"missing":null}"#), "the key must be there");
}

#[test]
fn files_that_cannot_be_read_are_one_error_each() {
    let ruleset = rules(RULES);
    let files = [
        file("cases/syntax.json", "{"),
        file("cases/rules.json", RULES),
        file(
            "cases/twice.json",
            r#"{"version":1,"cases":{"items":[{"id":"a","input":{}},{"id":"a","input":{}}]}}"#,
        ),
        file("cases/good.json", CASES),
        file(
            "cases/version.json",
            r#"{"version":2,"cases":{"items":[]}}"#,
        ),
    ];
    let run = Runner::new(&ruleset).run(&files);
    assert_eq!(run.summary.total(), 8);
    assert_eq!((run.summary.passed, run.summary.error), (4, 4));
    let errors: Vec<(Option<&str>, ErrorCode, Option<String>)> = run
        .results
        .iter()
        .filter(|r| r.id.is_none())
        .map(|r| {
            let error = r.error.as_ref().unwrap();
            (
                r.path.as_deref(),
                error.code(),
                error.context().pointer.clone(),
            )
        })
        .collect();
    assert_eq!(
        errors,
        [
            (Some("cases/syntax.json"), ErrorCode::FormatError, None),
            (
                Some("cases/rules.json"),
                ErrorCode::FormatError,
                Some(String::new())
            ),
            (
                Some("cases/twice.json"),
                ErrorCode::FormatError,
                Some("/cases/items/1/id".into())
            ),
            (
                Some("cases/version.json"),
                ErrorCode::UnsupportedVersion,
                Some("/version".into())
            ),
        ]
    );
    insta::assert_snapshot!("a_run_with_unreadable_files", run.to_string());
    let only = Runner::new(&ruleset).run(&files[..1]);
    assert_eq!(
        to_string(&write_case_result(&only.results[0])),
        to_string(&write_case_result(&run.results[0])),
        "the same error each time the file is run"
    );
}

#[cfg(feature = "yaml")]
#[test]
fn yaml_case_files_are_read_by_their_extension() {
    let ruleset = parse("RULE r IF x > 1 THEN v = 1");
    let yaml = "version: 1\ncases:\n  items:\n    - id: one\n      input: {x: 2}\n      expect: {selected: [r]}\n";
    for path in ["cases/a.yaml", "cases/a.yml", "A.YML"] {
        assert!(Runner::new(&ruleset).run(&[file(path, yaml)]).all_passed());
    }
    for path in ["cases/a.json", "cases/yaml", "yaml.d/a"] {
        let run = Runner::new(&ruleset).run(&[file(path, yaml)]);
        assert_eq!(run.summary.error, 1, "{path} is read as JSON");
    }
}

#[test]
fn a_bundle_runs_its_case_files_checked_against_their_digests() {
    let table = DecisionTable::from_csv(
        "HIT POLICY,unique\nyears,THEN discount\n> 5,0.15\n<= 5,0\n",
        &Limits::default(),
    );
    let table = match table {
        Ok(table) => table,
        Err(error) => panic!("{error}"),
    };
    let good = r#"{"version":1,"cases":{"items":[{"id":"vip","input":{"years":7},"expect":{"output":{"discount":{"$decimal":"0.15"}}}}]}}"#;
    let other = r#"{"version":1,"cases":{"items":[{"id":"new","input":{"years":1},"expect":{"output":{"discount":0}}}]}}"#;
    let refs = [
        cleanrules::bundle::CaseFileRef::new("cases/good.json", good.as_bytes()).unwrap(),
        cleanrules::bundle::CaseFileRef::new("cases/other.json", other.as_bytes()).unwrap(),
    ];
    let bundle = Bundle::new(
        table,
        BundleId::new("acme/discounts").unwrap(),
        Revision::new("1").unwrap(),
    )
    .with_cases(refs)
    .unwrap();
    let files = CaseFile::bundle_files(&bundle, |r| {
        Ok::<_, ()>(match r.path() {
            "cases/good.json" => good.as_bytes().to_vec(),
            _ => b"{\"changed\": true}".to_vec(),
        })
    })
    .unwrap();
    let run = Runner::for_bundle(&bundle)
        .run(&files)
        .with_ruleset_digest(true);
    assert_eq!(run.source, RulesSource::DecisionTable);
    assert_eq!(run.ruleset_digest, Some(bundle.ruleset_digest()));
    assert_eq!((run.summary.passed, run.summary.error), (1, 1));
    let error = run.results[1].error.as_ref().unwrap();
    assert_eq!(error.code(), ErrorCode::DecisionMismatch);
    assert_eq!(
        error.context().pointer.as_deref(),
        Some("/bundle/cases/1/digest")
    );
    assert_eq!(run.results[1].path.as_deref(), Some("cases/other.json"));
    // The caller's error, when it cannot open a file, is not a result.
    let missing = CaseFile::bundle_files(&bundle, |_| Err("no such file"));
    assert_eq!(missing.unwrap_err(), "no such file");
    // A table's own policy names the output it lacks.
    let table = DecisionTable::from_csv(
        "HIT POLICY,output_order\nyears,THEN discount\n> 5,0.15\n",
        &Limits::default(),
    )
    .unwrap();
    let text = r#"{"version":1,"cases":{"items":[{"id":"o","input":{"years":7},"expect":{"output":null}}]}}"#;
    let run = Runner::for_table(&table).run(&[CaseFile::new(cases(text))]);
    assert_eq!(
        run.results[0].mismatches[0].reason().as_deref(),
        Some("the decision has no single output (output_order)")
    );
    assert!(json(&run).contains(r#""hit_policy":"output_order""#));
}

/// Three files: two cases, an unreadable file, two cases.
fn three_files() -> Vec<CaseFile> {
    let two = |a: &str, b: &str| {
        CaseFile::new(cases(&format!(
            r#"{{"version":1,"cases":{{"items":[{{"id":"{a}","input":{{"x":2}}}},{{"id":"{b}","input":{{}}}}]}}}}"#
        )))
    };
    vec![
        two("a", "b").with_path("one.json"),
        file("bad.json", "[]"),
        two("c", "d").with_path("two.json"),
    ]
}

#[test]
fn limits_interrupt_a_run_before_an_item() {
    let ruleset = parse("RULE r IF x > 1 THEN v = 1\nRULE s IF x > 5 THEN v = 2");
    let files = three_files();
    let run_with = |limits: TestRunLimits| Runner::new(&ruleset).with_limits(limits).run(&files);
    let full = run_with(TestRunLimits::default());
    assert_eq!(full.summary.total(), 5);
    assert!(!full.summary.is_partial());

    let cases = run_with(TestRunLimits::default().with_max_results(3));
    assert_eq!(cases.summary.total(), 3);
    let interrupted = cases.summary.interrupted.unwrap();
    assert_eq!(
        (interrupted.reason, interrupted.remaining),
        (Interruption::MaxResults, 2)
    );
    // Each case costs two steps (two rules); the unreadable file none.
    let steps = run_with(TestRunLimits::default().with_max_steps(4));
    assert_eq!(steps.summary.total(), 3, "a, b and the file");
    let interrupted = steps.summary.interrupted.unwrap();
    assert_eq!(
        (interrupted.reason, interrupted.remaining),
        (Interruption::MaxSteps, 2)
    );
    let none = run_with(
        TestRunLimits::default()
            .with_max_steps(0)
            .with_max_results(0),
    );
    assert_eq!(
        none.summary.interrupted.unwrap().reason,
        Interruption::MaxResults,
        "max_cases is checked first"
    );
    // A run that gave its last result is complete, whatever the limits.
    let exact = run_with(
        TestRunLimits::default()
            .with_max_results(5)
            .with_max_steps(8),
    );
    assert!(!exact.summary.is_partial());
    insta::assert_snapshot!("a_partial_run", cases.to_string());
    assert!(json(&cases).contains(
        r#""summary":{"total":3,"passed":2,"failed":0,"error":1,"partial":true,"interrupted":{"reason":"max_results","remaining":2}}"#
    ));
    assert!(json(&cases).contains(r#""limits":{"max_explained":5,"max_results":3}"#));
}

#[test]
fn the_caller_cancels_between_results() {
    let ruleset = parse("RULE r IF x > 1 THEN v = 1");
    let files = three_files();
    let runner = Runner::new(&ruleset);
    let asked = Cell::new(0);
    let run = runner.run_until(&files, || {
        asked.set(asked.get() + 1);
        asked.get() > 2
    });
    assert_eq!(run.summary.total(), 2);
    let interrupted = run.summary.interrupted.unwrap();
    assert_eq!(
        (interrupted.reason, interrupted.remaining),
        (Interruption::Cancelled, 3)
    );
    // Not asked once the last result is given.
    let asked = Cell::new(0);
    let run = runner.run_until(&files, || {
        asked.set(asked.get() + 1);
        false
    });
    assert_eq!((asked.get(), run.summary.is_partial()), (5, false));

    let mut iter = runner.iter(&files);
    assert_eq!(iter.remaining(), 5);
    let first = iter.next().unwrap();
    assert_eq!(first.id.as_deref(), Some("a"));
    assert_eq!(iter.summary().total(), 1);
    iter.cancel();
    assert!(iter.is_finished());
    assert!(iter.next().is_none());
    assert_eq!(iter.summary().interrupted.unwrap().remaining, 4);
    let run = iter.into_test_run(vec![first]);
    assert_eq!(run.results.len(), 1);
    assert!(run.summary.is_partial());

    // Cancelling a finished run changes nothing.
    let mut iter = runner.iter(&files);
    assert_eq!(iter.by_ref().count(), 5);
    iter.cancel();
    assert!(!iter.summary().is_partial());
    assert!(format!("{iter:?}").starts_with("CaseRun"));
}

#[test]
fn a_single_case_runs_and_explains() {
    let ruleset = parse("RULE r IF x > 1 THEN v = 1");
    let case = Case::new("c", Value::Map(Map::new()))
        .unwrap()
        .with_expect(Expectation::new().with_selected(["r"]).unwrap());
    let runner = Runner::new(&ruleset);
    let result = runner.run_case(&case);
    assert_eq!(result.status, CaseStatus::Failed);
    assert!(result.explanation_text.is_some());
    let quiet = runner
        .clone()
        .with_limits(TestRunLimits::default().with_max_explained(0));
    assert_eq!(quiet.limits().max_explained, 0);
    assert!(quiet.run_case(&case).explanation_text.is_none());
    let strict = parse("STRICT\nRULE r IF x > 1 THEN v = 1");
    assert_eq!(
        Runner::new(&strict).explain(&case).unwrap_err().code(),
        ErrorCode::MissingField
    );
    let nothing = Expectation::new()
        .with_output(ExpectedOutput::NothingSelected)
        .unwrap();
    let case = case.with_expect(nothing);
    assert_eq!(runner.run_case(&case).status, CaseStatus::Passed);
}

#[test]
fn test_runs_are_output_only_and_written_as_text() {
    let ruleset = rules(RULES);
    let run = Runner::new(&ruleset).run(&[]);
    assert_eq!(
        json(&run),
        r#"{"source":"ruleset","hit_policy":"priority","logic":"boolean","engine":{"name":"cleanrules","version":"<version>","limits":{"max_size":16777216,"max_nesting":1024,"max_depth":256,"max_path_segments":256,"max_table_size":2097152,"max_values":1000000}},"files":[],"limits":{"max_explained":5},"summary":{"total":0,"passed":0,"failed":0,"error":0,"partial":false},"results":[]}"#
    );
    let document = write_test_run_as(&run, DocumentFormat::Json).unwrap();
    assert!(document.starts_with(r#"{"version":1,"test_run":{"#));
    let error = read_document(&document, &Limits::default()).unwrap_err();
    assert_eq!(error.code(), ErrorCode::Unsupported);
    assert_eq!(
        write_test_run_as(&run, DocumentFormat::Text).unwrap(),
        run.to_string()
    );
    assert_eq!(
        write_test_run_as(&run, DocumentFormat::Csv)
            .unwrap_err()
            .code(),
        ErrorCode::Unsupported
    );
    #[cfg(feature = "yaml")]
    assert!(
        write_test_run_as(&run, DocumentFormat::Yaml)
            .unwrap()
            .contains("test_run:")
    );
}

// --- Properties --------------------------------------------------------------

/// A rule set in the rule syntax over `x` and `y`, under any policy.
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
    ];
    let condition = atom.prop_recursive(2, 6, 2, |inner| {
        prop_oneof![
            inner.clone().prop_map(|c| format!("NOT ({c})")),
            (inner.clone(), inner).prop_map(|(a, b)| format!("({a}) AND ({b})")),
        ]
    });
    let policy = prop::sample::select(vec![
        "first",
        "priority",
        "unique",
        "any",
        "rule_order",
        "collect",
        "collect\nAGGREGATE sum",
    ]);
    let rule = (condition, 0..3i64, 0..3i64, any::<bool>());
    (policy, any::<bool>(), prop::collection::vec(rule, 0..5)).prop_map(
        |(policy, strict, rules)| {
            let mut text = format!("HIT POLICY {policy}\n");
            if strict {
                text.push_str("STRICT\n");
            }
            for (i, (when, priority, out, id)) in rules.into_iter().enumerate() {
                let id = if id {
                    format!(" ID 'R-{i}'")
                } else {
                    String::new()
                };
                let _ = writeln!(
                    text,
                    "RULE r{i}{id} PRIORITY {priority}\nIF {when}\nTHEN v = {out}"
                );
            }
            text
        },
    )
}

fn input_value() -> impl Strategy<Value = Value> {
    let field = prop_oneof![
        Just(None),
        Just(Some(Value::Null(cleanrules::NullCause::Null))),
        (0..5i64).prop_map(|n| Some(Value::Int(n))),
        Just(Some(Value::String("a".into()))),
    ];
    (field.clone(), field).prop_map(|(x, y)| {
        let mut map = Map::new();
        if let Some(x) = x {
            map.insert("x", x);
        }
        if let Some(y) = y {
            map.insert("y", y);
        }
        Value::Map(map)
    })
}

/// The case that expects exactly what evaluating `input` gives.
fn faithful_case(ruleset: &RuleSet, id: String, input: &Value) -> Case {
    let case = Case::new(id, input.clone()).unwrap();
    let expect = match ruleset.evaluate(input) {
        Err(error) => Expectation::new().with_error(error.code()).unwrap(),
        Ok(decision) => {
            let selected: Vec<&str> = decision.selected().map(|r| r.rule.effective_id()).collect();
            let outputs: Vec<Map> = decision
                .outputs()
                .into_iter()
                .map(|o| o.into_iter().collect())
                .collect();
            let unknown: Vec<&str> = decision
                .results()
                .iter()
                .filter(|r| r.when.truth() == Some(cleanrules::logic::Truth::Unknown))
                .map(|r| r.rule.effective_id())
                .collect();
            let mut expect = Expectation::new()
                .with_selected(selected)
                .unwrap()
                .with_outputs(outputs)
                .unwrap()
                .with_unknown(unknown)
                .unwrap();
            if let Ok(output) = decision.output() {
                let output = match output {
                    Some(Value::Map(map)) => ExpectedOutput::Includes(map),
                    _ => ExpectedOutput::NothingSelected,
                };
                expect = expect.with_output(output).unwrap();
            }
            expect
        }
    };
    case.with_expect(expect)
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(200))]

    #[test]
    fn a_case_of_what_the_decision_gives_passes(
        text in ruleset_text(),
        inputs in prop::collection::vec(input_value(), 1..6),
    ) {
        let ruleset = parse(&text);
        let items: Vec<Case> = inputs
            .into_iter()
            .enumerate()
            .map(|(i, input)| faithful_case(&ruleset, format!("c{i}"), &input))
            .collect();
        let run = Runner::new(&ruleset).run(&[CaseFile::new(Cases::new(items).unwrap())]);
        prop_assert!(run.all_passed(), "{}\n{}", text, run);
    }

    #[test]
    fn an_interrupted_run_is_a_prefix_of_the_full_run(
        text in ruleset_text(),
        inputs in prop::collection::vec(input_value(), 0..6),
        bad in any::<bool>(),
        stop in 0..8usize,
        steps in 0..30usize,
        expect_nothing in any::<bool>(),
    ) {
        let ruleset = parse(&text);
        let items: Vec<Case> = inputs
            .into_iter()
            .enumerate()
            .map(|(i, input)| {
                let case = Case::new(format!("c{i}"), input).unwrap();
                if expect_nothing {
                    case
                } else {
                    case.with_expect(Expectation::new().with_selected(["r0"]).unwrap())
                }
            })
            .collect();
        let mut files = vec![CaseFile::new(Cases::new(items).unwrap()).with_path("a.json")];
        if bad {
            files.insert(0, file("bad.json", "{"));
        }
        let runner = Runner::new(&ruleset).with_limits(TestRunLimits::default().with_max_explained(2));
        let full = runner.run(&files);
        let full_results: Vec<String> =
            full.results.iter().map(|r| to_string(&write_case_result(r))).collect();
        prop_assert!(!full.summary.is_partial());
        let limited = [
            TestRunLimits::default().with_max_explained(2).with_max_results(stop),
            TestRunLimits::default().with_max_explained(2).with_max_steps(steps),
        ];
        let mut runs: Vec<TestRun> = limited
            .into_iter()
            .map(|limits| runner.clone().with_limits(limits).run(&files))
            .collect();
        let asked = Cell::new(0);
        runs.push(runner.run_until(&files, || {
            asked.set(asked.get() + 1);
            asked.get() > stop
        }));
        for run in &runs {
            let given: Vec<String> =
                run.results.iter().map(|r| to_string(&write_case_result(r))).collect();
            prop_assert_eq!(&given[..], &full_results[..given.len()]);
            let remaining = full.summary.total() - given.len();
            if let Some(interrupted) = run.summary.interrupted {
                prop_assert!(remaining > 0);
                prop_assert_eq!(interrupted.remaining, remaining);
            } else {
                prop_assert_eq!(remaining, 0);
            }
            prop_assert_eq!(run.summary.total(), given.len());
        }
    }
}

#[test]
fn an_empty_case_file_gives_no_results() {
    let ruleset = parse("RULE r IF x > 1 THEN v = 1");
    let empty = CaseFile::new(Cases::new([]).unwrap()).with_path("empty.json");
    assert_eq!(empty.path(), Some("empty.json"));
    assert!(empty.cases().is_ok());
    let run = Runner::new(&ruleset)
        .with_limits(TestRunLimits::default().with_max_results(0))
        .run(&[empty]);
    assert!(!run.summary.is_partial(), "nothing remained");
    assert!(file("x.json", "{").cases().is_err());
}

#[test]
fn the_text_names_a_table_its_aggregate_and_missing_outputs() {
    let table = DecisionTable::from_csv(
        "HIT POLICY,collect\nAGGREGATE,sum\nSTRICT\nyears,THEN discount\n> 5,1\n> 6,2\n",
        &Limits::default(),
    )
    .unwrap();
    let text = r#"{"version":1,"cases":{"items":[
      {"id":"sum","input":{"years":7},"expect":{"output":{"discount":2},"outputs":[{"discount":1}]}}
    ]}}"#;
    let run = Runner::for_table(&table).run(&[CaseFile::new(cases(text))]);
    insta::assert_snapshot!("a_table_run", run.to_string());
    let rules = parse("HIT POLICY rule_order\nRULE a IF x > 1 THEN v = 1");
    let text = r#"{"version":1,"cases":{"items":[{"id":"one","input":{"x":2},"expect":{"output":{"v":1}}}]}}"#;
    let run = Runner::new(&rules).run(&[CaseFile::new(cases(text))]);
    insta::assert_snapshot!("a_run_without_a_single_output", run.to_string());
}

#[test]
fn a_run_left_early_is_partial() {
    let ruleset = parse("RULE r IF x > 1 THEN v = 1");
    let files = three_files();
    let runner = Runner::new(&ruleset);
    let mut iter = runner.iter(&files);
    let taken: Vec<_> = iter.by_ref().take(2).collect();
    let run = iter.into_test_run(taken);
    assert_eq!(run.results.len(), 2);
    let interrupted = run.summary.interrupted.unwrap();
    assert_eq!(
        (interrupted.reason, interrupted.remaining),
        (Interruption::Cancelled, 3)
    );
    assert!(!run.all_passed(), "a run left early is not a pass");
}

#[test]
fn an_owned_cursor_drives_the_same_run() {
    fn send<T: Send + Clone + 'static>(_: &T) {}
    let ruleset = parse("RULE r IF x > 1 THEN v = 1");
    let files = three_files();
    let runner = Runner::new(&ruleset);
    let full = runner.run(&files);
    let mut state = TestRunState::new();
    send(&state);
    let mut results = Vec::new();
    while let Some(result) = runner.next_result(&files, &mut state, || false) {
        results.push(result);
    }
    assert!(state.is_finished());
    assert_eq!(state.remaining(&files), 0);
    let owned = runner.test_run(&files, state, results);
    assert_eq!(json(&owned), json(&full));
    // Stopped after one, then asked for the test run: partial.
    let mut state = TestRunState::new();
    let first = runner.next_result(&files, &mut state, || false).unwrap();
    assert_eq!(state.summary().total(), 1);
    assert_eq!(state.remaining(&files), 4);
    runner.stop(&files, &mut state);
    assert!(runner.next_result(&files, &mut state, || false).is_none());
    let run = runner.test_run(&files, state, vec![first]);
    assert_eq!(run.summary.interrupted.unwrap().remaining, 4);
    // A cancel callback that says so before the first result.
    let mut state = TestRunState::new();
    assert!(runner.next_result(&files, &mut state, || true).is_none());
    assert_eq!(
        state.summary().interrupted.unwrap().reason,
        Interruption::Cancelled
    );
}

#[test]
fn a_case_is_its_path_and_id_in_a_run() {
    let ruleset = parse("RULE r IF x > 1 THEN v = 1");
    let one = || r#"{"version":1,"cases":{"items":[{"id":"a","input":{"x":2}}]}}"#;
    let files = [
        file("a.json", one()),
        file("a.json", one()),
        CaseFile::new(cases(one())),
        CaseFile::new(cases(one())),
        file("b.json", one()),
    ];
    let run = Runner::new(&ruleset)
        .with_limits(TestRunLimits::default().with_max_results(1))
        .run(&files);
    assert_eq!(run.summary.interrupted.unwrap().remaining, 4);
    let run = Runner::new(&ruleset).run(&files);
    let statuses: Vec<(Option<&str>, Option<&str>, CaseStatus)> = run
        .results
        .iter()
        .map(|r| (r.path.as_deref(), r.id.as_deref(), r.status))
        .collect();
    assert_eq!(
        statuses,
        [
            (Some("a.json"), Some("a"), CaseStatus::Passed),
            (Some("a.json"), None, CaseStatus::Error),
            (None, Some("a"), CaseStatus::Passed),
            (None, None, CaseStatus::Error),
            (Some("b.json"), Some("a"), CaseStatus::Passed),
        ]
    );
    assert_eq!(
        run.results[1].error.as_ref().unwrap().code(),
        ErrorCode::FormatError
    );
    insta::assert_snapshot!("a_run_with_repeated_files", run.to_string());
    // Every file given is listed, with the digest of its bytes when it has
    // one.
    let listed = json(&run);
    assert!(
        listed.contains(r#""files":[{"path":"a.json","digest":"sha256:"#),
        "{listed}"
    );
    assert!(listed.contains(r#"},{},{},{"path":"b.json""#), "{listed}");
}

#[test]
fn case_files_are_built_and_read_every_way() {
    let ruleset = parse("RULE r IF x > 1 THEN v = 1");
    let text = r#"{"version":1,"cases":{"items":[{"id":"a b","input":{"x":2},"expect":{"selected":[]}}]}}"#;
    let file = CaseFile::read_as(
        None,
        text.as_bytes(),
        DocumentFormat::Json,
        &Limits::default(),
    );
    assert_eq!(file.path(), None);
    assert_eq!(
        file.digest(),
        Some(cleanrules::digest::Digest::of(text.as_bytes()))
    );
    let case = file.case("a b").unwrap();
    assert!(file.case("missing").is_none());
    let runner = Runner::new(&ruleset);
    let decision = runner.evaluate(case).unwrap();
    assert_eq!(decision.selected_positions(), [0]);
    let run = runner.run(&[file]);
    assert!(
        run.to_string().contains(r#"FAILED  "a b""#),
        "ids are quoted"
    );
    let unreadable = CaseFile::unreadable(
        Some("gone.json"),
        cleanrules::Error::new(ErrorCode::FormatError, "the file cannot be opened"),
    );
    assert!(unreadable.case("a").is_none());
    assert!(unreadable.digest().is_none());
    let run = runner.run(&[unreadable]);
    assert_eq!(run.summary.error, 1);
    assert_eq!(run.results[0].path.as_deref(), Some("gone.json"));
}

#[test]
fn a_bundle_runs_under_its_recorded_limits() {
    use cleanrules::bundle::EngineLimits;
    let ruleset = parse("RULE r IF x > 1 THEN v = 1");
    let small = r#"{"version":1,"cases":{"items":[{"id":"a","input":{"x":2}}]}}"#;
    let refs = [cleanrules::bundle::CaseFileRef::new("cases/a.json", small.as_bytes()).unwrap()];
    let limits = EngineLimits::default().with(cleanrules::bundle::EngineLimit::MaxSize, 20);
    let bundle = Bundle::new(
        ruleset,
        BundleId::new("acme/limits").unwrap(),
        Revision::new("1").unwrap(),
    )
    .with_cases(refs)
    .unwrap()
    .with_engine_limits(limits)
    .unwrap();
    let file = CaseFile::bundle_file(&bundle, "cases/a.json", small.as_bytes()).unwrap();
    assert!(CaseFile::bundle_file(&bundle, "cases/b.json", small.as_bytes()).is_none());
    let run = Runner::for_bundle(&bundle).run(&[file]);
    assert_eq!(
        run.results[0].error.as_ref().unwrap().code(),
        ErrorCode::LimitExceeded,
        "read under the bundle's max_size"
    );
    assert_eq!(run.engine_limits, limits);
    let written = json(&run);
    assert!(
        written.contains(r#""bundle":{"id":"acme/limits","revision":"1"}"#),
        "{written}"
    );
    assert!(written.contains(r#""max_size":20"#), "{written}");
    assert_eq!(
        run.engine(),
        ("cleanrules", cleanrules::bundle::ENGINE_VERSION)
    );
    let other = Runner::new(bundle.ruleset())
        .with_engine_limits(EngineLimits::default())
        .run(&[]);
    assert_eq!(other.engine_limits, EngineLimits::default());
}

/// A caller that loads a bundle's case files up to a bound
/// (`CaseFile::bundle_files_bounded`): the core checks digests and gives the
/// result of a file it was not given whole.
#[test]
fn a_bundles_case_files_are_loaded_up_to_a_bound() {
    use cleanrules::bundle::{CaseFileRef, EngineLimit, EngineLimits};
    use cleanrules::cases::CaseFileBytes;
    let good = r#"{"version":1,"cases":{"items":[{"id":"a","input":{"x":2},"expect":{"selected":["r"]}}]}}"#;
    let paths = [
        "cases/good.json",
        "cases/cut.yaml",
        "cases/big.json",
        "cases/known.json",
    ];
    let refs = paths.map(|path| CaseFileRef::new(path, good.as_bytes()).unwrap());
    // The bundle's limit is below the size of every file.
    let limits = EngineLimits::default().with(EngineLimit::MaxSize, 20);
    let bundle = Bundle::new(
        parse("RULE r IF x > 1 THEN v = 1"),
        BundleId::new("acme/bounds").unwrap(),
        Revision::new("1").unwrap(),
    )
    .with_cases(refs)
    .unwrap()
    .with_engine_limits(limits)
    .unwrap();
    let mut bounds = Vec::new();
    let files = CaseFile::bundle_files_bounded(&bundle, |r, bound| {
        bounds.push(bound);
        Ok::<_, ()>(match r.path() {
            "cases/good.json" => CaseFileBytes::Whole(good.as_bytes().to_vec()),
            "cases/cut.yaml" => CaseFileBytes::TooLarge { size: None },
            "cases/known.json" => CaseFileBytes::TooLarge {
                size: Some(1 << 30),
            },
            // More than the bound, though the caller loaded it.
            _ => CaseFileBytes::Whole(vec![b' '; bound + 1]),
        })
    })
    .unwrap();
    let defaults = Limits::default();
    // The larger of the bundle's limit and the default one, by syntax.
    assert_eq!(
        bounds,
        [
            defaults.max_size,
            defaults.max_yaml_size,
            defaults.max_size,
            defaults.max_size
        ]
    );
    // Within the bound: checked against its digest, then read under the
    // bundle's limit, which refuses it.
    assert!(files[0].digest().is_some());
    let errors: Vec<_> = files.iter().map(|file| file.cases().unwrap_err()).collect();
    assert!(errors.iter().all(|e| e.code() == ErrorCode::LimitExceeded));
    assert_eq!(
        errors[0].message(),
        "the document has 88 bytes, more than the limit of 20"
    );
    assert_eq!(
        errors[1].message(),
        "the YAML text has more bytes than the limit of 1048576"
    );
    assert_eq!(
        errors[2].message(),
        format!(
            "the document has {} bytes, more than the limit of 20",
            defaults.max_size + 1
        )
    );
    assert_eq!(
        errors[3].message(),
        "the document has 1073741824 bytes, more than the limit of 20"
    );
    assert!(files[1..].iter().all(|file| file.digest().is_none()));
    // A file not loaded whole is never checked against its digest.
    let changed = CaseFile::bundle_files_bounded(&bundle, |_, _| {
        Ok::<_, ()>(CaseFileBytes::TooLarge { size: None })
    })
    .unwrap();
    assert!(
        changed
            .iter()
            .all(|f| f.cases().unwrap_err().code() == ErrorCode::LimitExceeded)
    );
    assert_eq!(
        CaseFile::bundle_files_bounded(&bundle, |_, _| Err("no such file")).unwrap_err(),
        "no such file"
    );
    // The bundle's rules alone: the defaults, and no bundle.
    let run = Runner::for_bundle_rules(&bundle).run(&[file("cases/good.json", good)]);
    assert!(run.all_passed());
    assert!(run.bundle.is_none());
    assert_eq!(run.engine_limits, EngineLimits::default());
    let table = DecisionTable::from_csv("x,THEN v\n> 1,1\n", &Limits::default()).unwrap();
    let tabled = Bundle::new(
        table,
        BundleId::new("acme/t").unwrap(),
        Revision::new("1").unwrap(),
    );
    assert_eq!(
        Runner::for_bundle_rules(&tabled).run(&[]).source,
        RulesSource::DecisionTable
    );
}

/// A case file its caller did not load whole: `limit_exceeded`, no digest.
#[test]
fn a_case_file_too_large_to_load_is_its_error() {
    let limits = Limits::default();
    let file = CaseFile::too_large(
        Some("cases/big.yml"),
        DocumentFormat::Yaml,
        Some(5 << 20),
        &limits,
    );
    assert_eq!(file.path(), Some("cases/big.yml"));
    assert!(file.digest().is_none());
    let error = file.cases().unwrap_err();
    assert_eq!(error.code(), ErrorCode::LimitExceeded);
    assert_eq!(
        error.message(),
        "the YAML text has 5242880 bytes, more than the limit of 1048576"
    );
    assert_eq!(
        error.hint(),
        Some("raise the YAML size limit if the file is trusted")
    );
}
