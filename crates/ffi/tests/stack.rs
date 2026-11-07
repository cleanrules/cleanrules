//! Every call at the default limits runs on a small stack through the C ABI
//! (`spec/json-format.md`, *Errors and limits*): 512 KiB in debug builds and
//! 128 KiB in release builds, less than a JVM thread (1 MiB) or a Node worker
//! has. A host calls the library from threads like these.

// A test: a panic is the failure report.
#![allow(clippy::unwrap_used, clippy::panic)]

mod common;

use std::fmt::Write as _;

use cleanrules_ffi::*;
use common::{
    Value, evaluate, evaluate_node, evaluate_value, explain, handle, limits, on_small_stack,
    read_decision, read_document, read_node, read_ruleset, summary, value_from_json, value_to_json,
    write_decision, write_document,
};

const DEPTH: usize = 256;
const NESTING: usize = 1024;

fn field(name: &str) -> String {
    format!(r#"{{"op":"field","path":["{name}"]}}"#)
}

/// `depth` nodes down to `x`, each `wrap`ping the one below it.
fn chain(depth: usize, wrap: impl Fn(usize, String) -> String) -> String {
    (1..depth).fold(field("x"), |inner, level| wrap(level, inner))
}

/// A sum `depth` nodes deep.
fn sum(depth: usize) -> String {
    chain(depth, |_, inner| {
        format!(r#"{{"op":"add","left":{inner},"right":{{"op":"const","value":1}}}}"#)
    })
}

/// Conditions exactly `DEPTH` nodes deep.
fn conditions() -> [String; 3] {
    [
        chain(DEPTH, |_, inner| {
            format!(r#"{{"op":"not","operand":{inner}}}"#)
        }),
        // A field per level, so that explanations keep every level.
        chain(DEPTH, |level, inner| {
            let op = if level % 2 == 0 { "and" } else { "or" };
            let other = field(&format!("b{level}"));
            format!(r#"{{"op":"{op}","operands":[{inner},{other}]}}"#)
        }),
        format!(
            r#"{{"op":"between","operand":{},"low":{{"op":"const","value":0}},"high":{}}}"#,
            sum(DEPTH - 1),
            sum(DEPTH - 1)
        ),
    ]
}

/// `[[…[1]…]]`, `depth` lists deep.
fn lists(depth: usize) -> String {
    format!("{}1{}", "[".repeat(depth), "]".repeat(depth))
}

/// Every field false, `x` 1, and `l` a list as deep as the input may nest.
fn input() -> String {
    let mut text = format!(r#"{{"a":false,"x":1,"l":{}"#, lists(NESTING - 1));
    for level in 1..DEPTH {
        let _ = write!(text, r#","b{level}":false"#);
    }
    text.push('}');
    text
}

#[test]
fn documents_at_the_depth_limit() {
    on_small_stack(|| {
        for condition in conditions() {
            let text = format!(r#"{{"version":1,"condition":{condition}}}"#);
            let document = read_document(text.as_bytes(), CR_FORMAT_JSON, None).unwrap();
            for format in [CR_FORMAT_JSON, CR_FORMAT_YAML, CR_FORMAT_TEXT] {
                let written = write_document(&document, format).unwrap();
                read_document(written.as_bytes(), format, None).unwrap();
            }
            let node = read_node(&condition, CR_FORMAT_JSON).unwrap();
            let input = value_from_json(&input()).unwrap();
            evaluate_node(&node, &input, None, false, false).unwrap();
        }
        let node = read_node(&sum(DEPTH), CR_FORMAT_JSON).unwrap();
        let input = value_from_json(&input()).unwrap();
        evaluate_node(&node, &input, None, false, true).unwrap();
        // The deepest text.
        let text = format!("{}x > 0{}", "(".repeat(512), ")".repeat(512));
        read_document(text.as_bytes(), CR_FORMAT_TEXT, None).unwrap();
    });
}

#[test]
fn decisions_at_the_depth_limit() {
    on_small_stack(|| {
        // Decisions are larger and nest deeper than their rule sets: their
        // readers raise the limits.
        let large = limits(&[
            (CR_LIMIT_MAX_NESTING, NESTING + 16),
            (CR_LIMIT_MAX_YAML_SIZE, 16 << 20),
        ]);
        for condition in conditions() {
            let text = format!(
                r#"{{"version":1,"ruleset":{{"rules":[{{"name":"r","when":{condition},"then":{{"y":{},"l":{}}}}}]}}}}"#,
                sum(DEPTH),
                field("l")
            );
            let ruleset = read_ruleset(text.as_bytes(), CR_FORMAT_JSON).unwrap();
            let decision = evaluate(&ruleset, input().as_bytes()).unwrap();
            for format in [CR_FORMAT_JSON, CR_FORMAT_YAML] {
                let written = write_decision(&decision, format).unwrap();
                read_decision(&ruleset, written.as_bytes(), format, Some(&large)).unwrap();
            }
            explain(&decision, CR_FORMAT_JSON).unwrap();
            explain(&decision, CR_FORMAT_TEXT).unwrap();
            // SAFETY: a live decision; the value and tree are freed on drop or
            // below.
            let explanation =
                handle(|out, error| unsafe { cr_decision_explain_value(decision.0, out, error) })
                    .map(Value)
                    .unwrap();
            value_to_json(explanation.0);
            // SAFETY: as above.
            let tree =
                handle(|out, error| unsafe { cr_decision_rule_tree(decision.0, 0, out, error) })
                    .unwrap();
            // SAFETY: a tree from the library, freed once.
            unsafe { cr_tree_free(tree) };
            summary(&ruleset, CR_FORMAT_JSON).unwrap();
            summary(&ruleset, CR_FORMAT_TEXT).unwrap();
        }
    });
}

#[test]
fn values_at_the_nesting_limit() {
    on_small_stack(|| {
        let text = lists(NESTING);
        let value = value_from_json(&text).unwrap();
        assert_eq!(value_to_json(value.0), text);
        // SAFETY: a live value; the copy is freed on drop.
        let copy = handle(|out, error| unsafe { cr_value_clone(value.0, out, error) })
            .map(Value)
            .unwrap();
        drop(value);
        let rules = r#"{"version":1,"ruleset":{"rules":[{"name":"r","when":{"op":"is_null","operand":{"op":"field","path":["l"]}},"then":{"y":{"op":"field","path":["l"]}}}]}}"#;
        let ruleset = read_ruleset(rules.as_bytes(), CR_FORMAT_JSON).unwrap();
        let input = value_from_json(&input()).unwrap();
        let decision = evaluate_value(&ruleset, &input).unwrap();
        explain(&decision, CR_FORMAT_JSON).unwrap();
        drop(copy);
    });
}
