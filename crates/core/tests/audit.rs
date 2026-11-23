//! Properties of the static audit (`spec/audit.md`): on random rule sets of
//! modelled tests, every witness shows its finding, every claim that something
//! never happens holds on a grid of inputs that meets every region the tests
//! draw, and every check is complete.

// A test: a panic is the failure report.
#![allow(clippy::unwrap_used, clippy::panic)]

use cleanrules::audit::{
    Audit, AuditCheck, AuditLimits, CheckStatus, DiagnosticCode, UndecidedReason, audit,
};
use cleanrules::eval::{Options, evaluate_tree};
use cleanrules::format::Document;
use cleanrules::json::Limits;
use cleanrules::logic::{Kleene, Logic};
use cleanrules::rules::{HitPolicy, RuleSet};
use cleanrules::value::Decimal;
use cleanrules::{Map, Value};
use proptest::prelude::*;
use std::fmt::Write as _;

/// A condition in the rule syntax over `x` and `y` (numbers, constants 0 to
/// 3), `flag` (a boolean) and `s` (a string).
fn condition(opaque: bool) -> impl Strategy<Value = String> {
    let number = (prop::sample::select(vec!["x", "y"]), 0..4i64);
    let atom = prop_oneof![
        (
            number.clone(),
            prop::sample::select(vec!["=", "!=", "<", "<=", ">", ">="])
        )
            .prop_map(|((f, c), op)| format!("{f} {op} {c}")),
        (number.clone(), 0..4i64).prop_map(|((f, a), b)| format!("{f} BETWEEN {a} AND {b}")),
        (number, 0..4i64).prop_map(|((f, a), b)| format!("{f} IN ({a}, {b})")),
        prop::sample::select(vec!["x", "y", "flag", "s"]).prop_map(|f| format!("{f} IS NULL")),
        Just("flag".to_owned()),
        prop::sample::select(vec!["'a'", "'b'"]).prop_map(|c| format!("s = {c}")),
        Just("s IN ('a', 'b')".to_owned()),
        Just("TRUE".to_owned()),
    ];
    // Tests the audit does not model, when asked for.
    let unmodelled = prop::sample::select(if opaque {
        vec![
            "x > y",
            "s STARTS WITH 'a'",
            "x + y >= 2",
            "x BETWEEN y AND 3",
        ]
    } else {
        vec!["TRUE"]
    })
    .prop_map(str::to_owned);
    let atom = prop_oneof![4 => atom, 1 => unmodelled];
    atom.prop_recursive(3, 12, 3, |inner| {
        prop_oneof![
            inner.clone().prop_map(|c| format!("NOT ({c})")),
            (inner.clone(), inner.clone()).prop_map(|(a, b)| format!("({a}) AND ({b})")),
            (inner.clone(), inner).prop_map(|(a, b)| format!("({a}) OR ({b})")),
        ]
    })
}

fn ruleset(opaque: bool) -> impl Strategy<Value = String> {
    let policy = prop::sample::select(vec![
        "first",
        "priority",
        "unique",
        "any",
        "rule_order",
        "collect",
    ]);
    let rule = (condition(opaque), 0..3i64, 0..2i64);
    (policy, prop::collection::vec(rule, 0..5)).prop_map(|(policy, rules)| {
        let mut text = format!("HIT POLICY {policy}\n");
        for (i, (when, priority, out)) in rules.into_iter().enumerate() {
            let _ = writeln!(
                text,
                "RULE r{i} PRIORITY {priority}\nIF {when}\nTHEN v = {out}"
            );
        }
        text
    })
}

fn parse(text: &str) -> RuleSet {
    match cleanrules::text::parse(text, &Limits::default()).unwrap() {
        Document::RuleSet(ruleset) => ruleset,
        other => panic!("not a rule set: {other:?}"),
    }
}

