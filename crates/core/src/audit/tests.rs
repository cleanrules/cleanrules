#![allow(clippy::unwrap_used, clippy::panic, clippy::print_stdout)]

use super::{
    Audit, AuditCheck, AuditLimit, AuditLimits, CheckStatus, DiagnosticCode, Severity,
    UndecidedReason, audit, audit_table,
};
use crate::eval::{Options, evaluate_tree};
use crate::format::Document;
use crate::json::Limits;
use crate::logic::{Kleene, Logic};
use crate::rules::RuleSet;
use crate::table::DecisionTable;
use crate::value::Value;

fn ruleset(text: &str) -> RuleSet {
    match crate::text::parse(text, &Limits::default()).unwrap() {
        Document::RuleSet(ruleset) => ruleset,
        Document::Rule(rule) => RuleSet::new(vec![rule], crate::rules::Config::default()).unwrap(),
        other => panic!("not a rule set: {other:?}"),
    }
}

fn run(text: &str) -> Audit {
    run_with(text, &AuditLimits::default())
}

fn run_with(text: &str, limits: &AuditLimits) -> Audit {
    let ruleset = ruleset(text);
    let report = audit(&ruleset, limits).unwrap();
    check_witnesses(&ruleset, &report);
    report
}

fn table(csv: &str) -> Audit {
    let table = DecisionTable::from_csv(csv, &Limits::default()).unwrap();
    let report = audit_table(&table, &AuditLimits::default()).unwrap();
    check_witnesses(table.ruleset(), &report);
    report
}

fn fires(ruleset: &RuleSet, name: &str, input: &Value) -> bool {
    let rule = &ruleset.rules()[ruleset.rule_position(name).unwrap()];
    let tree = evaluate_tree(rule.when(), input, &Kleene, Options::default()).unwrap();
    Kleene.fires(tree.truth().unwrap())
}

/// Every witness shows its finding (`spec/audit.md`, *Witnesses*).
fn check_witnesses(ruleset: &RuleSet, report: &Audit) {
    for d in &report.diagnostics {
        match d.code {
            DiagnosticCode::Overlap => {
                let w = d.witness.as_ref().unwrap();
                assert!(d.rules.iter().all(|r| fires(ruleset, r, w)), "{d:?}");
            }
            DiagnosticCode::Gap => {
                let w = d.witness.as_ref().unwrap();
                assert!(
                    ruleset.rules().iter().all(|r| !fires(ruleset, r.name(), w)),
                    "{d:?}"
                );
            }
            _ => assert!(d.witness.is_none()),
        }
    }
}

fn codes(report: &Audit) -> Vec<(&'static str, Severity, Vec<String>)> {
    report
        .diagnostics
        .iter()
        .map(|d| (d.code.as_str(), d.severity, d.rules.clone()))
        .collect()
}

fn names(names: &[&str]) -> Vec<String> {
    names.iter().map(|s| (*s).to_owned()).collect()
}

fn diagnostic(report: &Audit, code: DiagnosticCode) -> &super::Diagnostic {
    report
        .diagnostics
        .iter()
        .find(|d| d.code == code)
        .unwrap_or_else(|| panic!("no {code:?} in {report}"))
}

#[test]
fn unique_overlap_is_an_error_and_gaps_are_found() {
    let report = run(
        "HIT POLICY unique\nRULE adult IF age >= 18 THEN band = 'adult'\n\
         RULE senior ID \"AGE-2\" IF age >= 65 THEN band = 'senior'",
    );
    assert_eq!(
        codes(&report),
        [
            ("overlap", Severity::Error, names(&["adult", "senior"])),
            ("gap", Severity::Warning, names(&[])),
        ]
    );
    assert_eq!(report.rules[1].id.as_deref(), Some("AGE-2"));
    assert!(report.undecided.is_empty() && report.not_modelled.is_empty());
    assert_eq!(
        report.status(AuditCheck::Shadowed),
        CheckStatus::NotApplicable
    );
    assert_eq!(
        report.status(AuditCheck::Fields),
        CheckStatus::NotApplicable
    );
    assert_eq!(report.status(AuditCheck::Overlap), CheckStatus::Complete);
}

#[test]
fn exclusive_and_complete_rules_have_no_findings() {
    let report = run(
        "HIT POLICY unique\nRULE minor IF age < 18\nRULE adult IF age >= 18 AND age < 65\n\
         RULE senior IF age >= 65\nRULE unknown IF age IS NULL",
    );
    assert!(report.diagnostics.is_empty(), "{report}");
    assert!(
        AuditCheck::ALL
            .iter()
            .all(|c| report.status(*c) != CheckStatus::Partial)
    );
    assert!(report.to_string().contains("No findings."));
}

