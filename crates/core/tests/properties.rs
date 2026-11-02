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
use cleanrules::format::{
    Document, read_decision_document, read_document, read_node, read_parsed_document,
    write_decision, write_document, write_node,
};
use cleanrules::json::{Json, JsonNumber, Limits, same, to_string};
use cleanrules::logic::{Kleene, Truth};
use cleanrules::rules::{Config, HitPolicy, Rule, RuleSet};
use cleanrules::table::DecisionTable;
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

/// Conditions with the keys, strings and numbers that text must quote or
/// escape: keywords, spaces, backticks, quotes, control characters, -0.0.
fn odd_condition() -> impl Strategy<Value = Node> {
    let key = prop::sample::select(vec!["if", "a b", "x`y", "", "_1", "é", "NOT"]);
    let path = prop::collection::vec(
        prop_oneof![
            key.prop_map(|k| Segment::Key(k.into())),
            (0_u64..3).prop_map(Segment::Index)
        ],
        1..3,
    );
    let field = path.prop_filter_map("valid field", |p| Node::field(p).ok());
    let constant = prop_oneof![
        "[a\"'\\\n\t`é\u{7f}]{0,3}".prop_map(Value::String),
        any::<f64>()
            .prop_filter("finite", |f| f.is_finite())
            .prop_map(Value::Float),
        Just(Value::Float(-0.0)),
        Just(Value::Float(f64::INFINITY)),
        any::<i64>().prop_map(Value::Int),
        (any::<i64>(), 0_u32..6)
            .prop_map(|(c, s)| Value::Decimal(Decimal::new(i128::from(c), s).unwrap())),
    ]
    .prop_map(|v| Node::constant(v).unwrap());
    let operand = prop_oneof![
        field.clone(),
        constant.clone(),
        constant.prop_filter_map("valid neg", |n| Node::neg(n).ok()),
    ];
    (operand.clone(), operand, field).prop_filter_map("valid condition", |(a, b, f)| {
        Node::and(vec![
            Node::compare(CompareOp::Ne, a, b).ok()?,
            Node::is_null(f).ok()?,
        ])
        .ok()
    })
}

