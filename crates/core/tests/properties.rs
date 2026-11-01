//! Property tests of the core (`CLAUDE.md`, *Convenciones*): logic laws on real
//! evaluations, reasons, strict mode, JSON round trips, decisions read back, and
//! explanations whose causes point the right way.
//!
//! A counterexample found here becomes a conformance vector.

// A test: a panic is the failure report.
#![allow(clippy::unwrap_used, clippy::panic)]

use cleanrules::ast::{ArithOp, Closed, CompareOp, Node, StringOp};
use cleanrules::eval::{Evaluated, Options, Outcome, evaluate_tree};
use cleanrules::explain::{Requirement, Status, explain};
use cleanrules::format::{read_decision_document, read_node, write_decision, write_node};
use cleanrules::json::{Json, JsonNumber, Limits, same, to_string};
use cleanrules::logic::{Kleene, Truth};
use cleanrules::rules::{Config, HitPolicy, Rule, RuleSet};
use cleanrules::value::{Decimal, Map, NullCause, Value};
use cleanrules::{ErrorCode, Segment};
use proptest::prelude::*;

// --- Strategies ----------------------------------------------------------------------

fn number() -> impl Strategy<Value = Value> {
    prop_oneof![
        3 => (-5_i64..5).prop_map(Value::Int),
        1 => (-10.0_f64..10.0).prop_map(Value::Float),
        1 => (-1000_i128..1000).prop_map(|c| Value::Decimal(Decimal::new(c, 2).unwrap())),
    ]
}

fn scalar() -> impl Strategy<Value = Value> {
    prop_oneof![
        6 => number(),
        1 => any::<bool>().prop_map(Value::Bool),
        1 => "[abcé]{0,2}".prop_map(Value::String),
        1 => (1_i8..10).prop_map(|d| Value::Date(jiff::civil::date(2026, 1, d))),
    ]
}

fn maybe() -> impl Strategy<Value = Value> {
    prop_oneof![1 => Just(Value::Null(NullCause::Null)), 6 => scalar()]
}

fn input() -> impl Strategy<Value = Value> {
    (maybe(), maybe(), proptest::option::of(maybe())).prop_map(|(a, b, c)| {
        let mut map = Map::new();
        map.insert("a", a);
        map.insert("b", b);
        if let Some(c) = c {
            map.insert("c", c);
        }
        Value::Map(map)
    })
}

fn field() -> impl Strategy<Value = Node> {
    prop_oneof![
        5 => prop::sample::select(vec!["a", "b", "c"]).prop_map(|k| Node::field(vec![Segment::Key(k.into())]).unwrap()),
        1 => Just(Node::field(vec![Segment::Key("missing".into())]).unwrap()),
    ]
}

fn numeric() -> impl Strategy<Value = Node> {
    let leaf = prop_oneof![field(), number().prop_map(|v| Node::constant(v).unwrap())];
    leaf.prop_recursive(2, 6, 2, |inner| {
        prop_oneof![
            (
                prop::sample::select(vec![ArithOp::Add, ArithOp::Sub, ArithOp::Mul, ArithOp::Div]),
                inner.clone(),
                inner.clone()
            )
                .prop_filter_map("valid arithmetic", |(op, l, r)| Node::arith(op, l, r).ok()),
            inner
                .clone()
                .prop_filter_map("valid abs", |n| Node::abs(n).ok()),
            inner.prop_filter_map("valid neg", |n| Node::neg(n).ok()),
        ]
    })
}

fn value_node() -> impl Strategy<Value = Node> {
    prop_oneof![3 => field(), 2 => scalar().prop_map(|v| Node::constant(v).unwrap()), 1 => numeric()]
}

fn predicate() -> impl Strategy<Value = Node> {
    let ops = vec![
        CompareOp::Eq,
        CompareOp::Ne,
        CompareOp::Lt,
        CompareOp::Le,
        CompareOp::Gt,
        CompareOp::Ge,
    ];
    prop_oneof![
        4 => (prop::sample::select(ops), value_node(), value_node())
            .prop_filter_map("valid compare", |(op, l, r)| Node::compare(op, l, r).ok()),
        1 => (value_node(), prop::collection::vec(number(), 1..4))
            .prop_filter_map("valid in", |(n, vs)| Node::is_in(n, vs).ok()),
        1 => (value_node(), numeric(), numeric(), prop::sample::select(vec![Closed::Both, Closed::Left, Closed::Right, Closed::Neither]))
            .prop_filter_map("valid between", |(n, l, h, c)| Node::between(n, l, h, c).ok()),
        1 => value_node().prop_filter_map("valid is_null", |n| Node::is_null(n).ok()),
        1 => (value_node(), prop::sample::select(vec!["^a", "é$", "(?i)B"]))
            .prop_filter_map("valid matches", |(n, p)| Node::matches(n, p).ok()),
        1 => (prop::sample::select(vec![StringOp::StartsWith, StringOp::EndsWith, StringOp::Contains]), value_node(), "[abé]{0,1}")
            .prop_filter_map("valid string test", |(op, n, t)| Node::string_test(op, n, t).ok()),
    ]
}