#[test]
fn unsatisfiable_conditions() {
    let report = run(
        "RULE a IF x > 5 AND x < 3\nRULE b IF x BETWEEN 10 AND 1\nRULE c IF x = 'a' AND x > 5\n\
         RULE d IF FALSE\nRULE e IF x IN (1, 2) AND NOT x IN (1, 2)\nRULE f IF x != 5 OR x = 5\n\
         RULE g IF d > DATE \"9999-12-31\"\n\
         RULE h IF d > DATE \"2026-01-01\" AND d < DATE \"2026-01-02\"",
    );
    let never: Vec<&String> = report
        .diagnostics
        .iter()
        .filter(|d| d.code == DiagnosticCode::Unsatisfiable)
        .flat_map(|d| &d.rules)
        .collect();
    assert_eq!(never, ["a", "b", "c", "d", "e", "g", "h"]);
    // `f` is TRUE for every number: under first it then hides nothing
    // after it, and nothing before it fires.
    assert_eq!(
        report.status(AuditCheck::Unsatisfiable),
        CheckStatus::Complete
    );
}

#[test]
fn incompatible_and_mixed_types() {
    let report = run(
        "RULE a IF flag > TRUE\nRULE b IF x + 1 = 'one'\nRULE c IF 1 < 'a'\nRULE d IF x > 5\n\
         RULE e IF x = '5'\nRULE f IF x / 0 > 1 OR y > 1 / 0",
    );
    let incompatible: Vec<(&String, &str)> = report
        .diagnostics
        .iter()
        .filter(|d| d.code == DiagnosticCode::IncompatibleTypes)
        .map(|d| (&d.rules[0], d.pointer.as_deref().unwrap()))
        .collect();
    assert_eq!(
        incompatible,
        [
            (&"a".to_owned(), "/ruleset/rules/0/when"),
            (&"b".to_owned(), "/ruleset/rules/1/when"),
            (&"c".to_owned(), "/ruleset/rules/2/when"),
            (&"f".to_owned(), "/ruleset/rules/5/when/operands/1"),
        ]
    );
    let mixed = diagnostic(&report, DiagnosticCode::MixedTypes);
    assert_eq!(mixed.rules, names(&["b", "d", "e", "f"]));
    assert_eq!(
        mixed.groups,
        [crate::value::Group::Number, crate::value::Group::String]
    );
    let text = report.to_string();
    assert!(text.contains("groups number, string"), "{text}");
}

#[test]
fn shadowed_rules_by_policy() {
    let first = run("RULE a IF x > 3\nRULE b IF x > 5\nRULE c IF TRUE");
    let shadowed = diagnostic(&first, DiagnosticCode::Shadowed);
    assert_eq!(shadowed.rules, names(&["b", "a"]));
    assert!(shadowed.message.contains("move it before a"));
    let infos = first
        .diagnostics
        .iter()
        .filter(|d| d.code == DiagnosticCode::Overlap)
        .all(|d| d.severity == Severity::Info);
    assert!(infos);
    assert!(
        first
            .diagnostics
            .iter()
            .all(|d| d.code != DiagnosticCode::Gap)
    );

    let priority = run(
        "HIT POLICY priority\nRULE low PRIORITY 1 IF x > 5\nRULE high PRIORITY 2 IF x > 3\n\
         RULE tie PRIORITY 2 IF x > 4",
    );
    let shadowed: Vec<&Vec<String>> = priority
        .diagnostics
        .iter()
        .filter(|d| d.code == DiagnosticCode::Shadowed)
        .map(|d| &d.rules)
        .collect();
    assert_eq!(
        shadowed,
        [&names(&["low", "high", "tie"]), &names(&["tie", "high"])]
    );
    assert!(
        priority.diagnostics[0]
            .message
            .contains("higher priority than high and tie")
    );

    for policy in ["rule_order", "collect", "unique"] {
        let report = run(&format!(
            "HIT POLICY {policy}\nRULE a IF x > 3\nRULE b IF x > 5"
        ));
        assert_eq!(
            report.status(AuditCheck::Shadowed),
            CheckStatus::NotApplicable
        );
    }
    for policy in ["rule_order", "collect"] {
        let report = run(&format!(
            "HIT POLICY {policy}\nRULE a IF x > 3\nRULE b IF x > 5"
        ));
        assert_eq!(
            report.status(AuditCheck::Overlap),
            CheckStatus::NotApplicable
        );
        assert_eq!(
            diagnostic(&report, DiagnosticCode::Gap).severity,
            Severity::Info
        );
    }
}

#[test]
fn any_overlaps_compare_payloads() {
    let severity = |report: &Audit, a: &str, b: &str| {
        report
            .diagnostics
            .iter()
            .find(|d| d.code == DiagnosticCode::Overlap && d.rules == names(&[a, b]))
            .map(|d| d.severity)
    };
    let agree = run(
        "HIT POLICY any\nRULE a IF x > 3 THEN y = 1\nRULE b IF x > 4 THEN y = 1\n\
         RULE d IF x > 6 THEN y = x - x + 1",
    );
    assert_eq!(severity(&agree, "a", "b"), Some(Severity::Info));
    assert_eq!(severity(&agree, "a", "d"), Some(Severity::Warning));
    let differ = run(
        "HIT POLICY any\nRULE a IF x > 3 THEN y = 1\nRULE c IF x > 5 THEN y = 2\n\
         RULE d IF x > 6 THEN y = x - x + 1",
    );
    assert_eq!(severity(&differ, "a", "c"), Some(Severity::Error));
    assert_eq!(severity(&differ, "c", "d"), Some(Severity::Error));
    // The rule set's own evaluation of a witness for a and d raises, because
    // c fires too with another output: that witness does not show what a
    // warning about a and d says, so the pair is left undecided.
    assert_eq!(severity(&differ, "a", "d"), None);
    assert!(
        differ
            .undecided
            .iter()
            .any(|u| u.check == AuditCheck::Overlap
                && u.rules == names(&["a", "d"])
                && u.reason == UndecidedReason::NoWitness)
    );
}