fn fires(ruleset: &RuleSet, rule: usize, input: &Value) -> bool {
    let tree = evaluate_tree(
        ruleset.rules()[rule].when(),
        input,
        &Kleene,
        Options::default(),
    )
    .unwrap();
    Kleene.fires(tree.truth().unwrap())
}

fn position(ruleset: &RuleSet, name: &str) -> usize {
    ruleset.rule_position(name).unwrap()
}

/// Inputs meeting every region of the tests: values below, at, between and
/// above the constants, null, and a value of another group; and whether the
/// input is supported (every field present, of its group).
fn grid() -> Vec<(Value, bool)> {
    let half = |s: &str| Value::Decimal(Decimal::parse(s).unwrap());
    let numbers = || {
        vec![
            Value::Int(-1),
            Value::Int(0),
            half("0.5"),
            Value::Int(1),
            half("1.5"),
            Value::Int(2),
            half("2.5"),
            Value::Int(3),
            Value::Int(4),
        ]
    };
    let with_odd = |mut values: Vec<Value>, odd: Value| {
        let supported = values.len();
        values.push(Value::Null(cleanrules::NullCause::Null));
        values.push(odd);
        values
            .into_iter()
            .enumerate()
            .map(move |(i, v)| (v, i < supported))
    };
    let mut inputs = Vec::new();
    for (x, xs) in with_odd(numbers(), Value::String("a".into())) {
        for (y, ys) in with_odd(numbers(), Value::Bool(true)) {
            for (flag, fs) in with_odd(vec![Value::Bool(false), Value::Bool(true)], Value::Int(1)) {
                let strings = vec![
                    Value::String("a".into()),
                    Value::String("b".into()),
                    Value::String("c".into()),
                ];
                for (s, ss) in with_odd(strings, Value::Int(1)) {
                    let mut map = Map::new();
                    map.insert("x", x.clone());
                    map.insert("y", y.clone());
                    map.insert("flag", flag.clone());
                    map.insert("s", s);
                    inputs.push((Value::Map(map), xs && ys && fs && ss));
                }
            }
        }
    }
    inputs
}

fn ahead(ruleset: &RuleSet, rule: usize) -> Vec<usize> {
    let rules = ruleset.rules();
    match ruleset.hit_policy() {
        HitPolicy::Priority => (0..rules.len())
            .filter(|&i| {
                let (p, own) = (rules[i].get_priority(), rules[rule].get_priority());
                p > own || (p == own && i < rule)
            })
            .collect(),
        _ => (0..rule).collect(),
    }
}