fn condition() -> impl Strategy<Value = Node> {
    let leaf = prop_oneof![
        6 => predicate(),
        1 => field(),
        1 => any::<bool>().prop_map(|b| Node::constant(Value::Bool(b)).unwrap()),
    ];
    leaf.prop_recursive(3, 12, 3, |inner| {
        prop_oneof![
            prop::collection::vec(inner.clone(), 2..4)
                .prop_filter_map("valid and", |ops| Node::and(ops).ok()),
            prop::collection::vec(inner.clone(), 2..4)
                .prop_filter_map("valid or", |ops| Node::or(ops).ok()),
            inner.prop_filter_map("valid not", |n| Node::not(n).ok()),
        ]
    })
}

// --- Helpers -------------------------------------------------------------------------------

fn truth(node: &Node, input: &Value) -> Truth {
    evaluate_tree(node, input, &Kleene, Options::default())
        .unwrap()
        .truth()
        .unwrap()
}

fn reasons_follow_outcomes(result: &Evaluated<'_, Truth>) -> bool {
    let needs_reason = match &result.outcome {
        Outcome::Truth(t) => *t == Truth::Unknown,
        Outcome::Value(v) => v.is_null(),
    };
    needs_reason == result.reason.is_some() && result.children.iter().all(reasons_follow_outcomes)
}

fn causes<'d, 'a>(
    requirement: &'d Requirement<'d, 'a>,
    out: &mut Vec<&'d cleanrules::explain::Cause<'d, 'a>>,
) {
    match requirement {
        Requirement::Cause(cause) => out.push(cause),
        Requirement::Group(_, items) => items.iter().for_each(|i| causes(i, out)),
    }
}

fn ruleset(conditions: Vec<Node>, policy: HitPolicy) -> RuleSet {
    let rules = conditions
        .into_iter()
        .enumerate()
        .map(|(i, when)| {
            let payload = vec![(
                "v".to_owned(),
                Node::constant(Value::Int(i64::try_from(i).unwrap())).unwrap(),
            )];
            Rule::with_payload(format!("r{i}"), when, payload).unwrap()
        })
        .collect();
    RuleSet::new(
        rules,
        Config {
            hit_policy: policy,
            ..Config::default()
        },
    )
    .unwrap()
}

// --- Properties ------------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    #[test]
    fn evaluation_never_fails_and_reasons_follow_outcomes(node in condition(), data in input()) {
        let result = evaluate_tree(&node, &data, &Kleene, Options::default()).unwrap();
        prop_assert!(reasons_follow_outcomes(&result));
    }

    #[test]
    fn de_morgan_and_double_negation_hold_on_evaluation(a in condition(), b in condition(), data in input()) {
        let not = |n: Node| Node::not(n).unwrap();
        let left = not(Node::and(vec![a.clone(), b.clone()]).unwrap());
        let right = Node::or(vec![not(a.clone()), not(b.clone())]).unwrap();
        prop_assert_eq!(truth(&left, &data), truth(&right, &data));
        let left = not(Node::or(vec![a.clone(), b.clone()]).unwrap());
        let right = Node::and(vec![not(a.clone()), not(b)]).unwrap();
        prop_assert_eq!(truth(&left, &data), truth(&right, &data));
        prop_assert_eq!(truth(&not(not(a.clone())), &data), truth(&a, &data));
    }

    #[test]
    fn strict_mode_agrees_or_raises_an_evaluation_error(node in condition(), data in input()) {
        let strict = Options { strict: true, rule: None };
        match evaluate_tree(&node, &data, &Kleene, strict) {
            Ok(result) => prop_assert_eq!(result.truth(), Some(truth(&node, &data))),
            Err(error) => prop_assert!(error.code().is_a(ErrorCode::EvaluationError), "{}", error.code()),
        }
    }

    #[test]
    fn conditions_round_trip_through_json(node in condition()) {
        let written = write_node(&node);
        let read = read_node(&written, "", &Limits::default()).unwrap();
        prop_assert!(read == node);
        prop_assert!(same(&write_node(&read), &written));
    }

    #[test]
    fn every_decision_reads_back(conditions in prop::collection::vec(condition(), 1..4), data in input(),
                                 policy in prop::sample::select(vec![HitPolicy::First, HitPolicy::Priority, HitPolicy::RuleOrder, HitPolicy::Collect])) {
        let rules = ruleset(conditions, policy);
        let decision = rules.evaluate(&data).unwrap();
        let written = write_decision(&decision);
        let document = Json::Object(vec![
            ("version".into(), Json::Number(JsonNumber::from_literal("1"))),
            ("decision".into(), written.clone()),
        ]);
        let read = read_decision_document(&to_string(&document), &rules, &Limits::default()).unwrap();
        prop_assert!(same(&write_decision(&read), &written));
    }

    #[test]
    fn causes_point_the_right_way(conditions in prop::collection::vec(condition(), 1..4), data in input()) {
        let rules = ruleset(conditions, HitPolicy::RuleOrder);
        let decision = rules.evaluate(&data).unwrap();
        for entry in explain(&decision).rules {
            let mut found = Vec::new();
            causes(&entry.requirement, &mut found);
            prop_assert!(!found.is_empty(), "every outcome has a cause");
            match entry.status {
                Status::Selected => prop_assert!(found.iter().all(|c| c.holds())),
                Status::NotFired if entry.achievable == Some(true) => prop_assert!(!found.iter().any(|c| c.holds())),
                Status::Unknown => prop_assert!(found.iter().all(|c| c.truth() == Truth::Unknown)),
                _ => {}
            }
        }
    }
}