#[test]
fn priority_overlaps_say_who_wins() {
    let report = run(
        "HIT POLICY priority\nRULE a PRIORITY 1 IF x > 3\nRULE b PRIORITY 2 IF x > 4\nRULE c PRIORITY 2 IF x > 5",
    );
    let messages: Vec<&str> = report
        .diagnostics
        .iter()
        .filter(|d| d.code == DiagnosticCode::Overlap)
        .map(|d| d.message.as_str())
        .collect();
    assert!(messages[0].contains("b takes precedence (the higher priority wins)"));
    assert!(
        messages[2].contains("b takes precedence (with the same priority, rule order decides)")
    );
}

#[test]
fn groups_other_than_numbers() {
    let report = run(
        "HIT POLICY unique\nRULE a IF d BETWEEN DATE \"2026-01-01\" AND DATE \"2026-12-31\" CLOSED LEFT\n\
         RULE b IF d >= DATE \"2026-12-31\"\nRULE c IF t > DATETIME \"2026-01-01T00:00:00\"\n\
         RULE e IF z < DATETIME \"2026-01-01T00:00:00Z\"\nRULE f IF s IN ('a', 'b') AND flag\n\
         RULE g IF s NOT IN ('a', 'b') AND NOT flag\nRULE h IF flag = FALSE AND s = 'a'",
    );
    let overlaps: Vec<&Vec<String>> = report
        .diagnostics
        .iter()
        .filter(|d| d.code == DiagnosticCode::Overlap)
        .map(|d| &d.rules)
        .collect();
    // Every pair of rules on different fields overlaps; a and b do not, f, g
    // and h are exclusive.
    assert!(!overlaps.contains(&&names(&["a", "b"])));
    assert!(!overlaps.contains(&&names(&["f", "g"])));
    assert!(!overlaps.contains(&&names(&["f", "h"])));
    assert!(!overlaps.contains(&&names(&["g", "h"])));
    assert!(overlaps.contains(&&names(&["a", "c"])));
    assert!(overlaps.contains(&&names(&["c", "e"])));
    assert_eq!(report.status(AuditCheck::Overlap), CheckStatus::Complete);
}

#[test]
fn not_modelled_tests_are_listed_and_left_undecided() {
    let report = run(
        "RULE a IF name MATCHES 'a' AND x > 1\nRULE b IF a > b\nRULE c IF x BETWEEN lo AND 5\n\
         RULE d IF name STARTS WITH 'z'",
    );
    let listed: Vec<(&str, &str, &str)> = report
        .not_modelled
        .iter()
        .map(|n| (n.rule.as_str(), n.op, n.pointer.as_str()))
        .collect();
    assert_eq!(
        listed,
        [
            ("a", "matches", "/ruleset/rules/0/when/operands/0"),
            ("b", "gt", "/ruleset/rules/1/when"),
            ("c", "between", "/ruleset/rules/2/when"),
            ("d", "starts_with", "/ruleset/rules/3/when"),
        ]
    );
    assert!(
        report
            .undecided
            .iter()
            .all(|u| u.reason == UndecidedReason::NotModelled)
    );
    assert_eq!(report.status(AuditCheck::Types), CheckStatus::Complete);
    let text = report.to_string();
    assert!(
        text.contains("Not modelled (4):") && text.contains("Undecided ("),
        "{text}"
    );
}

#[test]
fn witnesses_that_cannot_be_built() {
    // An expression subject: the boxes meet, but no input is built.
    let report = run("HIT POLICY unique\nRULE a IF x * 2 > 10\nRULE b IF x * 2 > 20");
    let checks: Vec<(AuditCheck, UndecidedReason)> = report
        .undecided
        .iter()
        .map(|u| (u.check, u.reason))
        .collect();
    assert_eq!(
        checks,
        [
            (AuditCheck::Unsatisfiable, UndecidedReason::NoWitness),
            (AuditCheck::Unsatisfiable, UndecidedReason::NoWitness),
            (AuditCheck::Overlap, UndecidedReason::NoWitness),
        ]
    );
    // The gap is shown by chance: its fields get values of a supported input.
    assert_eq!(diagnostic(&report, DiagnosticCode::Gap).rules, names(&[]));
    // Related paths: `c IS NULL` with `c.age > 1` cannot both hold.
    let report = run("HIT POLICY unique\nRULE a IF c IS NULL\nRULE b IF c.age > 1");
    let checks: Vec<(AuditCheck, UndecidedReason)> = report
        .undecided
        .iter()
        .map(|u| (u.check, u.reason))
        .collect();
    assert_eq!(
        checks,
        [
            (AuditCheck::Overlap, UndecidedReason::NoWitness),
            (AuditCheck::Gap, UndecidedReason::NoWitness),
        ]
    );
    // Lists are indexed.
    let report =
        run("HIT POLICY unique\nRULE a IF items[2].price > 1\nRULE b IF items[0].price > 1");
    let overlap = diagnostic(&report, DiagnosticCode::Overlap);
    let crate::value::Value::Map(map) = overlap.witness.as_ref().unwrap() else {
        panic!()
    };
    let Some(crate::value::Value::List(items)) = map.get("items") else {
        panic!()
    };
    assert_eq!(items.len(), 3);
    assert!(items[1].is_null());
}

