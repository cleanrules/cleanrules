//! Comparing two versions of rules on the same cases
//! (`spec/comparison.md`): every kind of change, expected changes written as
//! the new version's expectations, cases on one side only, files that
//! cannot be read, partial sides, the relation of the two sides and of two
//! bundles, the text and JSON forms, and properties: a version compared with
//! itself has no change, swapping the sides swaps the items, a partial
//! comparison agrees with the full one, and cases that record what `b`
//! gives make every change expected.

// A test: a panic is the failure report.
#![allow(clippy::unwrap_used, clippy::panic)]

use std::cell::Cell;
use std::fmt::Write as _;

use cleanrules::bundle::{Bundle, BundleId, EngineLimit, EngineLimits, Relation, Revision};
use cleanrules::cases::{
    Case, CaseFile, CaseStatus, Cases, ChangeKind, ChangedPart, Comparator, ComparedItem,
    Expectation, ExpectedOutput, Interruption, Runner, SingleOutput, TestComparison, TestRunLimits,
};
use cleanrules::format::{
    Document, DocumentFormat, read_document, write_compared_item, write_test_comparison,
    write_test_comparison_as,
};
use cleanrules::json::{Limits, to_string};
use cleanrules::rules::RuleSet;
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

fn file(path: &str, text: &str) -> CaseFile {
    CaseFile::read(path, text.as_bytes(), &Limits::default())
}

fn compare(a: &RuleSet, b: &RuleSet, files: &[CaseFile]) -> TestComparison {
    Comparator::new(Runner::new(a), Runner::new(b))
        .unwrap()
        .with_max_explained(0)
        .compare(files, files)
}

/// The JSON of a comparison, with this engine's version as `<version>`.
fn json(comparison: &TestComparison) -> String {
    let version = format!(r#""version":"{}""#, cleanrules::bundle::ENGINE_VERSION);
    to_string(&write_test_comparison(comparison)).replace(&version, r#""version":"<version>""#)
}

fn kinds(comparison: &TestComparison) -> Vec<(Option<&str>, ChangeKind, Option<bool>)> {
    comparison
        .items
        .iter()
        .map(|item| (item.id.as_deref(), item.kind, item.expected))
        .collect()
}

/// The example rules with vip giving 20%.
fn vip_20() -> RuleSet {
    rules(&RULES.replace("\"0.15\"", "\"0.20\""))
}

#[test]
fn a_version_compared_with_itself_has_no_change() {
    let ruleset = rules(RULES);
    let files = [file("cases/discounts.json", CASES)];
    let comparison = compare(&ruleset, &ruleset, &files);
    assert_eq!(comparison.relation, Relation::SameRulesSameEngine);
    assert!(comparison.items.is_empty());
    assert_eq!(comparison.summary.unchanged, 4);
    assert_eq!(comparison.summary.total(), 4);
    assert!(!comparison.has_unexpected_changes());
    assert!(!comparison.is_partial());
    assert_eq!(comparison.a.ruleset_digest, ruleset.digest());
    assert_eq!(comparison.a.summary.passed, 4);
    // vip-gets-15 leaves blocked unknown; new-customer-gets-nothing selects
    // nothing.
    assert_eq!(
        (
            comparison.a.outcomes.nothing_selected,
            comparison.a.outcomes.with_unknown,
            comparison.a.outcomes.error
        ),
        (1, 1, 0)
    );
}

#[test]
fn a_changed_output_is_unexpected_unless_the_new_cases_state_it() {
    let (old, new) = (rules(RULES), vip_20());
    let files = [file("cases/discounts.json", CASES)];
    let comparator = Comparator::new(Runner::new(&old), Runner::new(&new)).unwrap();
    assert_eq!(comparator.max_explained(), 5);
    let comparison = comparator.compare(&files, &files);
    assert_eq!(comparison.relation, Relation::OtherRulesSameEngine);
    assert_eq!(
        kinds(&comparison),
        [(
            Some("vip-gets-15"),
            ChangeKind::SameSelectionOtherOutput,
            Some(false)
        )]
    );
    let item = &comparison.items[0];
    assert_eq!(item.differences, [ChangedPart::Output]);
    let (a, b) = (item.a.as_ref().unwrap(), item.b.as_ref().unwrap());
    assert_eq!(
        (a.status, b.status),
        (CaseStatus::Passed, CaseStatus::Failed)
    );
    assert!(a.explanation_text.is_some() && b.explanation_text.is_some());
    assert!(comparison.has_unexpected_changes());
    insta::assert_snapshot!("an_unexpected_change", comparison.to_string());
    insta::assert_snapshot!("an_unexpected_change_json", json(&comparison));

    // The new version's cases state the new discount: the change is expected,
    // and the old version fails them.
    let updated = CASES.replace(
        r#""output": { "discount": { "$decimal": "0.15" } }"#,
        r#""output": { "discount": { "$decimal": "0.20" } }"#,
    );
    let files_b = [file("cases/discounts.json", &updated)];
    let comparison = Comparator::new(Runner::new(&old), Runner::new(&new))
        .unwrap()
        .with_max_explained(0)
        .compare(&files, &files_b);
    assert_eq!(
        kinds(&comparison),
        [(
            Some("vip-gets-15"),
            ChangeKind::SameSelectionOtherOutput,
            Some(true)
        )]
    );
    assert_eq!(
        (comparison.summary.expected, comparison.summary.unexpected),
        (1, 0)
    );
    assert!(!comparison.has_unexpected_changes());
    assert!(
        comparison.items[0]
            .a
            .as_ref()
            .unwrap()
            .explanation_text
            .is_none()
    );
}

/// Cases on two inputs, with no expectations.
const XY: &str = r#"{"version":1,"cases":{"items":[
  {"id":"x1","input":{"x":1}},
  {"id":"x2","input":{"x":2}},
  {"id":"none","input":{}}
]}}"#;

