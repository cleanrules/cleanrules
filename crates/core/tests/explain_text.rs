//! Snapshots of the text of explanations and summaries (`spec/explain.md`,
//! *Text*): the snapshots, not the spec, fix its exact form.

#![allow(clippy::unwrap_used, clippy::panic)]

use cleanrules::Value;
use cleanrules::explain::{explain, summary};
use cleanrules::format::{Document, ValueContext, read_value};
use cleanrules::json::{self, Limits};
use cleanrules::rules::RuleSet;
use cleanrules::text::parse;

fn rules(text: &str) -> RuleSet {
    match parse(text, &Limits::default()).unwrap() {
        Document::RuleSet(ruleset) => ruleset,
        _ => panic!("not a rule set"),
    }
}

fn data(text: &str) -> Value {
    let json = json::parse(text, &Limits::default()).unwrap();
    read_value(&json, "", ValueContext::Data).unwrap()
}

fn explained(rules_text: &str, input: &str) -> String {
    let ruleset = rules(rules_text);
    let decision = ruleset.evaluate(&data(input)).unwrap();
    explain(&decision).to_string()
}

#[test]
fn a_priority_decision() {
    // The example of `spec/explain.md`, *Text*.
    let text = explained(
        "HIT POLICY priority
         RULE a IF x > 10 AND NOT y <= 3
         RULE b IF x BETWEEN 0 AND 5
         RULE c IF x > 1 AND NOT x > 1
         RULE d PRIORITY 1 IF x > 1
         RULE e PRIORITY 5 IF x > 0",
        r#"{"x": 8, "y": 2}"#,
    );
    insta::assert_snapshot!(text);
}

#[test]
fn unknown_conditions_give_their_reason() {
    let text = explained(
        r#"HIT POLICY first
         RULE missing DESCRIPTION "Spanish VIPs.\n\nNeeds both fields." IF country = "ES" AND vip
         RULE mismatch IF age > 18
         RULE nulls IF score >= 0.5e0
         RULE division IF total / count > 10
         RULE overflow IF big * 2 > 0
         RULE input IF [0] = 1"#,
        r#"{"age": "old", "score": null, "total": 250.5, "count": 0, "big": 9223372036854775807}"#,
    );
    insta::assert_snapshot!(text);
}

#[test]
fn values_are_written_as_constants_and_bounded() {
    let long = "a".repeat(80);
    let text = explained(
        r#"HIT POLICY first
         RULE named IF name STARTS WITH "Z"
         RULE listed IF labels IS NULL
         RULE mapped IF profile IS NULL
         RULE typed IF day = DATE "2026-01-01" OR rate = 0.5e0 OR price = 1.50 OR ok = FALSE
         RULE "odd name" IF `first name` = 'Ana'"#,
        &format!(
            r#"{{"name": "{long}", "labels": [0, 1, 2, 3, 4, 5, 6, 7], "profile": {{"a": [1, [2, [3]]], "b": "x\ny"}},
                "day": {{"$date": "2026-02-03"}}, "rate": 0.25, "price": {{"$decimal": "1.25"}}, "ok": true,
                "first name": "Eva"}}"#
        ),
    );
    insta::assert_snapshot!(text);
}

#[test]
fn counterfactuals_say_how_far_each_value_is() {
    let text = explained(
        r#"HIT POLICY first
         RULE limit IF x > 5
         RULE not_equal IF NOT code = "A"
         RULE equal IF code = "B"
         RULE price IF price <= 9.90
         RULE ratio IF ratio > 0.7e0
         RULE day IF day >= DATE "2026-11-01"
         RULE moment IF at < DATETIME "2026-11-02T09:00:00"
         RULE flipped IF 10 < x
         RULE negated IF NOT y < 3
         RULE nothing IF x > 100"#,
        r#"{"x": 5, "y": 1, "code": "A", "price": {"$decimal": "10.05"}, "ratio": 0.65,
            "day": {"$date": "2026-10-29"}, "at": {"$datetime": "2026-11-02T10:30:00.500000"}}"#,
    );
    insta::assert_snapshot!(text);
}

#[test]
fn groups_nest_and_conflicts_are_reported() {
    let text = explained(
        "HIT POLICY rule_order
         RULE both IF (a > 1 OR b > 1) AND (c > 1 OR d > 1)
         RULE fired IF (a < 1 OR b > 1) AND c < 1
         RULE either IF a < 1 OR b < 1
         RULE conflict IF a > 1 AND NOT a > 1",
        r#"{"a": 0, "b": 0, "c": 0, "d": 0}"#,
    );
    insta::assert_snapshot!(text);
}