#[test]
fn limits_are_reported() {
    let text =
        "HIT POLICY unique\nRULE a IF (x = 1 OR x = 2) AND (y = 1 OR y = 2)\nRULE b IF x > 0";
    let report = run_with(text, &AuditLimits::default().with_max_terms(3));
    assert_eq!(report.not_modelled.len(), 1);
    assert_eq!(report.not_modelled[0].op, "and");
    assert_eq!(report.not_modelled[0].limit, Some(AuditLimit::MaxTerms));
    assert!(
        report
            .undecided
            .iter()
            .any(|u| u.reason == UndecidedReason::Limit(AuditLimit::MaxTerms))
    );
    let report = run_with(text, &AuditLimits::default().with_max_steps(0));
    assert!(
        report
            .not_modelled
            .iter()
            .all(|n| n.limit == Some(AuditLimit::MaxSteps))
    );
    assert!(
        report
            .undecided
            .iter()
            .all(|u| u.reason == UndecidedReason::Limit(AuditLimit::MaxSteps))
    );
    assert!(
        report
            .undecided
            .iter()
            .any(|u| u.check == AuditCheck::Gap && u.rules.is_empty())
    );
    let text = report.to_string();
    assert!(text.contains("(max_steps)"), "{text}");
    // A few steps: the reading fits, the checks run out.
    let report = run_with(
        "HIT POLICY unique\nRULE a IF x > 1\nRULE b IF x > 2\nRULE c IF x > 3",
        &AuditLimits::default().with_max_steps(3),
    );
    assert_eq!(report.not_modelled, []);
    assert!(
        report
            .undecided
            .iter()
            .any(|u| u.reason == UndecidedReason::Limit(AuditLimit::MaxSteps))
    );
}

#[test]
fn empty_rule_sets_have_a_gap() {
    let report = run("HIT POLICY first");
    let gap = diagnostic(&report, DiagnosticCode::Gap);
    assert!(matches!(gap.witness, Some(crate::value::Value::Map(ref m)) if m.is_empty()));
}

#[test]
fn decision_tables() {
    let report = table(
        "HIT POLICY,unique\nRULE,country,age,THEN label\nes,ES,>= 18,'adult'\npt,'PT',>= 18,VIP\n\
         young,-,< 18,'young'\n,'PT',> 60,'senior'\n",
    );
    assert_eq!(report.status(AuditCheck::Fields), CheckStatus::Complete);
    let unlisted: Vec<(&str, &str)> = report
        .diagnostics
        .iter()
        .filter(|d| d.code == DiagnosticCode::UnlistedField)
        .map(|d| (d.rules[0].as_str(), d.pointer.as_deref().unwrap()))
        .collect();
    assert_eq!(
        unlisted,
        [
            ("es", "/decision_table/rows/0/inputs/0"),
            ("pt", "/decision_table/rows/1/outputs/0"),
        ]
    );
    assert_eq!(report.rules[3].name, "row_4");
    assert_eq!(report.rules[3].row, Some(3));
    let overlaps: Vec<&Vec<String>> = report
        .diagnostics
        .iter()
        .filter(|d| d.code == DiagnosticCode::Overlap)
        .map(|d| &d.rules)
        .collect();
    assert_eq!(overlaps, [&names(&["pt", "row_4"])]);
    // `ES` is a field: the cell compares two fields, which is not modelled.
    assert_eq!(
        report.not_modelled[0].pointer,
        "/decision_table/rows/0/inputs/0"
    );
    assert!(
        report
            .undecided
            .iter()
            .any(|u| u.rules == names(&["es", "row_4"]))
    );
    assert!(
        report
            .to_string()
            .starts_with("Audit of a decision table: 4 rules")
    );

    // Output order: rows sorted by output priority, and tests in a cell
    // point to the cell.
    let report = table(
        "HIT POLICY,O\nRULE,x,\"THEN level IN ('high', 'low')\"\nlow,> 1,'low'\nhigh,> 5,'high'\n\
         never,BETWEEN 5 AND 3,'low'\n",
    );
    let names: Vec<(&str, Option<usize>)> = report
        .rules
        .iter()
        .map(|r| (r.name.as_str(), r.row))
        .collect();
    assert_eq!(
        names,
        [("high", Some(1)), ("low", Some(0)), ("never", Some(2))]
    );
    assert_eq!(report.hit_policy, crate::table::TablePolicy::OutputOrder);
    assert_eq!(
        report.status(AuditCheck::Overlap),
        CheckStatus::NotApplicable
    );
}

