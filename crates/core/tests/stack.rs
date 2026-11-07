//! Every operation at the default limits runs on a small stack
//! (`spec/json-format.md`, *Errors and limits*): 512 KiB in debug builds and
//! 128 KiB in release builds, less than the threads of the bindings' hosts
//! have (a JVM thread has 1 MiB). Conditions and payload values are as deep as
//! `max_depth` allows, texts nest as deep as their reader allows, and data as
//! deep as `max_nesting` allows.

#![allow(clippy::unwrap_used, clippy::panic)]

mod common;

use std::fmt::Write as _;
use std::hash::{DefaultHasher, Hash, Hasher};

use cleanrules::eval::{Options, evaluate_tree, evaluate_value_tree};
use cleanrules::explain::{explain, summary};
use cleanrules::format::{
    Document, DocumentFormat, ValueContext, explanation_value, read_decision_document_as,
    read_document, read_document_as, read_value, result_value, write_decision_as,
    write_document_as, write_explanation, write_result, write_summary, write_value,
};
use cleanrules::json::{Limits, parse, to_string};
use cleanrules::logic::Kleene;
use cleanrules::{ErrorCode, Value};
use common::on_small_stack;

const DEPTH: usize = 256;
const NESTING: usize = 1024;

fn field(name: &str) -> String {
    format!(r#"{{"op":"field","path":["{name}"]}}"#)
}

fn constant() -> String {
    r#"{"op":"const","value":1}"#.to_owned()
}

/// `depth` nodes down to `x`, each `wrap`ping the one below it.
fn chain(depth: usize, wrap: impl Fn(usize, String) -> String) -> String {
    (1..depth).fold(field("x"), |inner, level| wrap(level, inner))
}

/// A value `depth` nodes deep: sums, nested to the left or to the right, or
/// `abs` and `neg` in turns.
fn value(depth: usize, shape: &str) -> String {
    chain(depth, |level, inner| match shape {
        "left" => format!(r#"{{"op":"add","left":{inner},"right":{}}}"#, constant()),
        "right" => format!(r#"{{"op":"sub","left":{},"right":{inner}}}"#, constant()),
        _ if level % 2 == 0 => format!(r#"{{"op":"abs","operand":{inner}}}"#),
        _ => format!(r#"{{"op":"neg","operand":{inner}}}"#),
    })
}

/// Conditions exactly `DEPTH` nodes deep, one of each kind of nesting.
fn conditions() -> Vec<(&'static str, String)> {
    let compare = |value: String| format!(r#"{{"op":"gt","left":{value},"right":{}}}"#, constant());
    vec![
        (
            "not",
            chain(DEPTH, |_, inner| {
                format!(r#"{{"op":"not","operand":{inner}}}"#)
            }),
        ),
        (
            // A field per level, so that explanations keep every level.
            "and/or",
            chain(DEPTH, |level, inner| {
                let op = if level % 2 == 0 { "and" } else { "or" };
                let other = field(&format!("b{level}"));
                format!(r#"{{"op":"{op}","operands":[{inner},{other}]}}"#)
            }),
        ),
        ("sum left", compare(value(DEPTH - 1, "left"))),
        ("sum right", compare(value(DEPTH - 1, "right"))),
        ("abs/neg", compare(value(DEPTH - 1, "unary"))),
        (
            "between",
            format!(
                r#"{{"op":"between","operand":{},"low":{},"high":{}}}"#,
                value(DEPTH - 1, "left"),
                value(DEPTH - 1, "right"),
                constant()
            ),
        ),
        (
            "is_null",
            format!(
                r#"{{"op":"is_null","operand":{}}}"#,
                value(DEPTH - 1, "unary")
            ),
        ),
    ]
}

/// Every field false, `x` 1.
fn input() -> Value {
    let mut text = String::from(r#"{"a":false,"x":1"#);
    for level in 1..DEPTH {
        let _ = write!(text, r#","b{level}":false"#);
    }
    text.push('}');
    read_value(
        &parse(&text, &Limits::default()).unwrap(),
        "",
        ValueContext::Data,
    )
    .unwrap()
}

fn formats() -> Vec<DocumentFormat> {
    let mut formats = vec![DocumentFormat::Json, DocumentFormat::Text];
    if cfg!(feature = "yaml") {
        formats.push(DocumentFormat::Yaml);
    }
    formats
}

#[test]
fn conditions_at_the_depth_limit() {
    on_small_stack(|| {
        let limits = Limits::default();
        let input = input();
        for (shape, condition) in conditions() {
            let text = format!(r#"{{"version":1,"condition":{condition}}}"#);
            let document = read_document(&text, &limits).unwrap();
            for format in formats() {
                let written = write_document_as(&document, format).unwrap();
                read_document_as(written.as_bytes(), format, &limits)
                    .unwrap_or_else(|e| panic!("{shape} as {format:?}: {e}"));
            }
            let Document::Condition(node) = document else {
                panic!("{shape}: not a condition")
            };
            let copy = node.clone();
            assert!(copy == node, "{shape}");
            let (mut a, mut b) = (DefaultHasher::new(), DefaultHasher::new());
            node.hash(&mut a);
            copy.hash(&mut b);
            assert_eq!(a.finish(), b.finish(), "{shape}");
            let text = node.to_string();
            assert!(
                cleanrules::text::parse_node(&text, &limits).unwrap() == node,
                "{shape}"
            );
            let tree = evaluate_tree(&node, &input, &Kleene, Options::default()).unwrap();
            let written = write_result(&tree);
            assert!(cleanrules::json::same(
                &parse(&to_string(&written), &limits).unwrap(),
                &written
            ));
            drop(write_value(&result_value(&tree)));
            drop(tree.clone());
        }
        // A value as deep, alone.
        let json = parse(&value(DEPTH, "left"), &limits).unwrap();
        let node = cleanrules::format::read_node(&json, "", &limits).unwrap();
        evaluate_value_tree(&node, &input, &Kleene, Options::default()).unwrap();
    });
}

#[test]
fn one_node_deeper_is_an_error() {
    on_small_stack(|| {
        let condition = chain(DEPTH + 1, |_, inner| {
            format!(r#"{{"op":"not","operand":{inner}}}"#)
        });
        let text = format!(r#"{{"version":1,"condition":{condition}}}"#);
        let error = read_document(&text, &Limits::default()).unwrap_err();
        assert_eq!(error.code(), ErrorCode::LimitExceeded);
    });
}

#[test]
fn rule_sets_at_the_depth_limit() {
    on_small_stack(|| {
        let limits = Limits::default();
        let input = input();
        for (shape, condition) in conditions() {
            let text = format!(
                r#"{{"version":1,"ruleset":{{"rules":[{{"name":"r","when":{condition},"then":{{"y":{}}}}}]}}}}"#,
                value(DEPTH, "left")
            );
            let document = read_document(&text, &limits).unwrap();
            for format in formats() {
                let written = write_document_as(&document, format).unwrap();
                read_document_as(written.as_bytes(), format, &limits)
                    .unwrap_or_else(|e| panic!("{shape} as {format:?}: {e}"));
            }
            let ruleset = document.into_ruleset().unwrap();
            let decision = ruleset.evaluate(&input).unwrap();
            // A decision can be larger than its rule set: in YAML, each level
            // is indented further, and its reader raises the limits.
            let large = Limits {
                max_yaml_size: 16 << 20,
                ..limits
            };
            for format in formats() {
                let Ok(written) = write_decision_as(&decision, format) else {
                    continue; // a decision has no text form
                };
                read_decision_document_as(written.as_bytes(), format, &ruleset, &large)
                    .unwrap_or_else(|e| panic!("{shape}'s decision as {format:?}: {e}"));
            }
            let explanation = explain(&decision);
            assert_ne!(to_string(&write_explanation(&explanation)), "");
            assert_ne!(explanation.to_string(), "");
            drop(write_value(&explanation_value(&explanation)));
            let summary = summary(&ruleset);
            assert_ne!(to_string(&write_summary(&summary)), "");
            assert_ne!(summary.to_string(), "");
        }
    });
}

#[test]
fn the_deepest_texts_read() {
    on_small_stack(|| {
        let limits = Limits::default();
        // Parentheses, NOT, minus and ABS nest 514 deep at most, two levels per
        // node at worst.
        for text in [
            format!("{}x > 0{}", "(".repeat(512), ")".repeat(512)),
            format!("{}x{}", "NOT (".repeat(255), ")".repeat(255)),
            format!("{}x{} > 0", "-(".repeat(254), ")".repeat(254)),
            format!("{}x{} > 0", "ABS(".repeat(254), ")".repeat(254)),
            format!("IF {}x{} > 0 THEN y = 1", "(".repeat(511), ")".repeat(511)),
            format!("x{} > 0", " + x".repeat(254)),
        ] {
            let document = cleanrules::text::parse(&text, &limits)
                .unwrap_or_else(|e| panic!("{}…: {e}", &text[..20]));
            cleanrules::text::to_string(&document).unwrap();
        }
        for text in [
            format!("{}x > 0{}", "(".repeat(100_000), ")".repeat(100_000)),
            format!("{}x", "NOT ".repeat(100_000)),
        ] {
            let error = cleanrules::text::parse(&text, &limits).unwrap_err();
            assert_eq!(error.code(), ErrorCode::LimitExceeded);
        }
    });
}

/// `[[…[1]…]]`, `depth` lists deep.
fn lists(depth: usize) -> String {
    format!("{}1{}", "[".repeat(depth), "]".repeat(depth))
}

#[test]
fn data_at_the_nesting_limit() {
    on_small_stack(|| {
        let limits = Limits::default();
        // The input itself is the outermost object.
        let text = format!(
            r#"{{"l":{},"m":{}}}"#,
            lists(NESTING - 1),
            lists(NESTING - 1)
        );
        let json = parse(&text, &limits).unwrap();
        let input = read_value(&json, "", ValueContext::Data).unwrap();
        assert!(cleanrules::json::same(&write_value(&input), &json));
        assert!(input.clone().any_equal(&input));
        // Read by a field, recorded in the decision, and its output.
        let rules = r#"{"version":1,"ruleset":{"rules":[{"name":"r","when":{"op":"eq","left":{"op":"field","path":["l"]},"right":{"op":"field","path":["m"]}},"then":{"y":{"op":"field","path":["l"]}}},{"name":"s","when":{"op":"is_null","operand":{"op":"field","path":["l"]}}}]}}"#;
        let ruleset = read_document(rules, &limits)
            .unwrap()
            .into_ruleset()
            .unwrap();
        let decision = ruleset.evaluate(&input).unwrap();
        let explanation = explain(&decision);
        assert_ne!(to_string(&write_explanation(&explanation)), "");
        assert_ne!(explanation.to_string(), "");
        // A decision document holds the recorded values a few levels down
        // (`decision`, `results`, `when`, `children`…): data in a decision may
        // nest deeper than the limit, so its reader raises it.
        let raised = Limits {
            max_nesting: NESTING + 16,
            max_yaml_size: 16 << 20,
            ..limits
        };
        for format in formats() {
            let Ok(written) = write_decision_as(&decision, format) else {
                continue;
            };
            read_decision_document_as(written.as_bytes(), format, &ruleset, &raised)
                .unwrap_or_else(|e| panic!("the decision as {format:?}: {e}"));
        }
        // Metadata as deep as a rule document allows.
        let text = format!(
            r#"{{"version":1,"rule":{{"name":"r","when":{},"metadata":{{"m":{}}}}}}}"#,
            field("a"),
            lists(NESTING - 3)
        );
        let document = read_document(&text, &limits).unwrap();
        for format in formats() {
            let Ok(written) = write_document_as(&document, format) else {
                continue; // text cannot hold metadata
            };
            read_document_as(written.as_bytes(), format, &limits)
                .unwrap_or_else(|e| panic!("the rule as {format:?}: {e}"));
        }
    });
}