fn check(ruleset: &RuleSet, report: &Audit, exact: bool) {
    let n = ruleset.rules().len();
    let rules =
        |names: &[String]| -> Vec<usize> { names.iter().map(|r| position(ruleset, r)).collect() };
    if exact {
        // Every test is modelled, so every check is decided; but under any,
        // the witness of a pair may fire a third rule with another output,
        // which raises, so it shows nothing about the pair.
        assert_eq!(report.not_modelled, []);
        let any = ruleset.hit_policy() == HitPolicy::Any;
        for u in &report.undecided {
            assert!(
                any && u.check == AuditCheck::Overlap && u.reason == UndecidedReason::NoWitness,
                "{report}"
            );
        }
        assert!(
            report.checks.iter().all(
                |c| c.status != CheckStatus::Partial || (any && c.check == AuditCheck::Overlap)
            )
        );
    }
    let undecided_pairs: Vec<Vec<usize>> = report
        .undecided
        .iter()
        .filter(|u| u.check == AuditCheck::Overlap)
        .map(|u| rules(&u.rules))
        .collect();
    let mut never = vec![false; n];
    let mut shadowed = Vec::new();
    let mut overlaps = Vec::new();
    let mut gap = false;
    for d in &report.diagnostics {
        let involved = rules(&d.rules);
        match d.code {
            DiagnosticCode::Unsatisfiable => never[involved[0]] = true,
            DiagnosticCode::Shadowed => shadowed.push(involved[0]),
            DiagnosticCode::Overlap => {
                let w = d.witness.as_ref().unwrap();
                assert!(involved.iter().all(|&r| fires(ruleset, r, w)), "{d:?}");
                overlaps.push((involved[0], involved[1]));
            }
            DiagnosticCode::Gap => {
                let w = d.witness.as_ref().unwrap();
                assert!((0..n).all(|r| !fires(ruleset, r, w)), "{d:?}");
                gap = true;
            }
            _ => {}
        }
    }
    let overlap_checked = report.status(AuditCheck::Overlap) != CheckStatus::NotApplicable;
    let gap_checked = report.status(AuditCheck::Gap) == CheckStatus::Complete;
    for (input, supported) in grid() {
        let fired: Vec<bool> = (0..n).map(|r| fires(ruleset, r, &input)).collect();
        for r in 0..n {
            assert!(
                !(never[r] && fired[r]),
                "{} fires on {input:?}",
                ruleset.rules()[r].name()
            );
        }
        for &j in &shadowed {
            if fired[j] {
                assert!(ahead(ruleset, j).iter().any(|&i| fired[i]), "{input:?}");
            }
        }
        if overlap_checked {
            for a in 0..n {
                for b in a + 1..n {
                    if fired[a] && fired[b] {
                        assert!(
                            overlaps.contains(&(a, b)) || undecided_pairs.contains(&vec![a, b]),
                            "{a} and {b} on {input:?}"
                        );
                    }
                }
            }
        }
        if supported && gap_checked && !gap {
            assert!(fired.iter().any(|f| *f), "a gap at {input:?}\n{report}");
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(96))]

    #[test]
    fn audits_hold_on_every_input(text in ruleset(false)) {
        let ruleset = parse(&text);
        let report = audit(&ruleset, &AuditLimits::default()).unwrap();
        check(&ruleset, &report, true);
    }

    #[test]
    fn strict_witnesses_hold_in_strict_mode(text in ruleset(true)) {
        let ruleset = parse(&text.replacen('\n', "\nSTRICT\n", 1));
        assert!(ruleset.strict());
        let report = audit(&ruleset, &AuditLimits::default()).unwrap();
        check(&ruleset, &report, false);
        for d in &report.diagnostics {
            let Some(witness) = &d.witness else { continue };
            let decision = ruleset.evaluate(witness);
            match (d.code, d.severity, decision) {
                (DiagnosticCode::Overlap, cleanrules::audit::Severity::Error, Err(e)) => {
                    assert_eq!(e.code().as_str(), "hit_policy_error");
                    assert!(d.rules.iter().all(|r| e.context().rules.contains(r)));
                }
                (DiagnosticCode::Overlap, _, Ok(decision)) => {
                    let fired: Vec<&str> = decision
                        .results()
                        .iter()
                        .filter(|r| r.fired)
                        .map(|r| r.rule.name())
                        .collect();
                    assert!(d.rules.iter().all(|r| fired.contains(&r.as_str())));
                }
                (DiagnosticCode::Gap, _, Ok(decision)) => {
                    assert!(decision.results().iter().all(|r| !r.fired));
                }
                other => panic!("{d:?} is not shown: {:?}", other.2.err()),
            }
        }
    }

    #[test]
    fn partial_audits_claim_nothing_false(text in ruleset(true)) {
        let ruleset = parse(&text);
        let report = audit(&ruleset, &AuditLimits::default()).unwrap();
        check(&ruleset, &report, false);
    }
}

#[test]
fn shadowed_rules_are_found_on_a_known_case() {
    let ruleset =
        parse("HIT POLICY first\nRULE a IF x >= 1\nRULE b IF x > 2 AND NOT (x IS NULL)\n");
    let report = audit(&ruleset, &AuditLimits::default()).unwrap();
    check(&ruleset, &report, true);
    assert_eq!(report.diagnostics[0].code, DiagnosticCode::Shadowed);
}