#[test]
fn table_rows_combine_their_cells() {
    let report = table("HIT POLICY,first\nx,y,THEN z\n> 1,> 1,1\n> 1,-,2\n-,-,3\n");
    let shadowed: Vec<&Vec<String>> = report
        .diagnostics
        .iter()
        .filter(|d| d.code == DiagnosticCode::Shadowed)
        .map(|d| &d.rules)
        .collect();
    assert!(shadowed.is_empty(), "{report}");
    assert!(
        report
            .diagnostics
            .iter()
            .all(|d| d.code != DiagnosticCode::Gap)
    );
    let report = table("HIT POLICY,first\nx,y,THEN z\n> 1,-,2\n> 1,> 1,1\n");
    assert_eq!(
        diagnostic(&report, DiagnosticCode::Shadowed).rules,
        names(&["row_2", "row_1"])
    );
}

#[test]
fn the_text_of_an_audit() {
    let report = run(
        "HIT POLICY collect\nAGGREGATE sum\nSTRICT\nRULE a IF x > 1 THEN n = 1\nRULE b IF name MATCHES 'a' THEN n = 2",
    );
    insta::assert_snapshot!(report.to_string());
}

#[test]
fn more_shapes_of_tests() {
    // Constants on the left, floats, open and half-open betweens, aware
    // datetimes, a list element as the field.
    let report = run(
        "HIT POLICY unique\nRULE a IF 5 < x AND x <= 7.5e0\nRULE b IF x BETWEEN 7 AND 9 CLOSED RIGHT\n\
         RULE c IF x BETWEEN 9 AND 11 CLOSED NONE\nRULE d IF z > DATETIME \"2026-01-01T00:00:00Z\"\n\
         RULE e IF z <= DATETIME \"2026-01-01T00:00:00+01:00\"\nRULE f IF items[0] > 1 AND items[1] > 1",
    );
    let overlaps: Vec<&Vec<String>> = report
        .diagnostics
        .iter()
        .filter(|d| d.code == DiagnosticCode::Overlap)
        .map(|d| &d.rules)
        .collect();
    assert!(overlaps.contains(&&names(&["a", "b"])), "{report}");
    assert!(!overlaps.contains(&&names(&["b", "c"])));
    assert!(!overlaps.contains(&&names(&["d", "e"])));
    assert!(overlaps.contains(&&names(&["a", "d"])));
    assert!(overlaps.contains(&&names(&["e", "f"])));
    assert!(report.undecided.is_empty(), "{report}");
    // `NOT IN` in a cell: the not's operand points to the cell too.
    let report = table("HIT POLICY,unique\nx,y\n\"NOT IN (1, 2)\",-\n-,\"x\"\n");
    let not_modelled: Vec<&str> = report
        .not_modelled
        .iter()
        .map(|n| n.pointer.as_str())
        .collect();
    assert_eq!(not_modelled, ["/decision_table/rows/1/inputs/1"]);
}

#[test]
fn terms_beyond_the_limit_in_a_disjunction() {
    let report = run_with(
        "HIT POLICY first\nRULE a IF x = 1 OR x = 2 OR x = 3 OR x = 4",
        &AuditLimits::default().with_max_terms(3),
    );
    assert_eq!(report.not_modelled[0].op, "or");
    assert_eq!(report.not_modelled[0].limit, Some(AuditLimit::MaxTerms));
}

#[test]
fn steps_may_run_out_anywhere() {
    let text = "HIT POLICY first\nRULE a IF x > 1 AND y > 1\nRULE b IF x > 2 OR y > 3\n\
                RULE c IF x > 3 AND NOT y IN (1, 2)\nRULE d IF TRUE";
    let full = run(text);
    assert!(full.undecided.is_empty(), "{full}");
    let mut seen = std::collections::BTreeSet::new();
    for steps in 0..400 {
        let report = run_with(text, &AuditLimits::default().with_max_steps(steps));
        for u in &report.undecided {
            assert_eq!(
                u.reason,
                UndecidedReason::Limit(AuditLimit::MaxSteps),
                "{report}"
            );
            seen.insert(u.check);
        }
        // Whatever was found with fewer steps is found with all of them.
        for d in &report.diagnostics {
            assert!(
                full.diagnostics
                    .iter()
                    .any(|f| f.code == d.code && f.rules == d.rules && f.severity == d.severity),
                "{d:?} with {steps} steps"
            );
        }
    }
    assert!(seen.contains(&AuditCheck::Shadowed) && seen.contains(&AuditCheck::Overlap));
    assert!(seen.contains(&AuditCheck::Gap) && seen.contains(&AuditCheck::Unsatisfiable));
}