#[test]
fn every_kind_of_change() {
    let files = [file("xy.json", XY)];
    let base = parse("HIT POLICY first\nRULE a IF x > 0 THEN v = 1\nRULE b IF x > 1 THEN v = 2");
    let cases = [
        // b selected for x2 instead of a, with the same output.
        (
            "HIT POLICY first\nRULE b IF x > 1 THEN v = 1\nRULE a IF x > 0 THEN v = 1",
            ChangeKind::OtherSelectionSameOutput,
            vec![ChangedPart::Selected],
        ),
        (
            "HIT POLICY first\nRULE a IF x > 0 THEN v = 3\nRULE b IF x > 1 THEN v = 2",
            ChangeKind::SameSelectionOtherOutput,
            vec![ChangedPart::Output],
        ),
        (
            "HIT POLICY first\nRULE b IF x > 1 THEN v = 2\nRULE a IF x > 0 THEN v = 1",
            ChangeKind::OtherSelectionOtherOutput,
            vec![ChangedPart::Selected, ChangedPart::Output],
        ),
        (
            "HIT POLICY unique\nRULE a IF x > 0 THEN v = 1\nRULE b IF x > 1 THEN v = 2",
            ChangeKind::Error,
            vec![ChangedPart::Error],
        ),
    ];
    for (text, change, differences) in cases {
        let other = parse(text);
        let comparison = compare(&base, &other, &files);
        let changed: Vec<&ComparedItem> = comparison
            .items
            .iter()
            .filter(|item| item.kind == change)
            .collect();
        assert!(!changed.is_empty(), "{text}: {comparison}");
        assert_eq!(changed[0].differences, differences, "{comparison}");
        assert_eq!(changed[0].expected, Some(false));
        let counted = comparison
            .summary
            .by_change
            .iter()
            .find(|(kind, _)| *kind == change)
            .unwrap()
            .1;
        assert_eq!(counted, changed.len());
    }
    // Only the unknown rules differ: c reads y, which no case has.
    let unknown = parse(
        "HIT POLICY first\nRULE a IF x > 0 THEN v = 1\nRULE b IF x > 1 THEN v = 2\nRULE c IF y > 0 THEN v = 9",
    );
    let comparison = compare(&base, &unknown, &files);
    assert_eq!(
        kinds(&comparison),
        [
            (Some("x1"), ChangeKind::OtherUnknown, Some(false)),
            (Some("x2"), ChangeKind::OtherUnknown, Some(false)),
            (Some("none"), ChangeKind::OtherUnknown, Some(false)),
        ]
    );
    assert_eq!(comparison.items[0].differences, [ChangedPart::Unknown]);
    insta::assert_snapshot!("other_unknown_rules", comparison.to_string());
}

#[test]
fn the_same_error_is_unchanged_and_another_is_a_change() {
    let strict = "HIT POLICY first\nSTRICT\nRULE a IF x > 0 THEN v = 1";
    let files = [file(
        "c.json",
        r#"{"version":1,"cases":{"items":[{"id":"missing","input":{}},{"id":"text","input":{"x":"a"}}]}}"#,
    )];
    let a = parse(strict);
    // The same errors under a description that changes the digest.
    let b = parse("HIT POLICY first\nSTRICT\nRULE a DESCRIPTION 'other words' IF x > 0 THEN v = 1");
    let comparison = compare(&a, &b, &files);
    assert_eq!(comparison.relation, Relation::OtherRulesSameEngine);
    assert_eq!(comparison.summary.unchanged, 2, "{comparison}");
    assert_eq!(comparison.a.outcomes.error, 2);
    // Another rule name: the errors' compared field `rule` differs.
    let c = parse("HIT POLICY first\nSTRICT\nRULE z IF x > 0 THEN v = 1");
    let comparison = compare(&a, &c, &files);
    assert_eq!(comparison.summary.unexpected, 2, "{comparison}");
    assert!(
        comparison
            .items
            .iter()
            .all(|item| item.kind == ChangeKind::Error)
    );
    insta::assert_snapshot!("other_errors", comparison.to_string());
    // An expected error that the new version raises: expected.
    let expecting = file(
        "c.json",
        r#"{"version":1,"cases":{"items":[{"id":"missing","input":{},"expect":{"error":{"code":"missing_field"}}}]}}"#,
    );
    let lenient = parse("HIT POLICY first\nRULE a IF x > 0 THEN v = 1");
    let comparison = Comparator::new(Runner::new(&lenient), Runner::new(&a))
        .unwrap()
        .with_max_explained(0)
        .compare(&[file("c.json", XY_MISSING)], &[expecting]);
    assert_eq!(
        kinds(&comparison),
        [(Some("missing"), ChangeKind::Error, Some(true))]
    );
    let item = &comparison.items[0];
    assert_eq!(item.b.as_ref().unwrap().status, CaseStatus::Passed);
    assert_eq!(
        item.b.as_ref().unwrap().error.as_ref().unwrap().code(),
        ErrorCode::MissingField
    );
}