/// Rule sets whose names, tags, payload keys and descriptions text must quote.
fn odd_ruleset() -> impl Strategy<Value = RuleSet> {
    let label = || {
        prop::sample::select(vec![
            "if", "a b", "", "x`y", "é", "rule_1", "NOT", "q\"\n", "_",
        ])
    };
    let rule = (
        label(),
        condition(),
        prop::collection::vec((label(), value_node()), 0..3),
        any::<i64>(),
        prop::collection::vec(label(), 0..3),
        proptest::option::of(r#"[a"\n\\é]{0,4}"#),
    )
        .prop_filter_map(
            "valid rule",
            |(name, when, then, priority, tags, description)| {
                let then = then.into_iter().map(|(k, v)| (k.to_owned(), v)).collect();
                let mut rule = Rule::with_payload(name, when, then)
                    .ok()?
                    .priority(priority)
                    .tags(tags.into_iter().map(str::to_owned).collect::<Vec<String>>());
                if let Some(description) = description {
                    rule = rule.description(description);
                }
                Some(rule)
            },
        );
    let policy = prop::sample::select(vec![
        HitPolicy::First,
        HitPolicy::Priority,
        HitPolicy::Collect,
    ]);
    (prop::collection::vec(rule, 0..4), policy, any::<bool>()).prop_filter_map(
        "valid rule set",
        |(rules, hit_policy, strict)| {
            let config = Config {
                hit_policy,
                strict,
                ..Config::default()
            };
            RuleSet::new(rules, config).ok()
        },
    )
}

/// Decision table documents whose names, descriptions and cells CSV must quote.
// One strategy for the whole document reads best as a single function.
#[allow(clippy::too_many_lines)]
fn odd_table() -> impl Strategy<Value = Json> {
    let string = |text: &str| Json::String(text.to_owned());
    let cell = prop::sample::select(vec![
        "-",
        "",
        "> 5",
        "'a,b'",
        "IN ('x', 'y''z')",
        "NOT IN (1, 2)",
        "BETWEEN 1 AND 2 CLOSED NONE",
        "IS NOT NULL",
        "\"q\\\"\"",
        "STARTS WITH 'é'",
        "-3",
        "TRUE",
        "y * 2",
        "\"tab\\there\"",
    ]);
    let output = proptest::option::of(prop::sample::select(vec![
        "1",
        "'x'",
        "y + 1",
        "'a\"b'",
        "\"line\\nbreak\"",
        "1.50",
        "0.25e0",
    ]));
    let label = proptest::option::of(prop::sample::select(vec![
        "a", "b c", " d", "e,f", "g\"h", "i\nj", "k;l", "é", "row_1", " ", "", "m\rn",
    ]));
    // The second output has output values: its cells hold one of them.
    let ranked = prop::sample::select(vec!["1", "2", "'x'"]);
    let row = (
        label.clone(),
        proptest::option::of(any::<i64>()),
        label,
        [cell.clone(), cell],
        (output, ranked),
    )
        .prop_map(
            move |(name, priority, description, inputs, (output, ranked))| {
                let outputs = [output, Some(ranked)];
                let mut members = Vec::new();
                // An empty name is an error; an empty description is none.
                if let Some(name) = name.filter(|n| !n.is_empty()) {
                    members.push(("name".to_owned(), string(name)));
                }
                if let Some(priority) = priority {
                    members.push((
                        "priority".to_owned(),
                        Json::Object(vec![("$int".into(), string(&priority.to_string()))]),
                    ));
                }
                if let Some(description) = description {
                    members.push(("description".to_owned(), string(description)));
                }
                members.push((
                    "inputs".into(),
                    Json::Array(inputs.iter().map(|c| string(c)).collect()),
                ));
                members.push((
                    "outputs".into(),
                    Json::Array(
                        outputs
                            .iter()
                            .map(|c| c.map_or(Json::Null, string))
                            .collect(),
                    ),
                ));
                Json::Object(members)
            },
        );
    let policy = prop::sample::select(vec![
        "first",
        "rule_order",
        "collect",
        "priority",
        "output_order",
    ]);
    (prop::collection::vec(row, 0..5), policy, any::<bool>()).prop_map(
        move |(mut rows, policy, strict)| {
            let mut table = vec![("hit_policy".to_owned(), string(policy))];
            if policy == "collect" {
                table.push(("aggregate".into(), string("count")));
            }
            if strict {
                table.push(("strict".into(), Json::Bool(true)));
            }
            if matches!(policy, "priority" | "output_order") {
                // Output values rank the rows: a row priority would be an error.
                for row in &mut rows {
                    if let Json::Object(members) = row {
                        members.retain(|(k, _)| k != "priority");
                    }
                }
            }
            table.push((
                "inputs".into(),
                Json::Array(vec![string("x"), string("`a b`")]),
            ));
            table.push((
                "outputs".into(),
                Json::Array(vec![
                    Json::Object(vec![("name".into(), string("y"))]),
                    Json::Object(vec![
                        ("name".into(), string("two words")),
                        (
                            "values".into(),
                            Json::Array(vec![
                                Json::Number(JsonNumber::from_literal("2")),
                                string("x"),
                                Json::Number(JsonNumber::from_literal("1")),
                            ]),
                        ),
                    ]),
                ]),
            ));
            table.push(("rows".into(), Json::Array(rows)));
            Json::Object(vec![
                (
                    "version".into(),
                    Json::Number(JsonNumber::from_literal("1")),
                ),
                ("decision_table".into(), Json::Object(table)),
            ])
        },
    )
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
    fn conditions_round_trip_through_text(node in prop_oneof![condition(), odd_condition()]) {
        let printed = node.to_string();
        let Document::Condition(read) = cleanrules::text::parse(&printed, &Limits::default())
            .map_err(|e| TestCaseError::fail(format!("{printed:?}: {e}")))?
        else {
            return Err(TestCaseError::fail(format!("{printed:?} is not a condition")));
        };
        prop_assert!(read == node, "{printed:?} read back as {read}");
        prop_assert_eq!(read.to_string(), printed);
    }

    #[test]
    fn rule_sets_round_trip_through_text(ruleset in odd_ruleset()) {
        let document = Document::RuleSet(ruleset);
        let printed = cleanrules::text::to_string(&document).unwrap();
        let read = cleanrules::text::parse(&printed, &Limits::default())
            .map_err(|e| TestCaseError::fail(format!("{printed:?}: {e}")))?;
        prop_assert!(same(&write_document(&read), &write_document(&document)), "{printed:?}");
        prop_assert_eq!(cleanrules::text::to_string(&read).unwrap(), printed);
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

    #[test]
    fn tables_round_trip_through_csv_and_json(json in odd_table()) {
        let limits = Limits::default();
        // Unnamed rows may clash with a row named row_1: such tables are invalid.
        let read = read_parsed_document(&json, &limits);
        prop_assume!(read.is_ok(), "{:?}", read.err());
        let Ok(Document::DecisionTable(table)) = read else {
            panic!("not a table")
        };
        let written = write_document(&Document::DecisionTable(table.clone()));
        let csv = table.to_csv();
        let from_csv = DecisionTable::from_csv(&csv, &limits).unwrap();
        prop_assert!(same(&write_document(&Document::DecisionTable(from_csv.clone())), &written), "{csv}");
        prop_assert_eq!(from_csv.to_csv(), csv);
        let Document::DecisionTable(from_json) = read_document(&to_string(&written), &limits).unwrap() else {
            panic!("not a table")
        };
        prop_assert!(same(&write_document(&Document::DecisionTable(from_json)), &written));
    }

    #[test]
    fn the_text_lists_every_rule_and_cause(conditions in prop::collection::vec(condition(), 1..4), data in input()) {
        let rules = ruleset(conditions, HitPolicy::RuleOrder);
        let decision = rules.evaluate(&data).unwrap();
        let explanation = explain(&decision);
        let text = explanation.to_string();
        let mut listed = 0;
        for entry in &explanation.rules {
            let mut found = Vec::new();
            causes(&entry.requirement, &mut found);
            listed += found.len();
            for cause in found {
                let line = format!("- {cause}");
                prop_assert!(text.lines().any(|l| l.trim_start() == line), "{line} in\n{text}");
            }
        }
        let cause_lines = text
            .lines()
            .map(str::trim_start)
            .filter(|l| l.starts_with("- ") && *l != "- all of:" && *l != "- any of:")
            .count();
        prop_assert_eq!(cause_lines, listed);
        prop_assert_eq!(text.lines().filter(|l| l.starts_with('[')).count(), explanation.rules.len());
    }
}