#[test]
fn witnesses_do_not_overwrite_what_they_hold() {
    let mut root = super::check::Tree::new();
    let key = |k: &str| crate::error::Segment::Key(k.into());
    let index = crate::error::Segment::Index;
    assert!(root.place(&[key("a"), index(1)], Value::Int(1)));
    assert!(!root.place(&[key("a"), index(1)], Value::Int(2)));
    assert!(!root.place(&[key("a"), key("b")], Value::Int(2)));
    assert!(!root.place(&[key("a"), index(1), key("c")], Value::Int(2)));
    assert!(root.place(&[key("a"), index(0), key("c")], Value::Int(3)));
    assert!(!root.place(&[key("a"), index(5000)], Value::Int(2)));
    assert!(!root.place(&[], Value::Int(2)));
    let Value::Map(map) = root.into_value() else {
        panic!()
    };
    let Some(Value::List(items)) = map.get("a") else {
        panic!()
    };
    assert!(matches!(items[1], Value::Int(1)));
}

#[test]
fn the_text_of_a_table_audit() {
    let report = table(
        "HIT POLICY,collect\nRULE,country,age,THEN label\nes,ES,>= 18,'adult'\npt,'PT',> TRUE,VIP\n\
         young,-,< '18',\n",
    );
    insta::assert_snapshot!(report.to_string());
}

#[test]
fn witnesses_are_evaluated_with_strictness() {
    // Not strict: y = 0 makes `y = 'k'` UNKNOWN, and a and b fire together.
    let text = "RULE a IF x > 5\nRULE b IF x > 3 OR y = 'k'\nRULE c IF y > 2";
    let lax = run(&format!("HIT POLICY unique\n{text}"));
    assert!(
        lax.diagnostics
            .iter()
            .any(|d| d.code == DiagnosticCode::Overlap && d.severity == Severity::Error)
    );
    // Strict: every input raises type_mismatch, so no overlap is shown.
    let strict = run(&format!("HIT POLICY unique\nSTRICT\n{text}"));
    assert!(
        strict
            .diagnostics
            .iter()
            .all(|d| d.code == DiagnosticCode::MixedTypes),
        "{strict}"
    );
    assert!(
        strict
            .undecided
            .iter()
            .all(|u| u.reason == UndecidedReason::NoWitness)
    );
    assert!(strict.strict);
}

#[test]
fn messages_follow_the_policy() {
    let first = run("RULE a IF x > 1\nRULE b IF x > 2");
    let overlap = diagnostic(&first, DiagnosticCode::Overlap);
    assert!(
        overlap
            .message
            .contains("a takes precedence over b (first hit policy)")
    );
    assert!(
        diagnostic(&first, DiagnosticCode::Gap)
            .message
            .contains("a last rule with the condition TRUE")
    );
    let priority = run("HIT POLICY priority\nRULE a IF x > 1");
    assert!(
        diagnostic(&priority, DiagnosticCode::Gap)
            .message
            .contains("the lowest priority")
    );
    let unique = run("HIT POLICY unique\nRULE a IF x > 1");
    assert!(
        diagnostic(&unique, DiagnosticCode::Gap)
            .message
            .contains("would fire with every other rule")
    );
    let shadowed = run("RULE a IF x > 1\nRULE b IF x > 2\nRULE c IF x > 3");
    let c = shadowed
        .diagnostics
        .iter()
        .find(|d| d.code == DiagnosticCode::Shadowed && d.rules[0] == "c")
        .unwrap();
    assert!(
        c.message
            .contains("at least one of rules a and b fires too")
    );
    let null = run("RULE a IF x > 1 / 0");
    assert!(
        diagnostic(&null, DiagnosticCode::IncompatibleTypes)
            .message
            .contains("its constant is null")
    );
}

#[test]
fn names_digests_and_severities() {
    for &code in super::DiagnosticCode::ALL {
        assert_eq!(DiagnosticCode::from_name(code.as_str()), Some(code));
    }
    for &check in AuditCheck::ALL {
        assert_eq!(AuditCheck::from_name(check.as_str()), Some(check));
    }
    for severity in [Severity::Error, Severity::Warning, Severity::Info] {
        assert_eq!(Severity::from_name(severity.as_str()), Some(severity));
    }
    assert_eq!(Severity::from_name("fatal"), None);
    assert_eq!(AuditCheck::from_name("spelling"), None);
    let text = "HIT POLICY unique\nRULE a IF x > 1\nRULE b IF x > 2";
    let ruleset = ruleset(text);
    let report = audit(&ruleset, &AuditLimits::default()).unwrap();
    assert_eq!(report.max_severity(), Some(Severity::Error));
    assert_eq!(report.ruleset_digest, None);
    let carried = report.clone().with_ruleset_digest(true);
    assert_eq!(carried.ruleset_digest, Some(ruleset.digest()));
    assert_eq!(carried.clone().with_ruleset_digest(false), report);
    let json =
        crate::format::write_audit_as(&carried, crate::format::DocumentFormat::Json).unwrap();
    assert!(json.contains(&format!("\"ruleset_digest\":\"{}\"", ruleset.digest())));
    assert_ne!(carried, report);
    let mut other = report.clone();
    other.diagnostics[0].witness = Some(Value::Int(1));
    assert_ne!(other, report);
    assert_eq!(
        run("RULE a IF x > 1\nRULE b IF TRUE").max_severity(),
        Some(Severity::Info)
    );
    assert_eq!(
        run("HIT POLICY first").max_severity(),
        Some(Severity::Warning)
    );
    // Ordered by seriousness, so the most serious is the maximum.
    assert!(Severity::Info < Severity::Warning && Severity::Warning < Severity::Error);
    assert!(report.at_least(Severity::Error) && report.at_least(Severity::Info));
    let info = run("RULE a IF x > 1\nRULE b IF TRUE");
    assert!(info.at_least(Severity::Info) && !info.at_least(Severity::Warning));
    let first = run("HIT POLICY first");
    assert!(first.at_least(Severity::Warning) && !first.at_least(Severity::Error));
    let none = run("HIT POLICY unique\nRULE a IF x < 1\nRULE b IF x >= 1");
    assert_eq!(none.max_severity(), None);
    assert!(!none.at_least(Severity::Info));
    // The text of an audit writes each diagnostic as its own text does.
    let text = report.to_string();
    for d in &report.diagnostics {
        let line = d.to_string();
        assert!(line.starts_with("error    overlap  a, b: ") || line.starts_with("warning  gap  "));
        assert!(line.ends_with('\n') && text.contains(&line));
    }
}