#[test]
fn notes_say_why_a_rule_was_not_selected() {
    let cases = [
        "HIT POLICY priority
         RULE low PRIORITY 2 IF x > 1
         RULE high PRIORITY 7 IF x > 0",
        "HIT POLICY priority
         RULE first PRIORITY 2 IF x > 1
         RULE second PRIORITY 2 IF x > 0",
        "HIT POLICY any
         RULE a IF x > 1 THEN r = 1
         RULE b IF x > 0 THEN r = 1",
        "HIT POLICY first
         RULE a IF x > 1
         RULE b IF x > 0",
        "HIT POLICY first
         RULE a IF x > 100",
    ];
    let text: Vec<String> = cases
        .iter()
        .map(|rules| explained(rules, r#"{"x": 5}"#))
        .collect();
    insta::assert_snapshot!(text.join("\n\n"));
}

#[test]
fn a_summary() {
    let ruleset = rules(
        r#"HIT POLICY collect
         AGGREGATE sum
         STRICT
         RULE vip PRIORITY 10 TAGS gold, loyalty DESCRIPTION "Long-standing customers"
         IF customer.years > 5 AND customer.`first name` STARTS WITH "A"
         THEN discount = order.total * 0.15
         RULE "big order" PRIORITY -1 TAGS loyalty, "two words"
         IF order.total > 1000 AND order.lines[0].sku IN ("a", "b", "c") AND order.country != "ES"
         THEN discount = 0.05
         RULE always IF TRUE
         RULE acting IF FALSE"#,
    );
    let overview = summary(&ruleset).with_actions([false, true, false, true]);
    insta::assert_snapshot!(overview.to_string());
}

#[test]
fn a_summary_of_no_rules() {
    insta::assert_snapshot!(summary(&rules("HIT POLICY first")).to_string());
}

#[test]
fn the_summary_of_the_spec() {
    // The example of `spec/explain.md`, *Summaries*.
    let ruleset = rules(
        r#"HIT POLICY priority
         RULE vip PRIORITY 10 TAGS gold IF customer.years > 5 AND since IS NULL
         THEN discount = 0.15, label = "VIP"
         RULE loyal PRIORITY 5 IF customer.years > 2 THEN discount = 0.05 DO notify"#,
    );
    let overview = summary(&ruleset);
    insta::assert_snapshot!(overview.to_string());
}

#[test]
fn actions_are_one_per_rule() {
    let ruleset = rules("HIT POLICY first\nRULE a IF x > 1\nRULE b IF x > 2");
    assert_eq!(summary(&ruleset).actions, [false, false]);
    assert_eq!(
        summary(&ruleset).with_actions([true]).actions,
        [true, false]
    );
    assert_eq!(
        summary(&ruleset).with_actions([true, true, true]).actions,
        [true, true]
    );
    // A named action is the core's to report, whatever a binding says.
    let ruleset = rules("HIT POLICY first\nRULE a IF x > 1 DO notify\nRULE b IF x > 2");
    assert_eq!(summary(&ruleset).actions, [true, false]);
    assert_eq!(
        summary(&ruleset).with_actions([false, true]).actions,
        [true, true]
    );
}

#[test]
fn displayed_text_stays_on_one_line_per_cause() {
    // A key with a control character has no canonical text; displayed, it is
    // escaped. Long map keys are cut like strings.
    let ruleset = rules("HIT POLICY collect AGGREGATE sum\nRULE a IF `k` IS NULL THEN n = 1");
    let decision = ruleset
        .evaluate(&data(&format!(r#"{{"k": {{"{}": 1}}}}"#, "é".repeat(100))))
        .unwrap();
    insta::assert_snapshot!(explain(&decision).to_string());
    let node = cleanrules::ast::Node::compare(
        cleanrules::ast::CompareOp::Gt,
        cleanrules::ast::Node::field(vec![cleanrules::Segment::Key("k\ny".into())]).unwrap(),
        cleanrules::ast::Node::constant(Value::Int(1)).unwrap(),
    )
    .unwrap();
    assert_eq!(node.to_string(), "`k\\u000ay` > 1");
}