const XY_MISSING: &str = r#"{"version":1,"cases":{"items":[{"id":"missing","input":{}}]}}"#;

#[test]
fn expectations_must_check_every_part_that_differs() {
    let a = parse("HIT POLICY first\nRULE a IF x > 0 THEN v = 1\nRULE b IF x > 1 THEN v = 2");
    let b = parse("HIT POLICY first\nRULE b IF x > 1 THEN v = 3\nRULE a IF x > 0 THEN v = 1");
    let with = |expect: &str| {
        file(
            "c.json",
            &format!(
                r#"{{"version":1,"cases":{{"items":[{{"id":"x2","input":{{"x":2}},"expect":{expect}}}]}}}}"#
            ),
        )
    };
    let files_a = [with("{}")];
    let expected = |expect: &str| {
        let comparison = Comparator::new(Runner::new(&a), Runner::new(&b))
            .unwrap()
            .compare(&files_a, &[with(expect)]);
        assert_eq!(comparison.items.len(), 1, "{comparison}");
        comparison.items[0].expected.unwrap()
    };
    // Selected and output differ.
    assert!(!expected("{}"));
    assert!(!expected(r#"{"selected":["b"]}"#));
    assert!(!expected(r#"{"output":{"v":3}}"#));
    assert!(expected(r#"{"selected":["b"],"output":{"v":3}}"#));
    assert!(expected(r#"{"selected":["b"],"outputs":[{"v":3}]}"#));
    // An output that names no key does not state the new value of `v`.
    assert!(!expected(r#"{"selected":["b"],"output":{}}"#));
    assert!(!expected(r#"{"selected":["b"],"outputs":[{}]}"#));
    // `b` must pass.
    assert!(!expected(r#"{"selected":["a"],"output":{"v":3}}"#));
    // A fixed error is expected by an expectation that states the new
    // decision (its selection or output), not by `unknown` alone nor by an
    // empty one.
    let unique = parse("HIT POLICY unique\nRULE a IF x > 0 THEN v = 1\nRULE b IF x > 1 THEN v = 2");
    for (expect, is_expected) in [
        (r#"{"unknown":[]}"#, false),
        (r#"{"selected":["b"]}"#, true),
        (r#"{"output":{"v":3}}"#, true),
    ] {
        let comparison = Comparator::new(Runner::new(&unique), Runner::new(&b))
            .unwrap()
            .compare(&files_a, &[with(expect)]);
        assert_eq!(
            kinds(&comparison),
            [(Some("x2"), ChangeKind::Error, Some(is_expected))],
            "{expect}"
        );
    }
    let comparison = Comparator::new(Runner::new(&unique), Runner::new(&b))
        .unwrap()
        .compare(&files_a, &files_a);
    assert_eq!(
        kinds(&comparison),
        [(Some("x2"), ChangeKind::Error, Some(false))]
    );
}

#[test]
fn an_output_change_is_expected_only_for_the_keys_named() {
    let a = parse(
        "HIT POLICY first\nRULE a IF x > 0 THEN discount = 1, tier = 'gold'\nRULE b IF x > 1 THEN discount = 2",
    );
    let changed_tier = parse(
        "HIT POLICY first\nRULE a IF x > 0 THEN discount = 1, tier = 'silver'\nRULE b IF x > 1 THEN discount = 2",
    );
    let changed_both = parse(
        "HIT POLICY first\nRULE a IF x > 0 THEN discount = 3, tier = 'silver'\nRULE b IF x > 1 THEN discount = 2",
    );
    let unique = parse(
        "HIT POLICY rule_order\nRULE a IF x > 0 THEN discount = 1, tier = 'gold'\nRULE b IF x > 1 THEN discount = 2",
    );
    let with = |expect: &str| {
        file(
            "c.json",
            &format!(
                r#"{{"version":1,"cases":{{"items":[{{"id":"x2","input":{{"x":2}},"expect":{expect}}}]}}}}"#
            ),
        )
    };
    let files_a = [with("{}")];
    let item = |b: &RuleSet, expect: &str| {
        let comparison = Comparator::new(Runner::new(&a), Runner::new(b))
            .unwrap()
            .compare(&files_a, &[with(expect)]);
        assert_eq!(comparison.items.len(), 1, "{comparison}");
        let item = &comparison.items[0];
        (item.expected.unwrap(), item.unstated.clone())
    };
    let unstated_output = vec![cleanrules::cases::Unstated::Part(ChangedPart::Output)];
    // Only tier changes: a case that names discount does not state it.
    assert_eq!(
        item(&changed_tier, r#"{"output":{"discount":1}}"#),
        (false, unstated_output.clone())
    );
    assert_eq!(
        item(&changed_tier, r#"{"output":{"tier":"silver"}}"#),
        (true, vec![])
    );
    assert_eq!(
        item(&changed_tier, r#"{"outputs":[{"tier":"silver"}]}"#),
        (true, vec![])
    );
    // Both change: both must be named.
    assert_eq!(
        item(&changed_both, r#"{"output":{"tier":"silver"}}"#),
        (false, unstated_output.clone())
    );
    assert_eq!(
        item(
            &changed_both,
            r#"{"output":{"tier":"silver","discount":3}}"#
        ),
        (true, vec![])
    );
    // From a single output to two outputs: the number is stated by
    // `outputs`, and the first position's changes need naming too.
    let to_multi = |expect: &str| {
        let comparison = Comparator::new(Runner::new(&a), Runner::new(&unique))
            .unwrap()
            .compare(&files_a, &[with(expect)]);
        comparison.items[0].expected.unwrap()
    };
    assert!(!to_multi(r#"{"selected":["a","b"]}"#));
    assert!(to_multi(
        r#"{"selected":["a","b"],"outputs":[{},{"discount":2}]}"#
    ));
    // A switch from some output to none is stated by `output: null`.
    let none = parse(
        "HIT POLICY first\nRULE a IF x > 5 THEN discount = 1, tier = 'gold'\nRULE b IF x > 5 THEN discount = 2",
    );
    assert_eq!(
        item(&none, r#"{"selected":[],"output":null}"#),
        (true, vec![])
    );
    let (expected, unstated) = item(&none, r#"{"selected":[]}"#);
    assert!(!expected);
    assert_eq!(unstated, unstated_output);
    // `b` did not pass: the status is why.
    assert_eq!(
        item(&changed_tier, r#"{"output":{"tier":"gold"}}"#),
        (false, vec![cleanrules::cases::Unstated::Status])
    );
}

#[test]
fn cases_on_one_side_and_files_that_cannot_be_read() {
    let ruleset = parse("HIT POLICY first\nRULE a IF x > 0 THEN v = 1");
    let files_a = [
        file(
            "one.json",
            r#"{"version":1,"cases":{"items":[{"id":"kept","input":{"x":1}},{"id":"gone","input":{"x":1}}]}}"#,
        ),
        file(
            "two.json",
            r#"{"version":1,"cases":{"items":[{"id":"t","input":{"x":1}}]}}"#,
        ),
        file("broken.json", "{"),
    ];
    let files_b = [
        file(
            "one.json",
            r#"{"version":1,"cases":{"items":[{"id":"new","input":{"x":0}},{"id":"kept","input":{"x":1}}]}}"#,
        ),
        file("two.json", "not json"),
        file("broken.json", "{"),
        file(
            "three.json",
            r#"{"version":1,"cases":{"items":[{"id":"t","input":{"x":1}}]}}"#,
        ),
    ];
    let comparison = Comparator::new(Runner::new(&ruleset), Runner::new(&ruleset))
        .unwrap()
        .compare(&files_a, &files_b);
    let listed: Vec<(Option<&str>, Option<&str>, ChangeKind)> = comparison
        .items
        .iter()
        .map(|item| (item.path.as_deref(), item.id.as_deref(), item.kind))
        .collect();
    assert_eq!(
        listed,
        [
            (Some("one.json"), Some("gone"), ChangeKind::Removed),
            (Some("one.json"), Some("new"), ChangeKind::Added),
            (Some("two.json"), None, ChangeKind::Error),
            (Some("three.json"), Some("t"), ChangeKind::Added),
        ]
    );
    // two.json's case is not compared: side b cannot read the file. The
    // file both sides cannot read, with the same error, is unchanged.
    let summary = &comparison.summary;
    assert_eq!(
        (
            summary.unchanged,
            summary.unexpected,
            summary.added,
            summary.removed,
            summary.not_compared
        ),
        (1, 1, 2, 1, 2)
    );
    assert_eq!(summary.total(), 7);
    // An added case carries the new side's result only, a removed case the
    // old side's.
    assert!(comparison.items[0].a.is_some() && comparison.items[0].b.is_none());
    assert!(comparison.items[1].a.is_none() && comparison.items[1].b.is_some());
    assert!(comparison.items[2].b.as_ref().unwrap().error.is_some());
    insta::assert_snapshot!("cases_on_one_side", comparison.to_string());
}

#[test]
fn files_that_repeat_a_path_are_joined_in_order() {
    let ruleset = parse("HIT POLICY first\nRULE a IF x > 0 THEN v = 1");
    let cases = r#"{"version":1,"cases":{"items":[{"id":"c","input":{"x":1}}]}}"#;
    let files = [
        file("broken.json", "{"),
        file("broken.json", cases),
        CaseFile::new(read_cases(cases)),
        CaseFile::new(read_cases(cases)),
    ];
    let comparison = compare(&ruleset, &ruleset, &files);
    assert!(comparison.items.is_empty(), "{comparison}");
    // The files neither side can read, or that repeat a path, are not
    // compared.
    assert_eq!(
        (
            comparison.summary.unchanged,
            comparison.summary.not_compared
        ),
        (1, 3)
    );
    assert!(!comparison.is_complete());
}

fn read_cases(text: &str) -> Cases {
    match read_document(text, &Limits::default()).unwrap() {
        Document::Cases(cases) => cases,
        other => panic!("not a case file: {other:?}"),
    }
}

#[test]
fn a_partial_side_leaves_items_not_compared() {
    let (old, new) = (rules(RULES), vip_20());
    let files = [file("cases/discounts.json", CASES)];
    // Side b stops after one result: vip-gets-15 is compared, the rest not.
    let comparison = Comparator::new(
        Runner::new(&old),
        Runner::new(&new).with_limits(TestRunLimits::default().with_max_results(1)),
    )
    .unwrap()
    .with_max_explained(0)
    .compare(&files, &files);
    assert!(comparison.is_partial());
    assert!(!comparison.a.is_partial());
    let interrupted = comparison.b.summary.interrupted.unwrap();
    assert_eq!(
        (interrupted.reason, interrupted.remaining),
        (Interruption::MaxResults, 3)
    );
    assert_eq!(
        (
            comparison.summary.unexpected,
            comparison.summary.not_compared
        ),
        (1, 3)
    );
    assert_eq!(comparison.b.limits.max_results, Some(1));
    insta::assert_snapshot!("a_partial_comparison", comparison.to_string());

    // The caller cancels after two items: side a stops, and side b never
    // starts.
    let asked = Cell::new(0);
    let comparison = Comparator::new(Runner::new(&old), Runner::new(&new))
        .unwrap()
        .compare_until(&files, &files, || {
            asked.set(asked.get() + 1);
            asked.get() > 2
        });
    assert_eq!(comparison.a.summary.total(), 2);
    assert_eq!(comparison.b.summary.total(), 0);
    let reasons = (
        comparison.a.summary.interrupted.unwrap().reason,
        comparison.b.summary.interrupted.unwrap().reason,
    );
    assert_eq!(reasons, (Interruption::Cancelled, Interruption::Cancelled));
    assert_eq!(comparison.summary.not_compared, 4);
    assert_eq!(asked.get(), 3);
}

#[test]
fn the_relation_of_the_two_sides() {
    let ruleset = rules(RULES);
    let files = [file("cases/discounts.json", CASES)];
    let other_limits = EngineLimits::default().with(EngineLimit::MaxDepth, 100);
    let comparison = Comparator::new(
        Runner::new(&ruleset),
        Runner::new(&ruleset).with_engine_limits(other_limits),
    )
    .unwrap()
    .compare(&files, &files);
    assert_eq!(comparison.relation, Relation::SameRulesOtherEngine);
    assert!(
        comparison
            .to_string()
            .starts_with("Comparison of two test runs: same rules, other engine")
    );
    assert!(
        comparison
            .to_string()
            .contains("\nengine limits differ: max_depth a 256, b 100\n")
    );
    let other = vip_20();
    let comparison = Comparator::new(
        Runner::new(&ruleset),
        Runner::new(&other).with_engine_limits(other_limits),
    )
    .unwrap()
    .compare(&files, &files);
    assert_eq!(comparison.relation, Relation::OtherRulesOtherEngine);
    assert!(
        comparison
            .to_string()
            .contains("note: both the rules and the engine differ")
    );
    // A decision table and the rule set it compiles to are the same rules.
    let table = r#"{"version":1,"decision_table":{"hit_policy":"first","inputs":["x"],"outputs":[{"name":"v"}],"rows":[{"name":"r","inputs":["> 0"],"outputs":["1"]}]}}"#;
    let Document::DecisionTable(table) = read_document(table, &Limits::default()).unwrap() else {
        panic!("not a table")
    };
    let comparison = Comparator::new(Runner::for_table(&table), Runner::new(table.ruleset()))
        .unwrap()
        .compare(&[], &[]);
    assert_eq!(comparison.relation, Relation::SameRulesSameEngine);
    assert_eq!(comparison.summary.total(), 0);
    assert!(
        comparison
            .to_string()
            .contains("\na: a decision table, 1 rule, hit policy first")
    );
}

fn bundle(ruleset: &RuleSet, revision: &str, cases: &[(&str, &str)]) -> Bundle {
    Bundle::new(
        ruleset.clone(),
        BundleId::new("acme/discounts").unwrap(),
        Revision::new(revision).unwrap(),
    )
    .with_cases(
        cases.iter().map(|(path, text)| {
            cleanrules::bundle::CaseFileRef::new(path, text.as_bytes()).unwrap()
        }),
    )
    .unwrap()
}

#[test]
fn two_bundles_are_compared_on_their_own_cases() {
    let (old, new) = (rules(RULES), vip_20());
    let updated = CASES.replace(
        r#""output": { "discount": { "$decimal": "0.15" } }"#,
        r#""output": { "discount": { "$decimal": "0.20" } }"#,
    );
    let a = bundle(&old, "1", &[("cases/discounts.json", CASES)]);
    let b = bundle(&new, "2", &[("cases/discounts.json", &updated)]);
    let files_a = CaseFile::bundle_files(&a, |_| Ok::<_, ()>(CASES.as_bytes().to_vec())).unwrap();
    let files_b = CaseFile::bundle_files(&b, |_| Ok::<_, ()>(updated.as_bytes().to_vec())).unwrap();
    let comparison = Comparator::for_bundles(&a, &b)
        .unwrap()
        .with_max_explained(0)
        .compare(&files_a, &files_b);
    assert_eq!(comparison.relation, Relation::OtherRulesSameEngine);
    let bundles = comparison.bundles.as_ref().unwrap();
    assert_eq!(bundles.relation(), Relation::OtherRulesSameEngine);
    assert_eq!(
        kinds(&comparison),
        [(
            Some("vip-gets-15"),
            ChangeKind::SameSelectionOtherOutput,
            Some(true)
        )]
    );
    assert!(comparison.a.bundle.is_some());
    insta::assert_snapshot!("two_bundles", comparison.to_string());

    // One revision with other rules is a conflict, refused.
    let conflicting = bundle(&new, "1", &[]);
    let error = Comparator::for_bundles(&a, &conflicting).unwrap_err();
    assert_eq!(error.code(), ErrorCode::DecisionMismatch);
    let error =
        Comparator::new(Runner::for_bundle(&a), Runner::for_bundle(&conflicting)).unwrap_err();
    assert_eq!(error.code(), ErrorCode::DecisionMismatch);
    assert!(error.message().contains("acme/discounts revision 1"));
}

#[test]
fn a_decision_without_a_single_output_and_its_forms() {
    let a = parse("HIT POLICY rule_order\nRULE a IF x > 0 THEN v = 1\nRULE b IF x > 1 THEN v = 2");
    let b = parse("HIT POLICY rule_order\nRULE a IF x > 0 THEN v = 1\nRULE b IF x > 1 THEN v = 3");
    let comparison = compare(&a, &b, &[file("xy.json", XY)]);
    let item = &comparison.items[0];
    let decision = item.b.as_ref().unwrap().decision.as_ref().unwrap();
    assert!(matches!(decision.output, SingleOutput::NoSingleOutput));
    assert_eq!(decision.outputs.len(), 2);
    let written = to_string(&write_compared_item(item));
    assert!(!written.contains(r#""output":"#), "{written}");
    assert!(
        written.contains(r#""outputs":[{"v":1},{"v":3}]"#),
        "{written}"
    );
    insta::assert_snapshot!("no_single_output", comparison.to_string());

    // The document: JSON and text; readers refuse it.
    let text = write_test_comparison_as(&comparison, DocumentFormat::Json).unwrap();
    assert!(
        text.starts_with(r#"{"version":1,"test_comparison":{"relation":"other_rules_same_engine""#)
    );
    let error = read_document(&text, &Limits::default()).unwrap_err();
    assert_eq!(error.code(), ErrorCode::Unsupported);
    assert_eq!(
        write_test_comparison_as(&comparison, DocumentFormat::Text).unwrap(),
        comparison.to_string()
    );
    assert_eq!(
        write_test_comparison_as(&comparison, DocumentFormat::Csv)
            .unwrap_err()
            .code(),
        ErrorCode::Unsupported
    );
}

#[test]
fn names_round_trip() {
    for kind in ChangeKind::ALL {
        assert_eq!(ChangeKind::from_name(kind.as_str()), Some(*kind));
        assert_eq!(kind.to_string(), kind.as_str());
        assert_eq!(
            kind.is_change(),
            !matches!(kind, ChangeKind::Added | ChangeKind::Removed)
        );
    }
    assert_eq!(cleanrules::cases::Unstated::Status.to_string(), "status");
    for part in ChangedPart::ALL {
        assert_eq!(ChangedPart::from_name(part.as_str()), Some(*part));
        assert_eq!(part.to_string(), part.as_str());
    }
    assert_eq!(ChangeKind::from_name("unchanged"), None);
    assert_eq!(ChangedPart::from_name("outputs"), None);
}

// --- Properties ---------------------------------------------------------------

fn ruleset_text() -> impl Strategy<Value = String> {
    let atom = prop_oneof![
        (0..3i64).prop_map(|n| format!("x > {n}")),
        (0..3i64).prop_map(|n| format!("y = {n}")),
        Just("x IS NULL".to_owned()),
    ];
    let condition = atom.prop_recursive(2, 6, 2, |inner| {
        prop_oneof![
            inner.clone().prop_map(|c| format!("NOT ({c})")),
            (inner.clone(), inner).prop_map(|(a, b)| format!("({a}) AND ({b})")),
        ]
    });
    let policy = prop::sample::select(vec!["first", "priority", "unique", "rule_order"]);
    let rule = (condition, 0..3i64, 0..3i64);
    (policy, any::<bool>(), prop::collection::vec(rule, 0..4)).prop_map(
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
    let field = prop_oneof![
        Just(None),
        Just(Some(Value::Null(cleanrules::NullCause::Null))),
        (0..4i64).prop_map(|n| Some(Value::Int(n))),
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

fn plain_cases(inputs: &[Value]) -> CaseFile {
    let items = inputs
        .iter()
        .enumerate()
        .map(|(i, input)| Case::new(format!("c{i}"), input.clone()).unwrap());
    CaseFile::new(Cases::new(items).unwrap()).with_path("c.json")
}

/// The cases that expect exactly what `ruleset` gives on `inputs`.
fn recorded_cases(ruleset: &RuleSet, inputs: &[Value]) -> CaseFile {
    let items = inputs.iter().enumerate().map(|(i, input)| {
        let case = Case::new(format!("c{i}"), input.clone()).unwrap();
        let expect = match ruleset.evaluate(input) {
            Err(error) => Expectation::new().with_error(error.code()).unwrap(),
            Ok(decision) => {
                let selected: Vec<&str> =
                    decision.selected().map(|r| r.rule.effective_id()).collect();
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
    });
    CaseFile::new(Cases::new(items).unwrap()).with_path("c.json")
}

/// An item as comparable text: its JSON without explanations.
fn item_json(item: &ComparedItem) -> String {
    to_string(&write_compared_item(item))
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(200))]

    #[test]
    fn a_version_compared_with_itself_has_no_change_property(
        text in ruleset_text(),
        inputs in prop::collection::vec(input_value(), 0..6),
    ) {
        let ruleset = parse(&text);
        let files = [plain_cases(&inputs)];
        let comparison = compare(&ruleset, &ruleset, &files);
        prop_assert!(comparison.items.is_empty(), "{}", comparison);
        prop_assert_eq!(comparison.summary.unchanged, inputs.len());
    }

    #[test]
    fn swapping_the_sides_swaps_the_items(
        a in ruleset_text(),
        b in ruleset_text(),
        inputs in prop::collection::vec(input_value(), 0..6),
    ) {
        let (a, b) = (parse(&a), parse(&b));
        let files = [plain_cases(&inputs)];
        let forward = compare(&a, &b, &files);
        let backward = compare(&b, &a, &files);
        prop_assert_eq!(forward.relation, backward.relation);
        prop_assert_eq!(forward.items.len(), backward.items.len());
        for (f, r) in forward.items.iter().zip(&backward.items) {
            prop_assert_eq!(&f.id, &r.id);
            prop_assert_eq!(f.kind, r.kind);
            prop_assert_eq!(&f.differences, &r.differences);
            // No case has an expectation: no change is expected.
            prop_assert_eq!(f.expected, Some(false));
        }
    }

    #[test]
    fn cases_recorded_from_b_make_every_change_expected(
        a in ruleset_text(),
        b in ruleset_text(),
        inputs in prop::collection::vec(input_value(), 0..6),
    ) {
        let (a, b) = (parse(&a), parse(&b));
        let files_a = [plain_cases(&inputs)];
        let files_b = [recorded_cases(&b, &inputs)];
        let comparison = Comparator::new(Runner::new(&a), Runner::new(&b))
            .unwrap()
            .with_max_explained(0)
            .compare(&files_a, &files_b);
        prop_assert_eq!(comparison.summary.unexpected, 0, "{}", comparison);
        prop_assert_eq!(comparison.b.summary.passed, inputs.len());
    }

    #[test]
    fn a_partial_comparison_agrees_with_the_full_one(
        a in ruleset_text(),
        b in ruleset_text(),
        inputs in prop::collection::vec(input_value(), 0..6),
        max_a in prop::option::of(0..7usize),
        max_b in prop::option::of(0..7usize),
    ) {
        let (a, b) = (parse(&a), parse(&b));
        let files = [plain_cases(&inputs)];
        let full = compare(&a, &b, &files);
        let limited = |max: Option<usize>| match max {
            Some(max) => TestRunLimits::default().with_max_results(max),
            None => TestRunLimits::default(),
        };
        let partial = Comparator::new(
            Runner::new(&a).with_limits(limited(max_a)),
            Runner::new(&b).with_limits(limited(max_b)),
        )
        .unwrap()
        .with_max_explained(0)
        .compare(&files, &files);
        let compared = max_a.unwrap_or(usize::MAX).min(max_b.unwrap_or(usize::MAX)).min(inputs.len());
        prop_assert_eq!(
            partial.summary.unchanged + partial.summary.changed(),
            compared
        );
        prop_assert_eq!(partial.summary.not_compared, inputs.len() - compared);
        prop_assert_eq!(partial.is_partial(), compared < inputs.len());
        // Every change listed is listed alike in the full comparison.
        let full_items: Vec<String> = full.items.iter().map(item_json).collect();
        let partial_items: Vec<String> = partial.items.iter().map(item_json).collect();
        prop_assert_eq!(&partial_items[..], &full_items[..partial_items.len()]);
    }
}

#[test]
fn the_text_of_every_kind_of_line() {
    // Aggregates, strict mode, errors naming rules, files without a path or
    // read at the root, and a partial side a.
    let a = parse("HIT POLICY collect\nAGGREGATE sum\nSTRICT\nRULE a IF x > 0 THEN v = 1");
    let b = parse("HIT POLICY unique\nRULE a IF x > 0 THEN v = 1\nRULE b IF x > 1 THEN v = 2");
    let cases = r#"{"version":1,"cases":{"items":[{"id":"two","input":{"x":2}},{"id":"three","input":{"x":3}}]}}"#;
    let files_a = [file("root.json", "[]"), CaseFile::new(read_cases(cases))];
    let files_b = [file("root.json", "{}"), CaseFile::new(read_cases(cases))];
    let comparison = Comparator::new(
        Runner::new(&a).with_limits(TestRunLimits::default().with_max_results(2)),
        Runner::new(&b),
    )
    .unwrap()
    .compare(&files_a, &files_b);
    insta::assert_snapshot!("every_kind_of_line", comparison.to_string());

    // Two identical bundles, one of whose case files is not the one it names.
    let ruleset = parse("HIT POLICY first\nRULE a IF x > 0 THEN v = 1");
    let same = bundle(&ruleset, "1", &[("cases/c.json", XY)]);
    let files = CaseFile::bundle_files(&same, |_| Ok::<_, ()>(b"{}".to_vec())).unwrap();
    let comparison = Comparator::for_bundles(&same, &same)
        .unwrap()
        .compare(&files, &[]);
    insta::assert_snapshot!("identical_bundles", comparison.to_string());

    // A file without a path that neither side can read, with other errors.
    let unreadable = |message: &str| {
        CaseFile::unreadable(
            None,
            cleanrules::Error::new(ErrorCode::FormatError, message),
        )
    };
    let comparison = compare(&ruleset, &ruleset, &[unreadable("one")]);
    assert!(comparison.items.is_empty());
    let comparison = Comparator::new(Runner::new(&ruleset), Runner::new(&ruleset))
        .unwrap()
        .compare(&[unreadable("one")], &[unreadable("other")]);
    assert!(comparison.items.is_empty(), "the message is not compared");
    let comparison = Comparator::new(Runner::new(&ruleset), Runner::new(&ruleset))
        .unwrap()
        .compare(
            &[unreadable("one")],
            &[CaseFile::unreadable(
                None,
                cleanrules::Error::new(ErrorCode::FormatError, "one").at_pointer("/cases"),
            )],
        );
    let text = comparison.to_string();
    assert!(
        text.contains("UNEXPECTED  (a case file without a path)\n"),
        "{text}"
    );
    assert!(
        text.contains("b: error, format_error at /cases: one"),
        "{text}"
    );
}

#[test]
fn unexpected_changes_are_explained_first() {
    let a = parse("HIT POLICY first\nRULE a IF x > 0 THEN v = 1");
    let b = parse("HIT POLICY first\nRULE a IF x > 0 THEN v = 2");
    let files_a = [file(
        "c.json",
        r#"{"version":1,"cases":{"items":[{"id":"stated","input":{"x":1}},{"id":"plain","input":{"x":1}}]}}"#,
    )];
    let files_b = [file(
        "c.json",
        r#"{"version":1,"cases":{"items":[{"id":"stated","input":{"x":1},"expect":{"output":{"v":2}}},{"id":"plain","input":{"x":1}}]}}"#,
    )];
    let comparison = Comparator::new(Runner::new(&a), Runner::new(&b))
        .unwrap()
        .with_max_explained(1)
        .compare(&files_a, &files_b);
    assert_eq!(
        kinds(&comparison),
        [
            (
                Some("stated"),
                ChangeKind::SameSelectionOtherOutput,
                Some(true)
            ),
            (
                Some("plain"),
                ChangeKind::SameSelectionOtherOutput,
                Some(false)
            ),
        ]
    );
    let explained: Vec<bool> = comparison
        .items
        .iter()
        .map(|item| item.b.as_ref().unwrap().explanation_text.is_some())
        .collect();
    assert_eq!(explained, [false, true]);
    assert!(comparison.is_complete());
}

#[test]
fn bundles_must_be_those_of_the_runners() {
    let ruleset = parse("HIT POLICY first\nRULE a IF x > 0 THEN v = 1");
    let one = bundle(&ruleset, "1", &[]);
    let two = bundle(&ruleset, "2", &[]);
    let error = Comparator::new(Runner::new(&ruleset), Runner::for_bundle(&two))
        .unwrap()
        .with_bundles(&one, &two)
        .unwrap_err();
    assert_eq!(error.code(), ErrorCode::DecisionMismatch);
    assert!(error.message().contains("side a"), "{error}");
    let comparison = Comparator::new(
        Runner::for_bundle(&one).with_limits(TestRunLimits::default().with_max_results(0)),
        Runner::for_bundle(&two),
    )
    .unwrap()
    .with_bundles(&one, &two)
    .unwrap()
    .compare(&[], &[]);
    assert_eq!(comparison.a.limits.max_results, Some(0));
    assert!(comparison.bundles.is_some());
}

#[test]
fn a_side_carries_the_statistics_its_runner_gathers() {
    let (old, new) = (rules(RULES), vip_20());
    let files = [file("cases/discounts.json", CASES)];
    let comparison = Comparator::new(Runner::new(&old).with_statistics(true), Runner::new(&new))
        .unwrap()
        .with_max_explained(0)
        .compare(&files, &files);
    let statistics = comparison.a.statistics.as_ref().unwrap();
    let alone = Runner::new(&old).with_statistics(true).run(&files);
    assert_eq!(
        to_string(&cleanrules::format::write_statistics(statistics)),
        to_string(&cleanrules::format::write_statistics(
            alone.statistics.as_ref().unwrap()
        ))
    );
    assert!(comparison.b.statistics.is_none());
    let written = json(&comparison);
    assert_eq!(written.matches(r#""statistics":"#).count(), 1);
}