#[test]
fn a_gap_witness_gives_every_field_a_value() {
    // `c` and `c.age` cannot both have a value: no supported input, so no
    // gap is reported, though one exists.
    let report = run("HIT POLICY first\nRULE a IF c = 1\nRULE b IF c.age > 1");
    assert!(
        report
            .diagnostics
            .iter()
            .all(|d| d.code != DiagnosticCode::Gap)
    );
    assert!(
        report
            .undecided
            .iter()
            .any(|u| u.check == AuditCheck::Gap && u.reason == UndecidedReason::NoWitness)
    );
}

#[test]
fn the_text_of_an_audit_with_witnesses() {
    let report = run(
        "HIT POLICY unique\nRULE adult IF age >= 18 THEN band = 'adult'\n\
         RULE senior ID \"AGE-2\" IF age >= 65 THEN band = 'senior'",
    );
    let limited = run_with(
        "HIT POLICY unique\nRULE adult IF age >= 18\nRULE senior IF age >= 65",
        &AuditLimits::default().with_max_steps(0),
    );
    insta::assert_snapshot!(format!("{report}{limited}"));
}

#[test]
fn documents_are_audited_with_pointers_into_them() {
    use crate::bundle::{Bundle, BundleId, Revision};
    let limits = Limits::default();
    let first = |document: &Document| {
        let report = super::audit_document(document, &AuditLimits::default()).unwrap();
        let pointer = report.diagnostics[0].pointer.clone();
        let unmodelled = report.not_modelled.first().map(|n| n.pointer.clone());
        (report, pointer, unmodelled)
    };
    // A rule: the rule set of that one rule, under `/rule`.
    let rule = crate::text::parse("RULE odd IF x > TRUE OR name MATCHES 'a'", &limits).unwrap();
    let (report, pointer, unmodelled) = first(&rule);
    assert_eq!(report.source.as_str(), "ruleset");
    assert_eq!(pointer.as_deref(), Some("/rule/when/operands/0"));
    assert_eq!(unmodelled.as_deref(), Some("/rule/when/operands/1"));
    // A rule set, as `audit`.
    let rules = crate::text::parse(
        "HIT POLICY first\nRULE a IF x > TRUE\nRULE b IF y > 1",
        &limits,
    )
    .unwrap();
    let (report, pointer, _) = first(&rules);
    assert_eq!(pointer.as_deref(), Some("/ruleset/rules/0/when"));
    assert_eq!(report.document, None);
    // A bundle of a rule set, and of a table: prefixed with `/bundle`.
    let bundle = |rules: crate::bundle::BundleRules| {
        Document::Bundle(Box::new(Bundle::new(
            rules,
            BundleId::new("acme/audit").unwrap(),
            Revision::new("1").unwrap(),
        )))
    };
    let (report, pointer, _) = first(&bundle(rules.into_ruleset().unwrap().into()));
    assert_eq!(pointer.as_deref(), Some("/bundle/ruleset/rules/0/when"));
    assert_eq!(
        report.document.map(super::AuditDocument::as_str),
        Some("bundle")
    );
    assert!(
        report
            .to_string()
            .starts_with("Audit of a rule set in a bundle: 2 rules, hit policy first")
    );
    let csv = "HIT POLICY,first\nRULE,country,age,THEN label\nes,ES,> TRUE,'adult'\n";
    let table = DecisionTable::from_csv(csv, &limits).unwrap();
    let (report, pointer, unmodelled) = first(&Document::DecisionTable(table.clone()));
    assert_eq!(pointer.as_deref(), Some("/decision_table/rows/0/inputs/1"));
    assert_eq!(
        unmodelled.as_deref(),
        Some("/decision_table/rows/0/inputs/0")
    );
    let (bundled, pointer, unmodelled) = first(&bundle(table.into()));
    assert_eq!(
        pointer.as_deref(),
        Some("/bundle/decision_table/rows/0/inputs/1")
    );
    assert_eq!(
        unmodelled.as_deref(),
        Some("/bundle/decision_table/rows/0/inputs/0")
    );
    // The text names a bundled table's cells as it names the table's own.
    assert!(
        report
            .to_string()
            .contains("at row es, input age (/decision_table/rows/0/inputs/1)")
    );
    assert!(
        bundled
            .to_string()
            .contains("at row es, input age (/bundle/decision_table/rows/0/inputs/1)")
    );
    // A condition and a case file hold no rules.
    for document in [
        crate::text::parse("x > 1", &limits).unwrap(),
        crate::format::read_document(r#"{"version": 1, "cases": {"items": []}}"#, &limits).unwrap(),
    ] {
        let error = super::audit_document(&document, &AuditLimits::default()).unwrap_err();
        assert_eq!(error.code().as_str(), "unsupported");
        assert!(error.hint().is_some());
    }
}

#[test]
fn a_rule_document_has_no_checks_between_rules() {
    let limits = Limits::default();
    let rule = crate::text::parse("RULE odd IF x > TRUE OR name MATCHES 'a'", &limits).unwrap();
    let report = super::audit_document(&rule, &AuditLimits::default()).unwrap();
    assert_eq!(
        report.document.map(super::AuditDocument::as_str),
        Some("rule")
    );
    // The checks between rules do not apply to a rule: no gap of a default
    // policy, which the rule did not choose.
    let statuses: Vec<(&str, &str)> = report
        .checks
        .iter()
        .map(|c| (c.check.as_str(), c.status.as_str()))
        .collect();
    assert_eq!(
        statuses,
        [
            ("types", "complete"),
            ("fields", "not_applicable"),
            ("unsatisfiable", "partial"),
            ("shadowed", "not_applicable"),
            ("overlap", "not_applicable"),
            ("gap", "not_applicable"),
        ]
    );
    assert!(
        report
            .diagnostics
            .iter()
            .all(|d| d.code == DiagnosticCode::IncompatibleTypes)
    );
    assert!(
        report
            .undecided
            .iter()
            .all(|u| u.check == AuditCheck::Unsatisfiable)
    );
    assert!(
        report
            .to_string()
            .starts_with("Audit of a rule, logic boolean\n")
    );
    let plain = crate::text::parse("RULE a IF x > 1", &limits).unwrap();
    let report = super::audit_document(&plain, &AuditLimits::default()).unwrap();
    assert!(report.diagnostics.is_empty() && report.undecided.is_empty());
}

#[test]
fn places_that_are_not_cells_are_written_as_pointers() {
    let csv = "HIT POLICY,first\nRULE,age,THEN label\nadult,>= 18,'adult'\n";
    let table = DecisionTable::from_csv(csv, &Limits::default()).unwrap();
    let report = audit_table(&table, &AuditLimits::default()).unwrap();
    for (pointer, place) in [
        (
            "/decision_table/rows/0",
            "row adult (/decision_table/rows/0)",
        ),
        (
            "/decision_table/rows/0/inputs/0",
            "row adult, input age (/decision_table/rows/0/inputs/0)",
        ),
        (
            "/decision_table/rows/0/outputs/0",
            "row adult, output label (/decision_table/rows/0/outputs/0)",
        ),
        (
            "/decision_table/rows/1/inputs/0",
            "/decision_table/rows/1/inputs/0",
        ),
        ("/decision_table/rows/x", "/decision_table/rows/x"),
        (
            "/decision_table/rows/0/inputs/1",
            "/decision_table/rows/0/inputs/1",
        ),
        (
            "/decision_table/rows/0/outputs/1",
            "/decision_table/rows/0/outputs/1",
        ),
        (
            "/decision_table/rows/0/inputs/x",
            "/decision_table/rows/0/inputs/x",
        ),
        (
            "/decision_table/rows/0/inputs/0/1",
            "/decision_table/rows/0/inputs/0/1",
        ),
        ("/decision_table/inputs/0", "/decision_table/inputs/0"),
    ] {
        assert_eq!(report.place(pointer), place, "{pointer}");
    }
    let rules = audit(&ruleset("RULE a IF x > 1"), &AuditLimits::default()).unwrap();
    assert_eq!(
        rules.place("/decision_table/rows/0/inputs/0"),
        "/decision_table/rows/0/inputs/0"
    );
}

#[test]
fn a_diagnostics_text_names_its_cell_in_the_audit_only() {
    let report = table("RULE,country,THEN rate\nes,ES,1\npt,'PT',2\n");
    let unlisted = &report.diagnostics[0];
    assert_eq!(unlisted.code, DiagnosticCode::UnlistedField);
    let alone = unlisted.to_string();
    let in_audit = report.diagnostic_text(unlisted);
    assert!(alone.contains("\n           at /decision_table/rows/0/inputs/0\n"));
    assert!(
        in_audit
            .contains("\n           at row es, input country (/decision_table/rows/0/inputs/0)\n")
    );
    let text = report.to_string();
    assert!(text.contains(&in_audit) && !text.contains(&alone));
    // Without cells, both are the same.
    let ruleset = run("HIT POLICY unique\nRULE a IF x > 1\nRULE b IF x > 2");
    for d in &ruleset.diagnostics {
        assert_eq!(ruleset.diagnostic_text(d), d.to_string());
    }
}
