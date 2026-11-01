//! cleanrules and zen-engine on the core's workload (`crates/core/benches/common`):
//! the same 12 rules, as a cleanrules rule set and as a zen decision table, over
//! the same 1,000 applicants.
//!
//! Before measuring, both engines must select the same rules for every
//! applicant. They do not do the same work: cleanrules evaluates every rule and
//! keeps each result tree so that the decision can be explained, while zen's
//! first hit stops at the first matching row.

#![allow(clippy::unwrap_used, clippy::panic, missing_docs)]

#[path = "../../../crates/core/benches/common/mod.rs"]
mod common;

use std::future::Future;
use std::hint::black_box;
use std::pin::pin;
use std::sync::Arc;
use std::task::{Context, Poll, Waker};

use cleanrules::Value;
use cleanrules::rules::HitPolicy;
use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use serde_json::{Value as JsonValue, json};
use zen_engine::model::DecisionContent;
use zen_engine::{Decision, DecisionEngine, Variable};

/// Run a future that never waits: a decision table has no asynchronous node, so
/// no executor is needed and none is measured.
fn block_on<F: Future>(future: F) -> F::Output {
    let mut future = pin!(future);
    let mut context = Context::from_waker(Waker::noop());
    loop {
        if let Poll::Ready(output) = future.as_mut().poll(&mut context) {
            return output;
        }
    }
}

/// The rules of `common`, in the same order, as decision table rows.
fn rows() -> Vec<JsonValue> {
    let cells: [(&str, &[(&str, &str)]); 12] = [
        ("underage", &[("age", "< 18")]),
        ("sanctioned", &[("country", r#""KP", "IR", "SY""#)]),
        (
            "bad_email",
            &[("email", r#"not matches($, "^[^@\s]+@[^@\s]+\.[a-z]+$")"#)],
        ),
        ("high_debt", &[("expr", "debt / income > 0.6")]),
        ("low_score", &[("score", "< 500")]),
        (
            "prime",
            &[
                ("score", ">= 750"),
                ("income", "> 50000"),
                ("employment", r#""employed", "self_employed""#),
            ],
        ),
        (
            "near_prime",
            &[("score", "[650..750]"), ("income", "> 30000")],
        ),
        (
            "student",
            &[("employment", r#""student""#), ("age", "< 26")],
        ),
        ("senior", &[("age", ">= 65"), ("income", "> 20000")]),
        ("partner", &[("email", r#"endsWith($, "@example.com")"#)]),
        (
            "manual_review",
            &[("expr", "score == null or income == null")],
        ),
        ("default", &[]),
    ];
    cells
        .iter()
        .zip(1..)
        .map(|((name, cells), rate)| {
            let mut row = json!({
                "_id": name,
                "decision": format!("'{name}'"),
                "rate": format!("base_rate * {rate}"),
            });
            for (column, cell) in *cells {
                row[*column] = json!(cell);
            }
            row
        })
        .collect()
}

fn decision(hit_policy: &str) -> Decision {
    let field = |id: &str| json!({ "id": id, "name": id, "field": id });
    let inputs: Vec<JsonValue> = ["age", "country", "email", "score", "income", "employment"]
        .into_iter()
        .map(field)
        .chain([json!({ "id": "expr", "name": "expr" })])
        .collect();
    let table = json!({
        "id": "dt", "name": "screening", "type": "decisionTableNode",
        "content": {
            "hitPolicy": hit_policy, "passThrough": false,
            "inputs": inputs,
            "outputs": [field("decision"), field("rate")],
            "rules": rows(),
        }
    });
    let content: DecisionContent = serde_json::from_value(json!({
        "nodes": [
            { "id": "in", "name": "request", "type": "inputNode", "content": {} },
            table,
            { "id": "out", "name": "response", "type": "outputNode", "content": {} },
        ],
        "edges": [
            { "id": "e1", "sourceId": "in", "targetId": "dt" },
            { "id": "e2", "sourceId": "dt", "targetId": "out" },
        ],
    }))
    .unwrap();
    let mut decision = DecisionEngine::default()
        .create_decision(Arc::new(content))
        .unwrap();
    decision.compile();
    decision
}

fn to_json(value: &Value) -> JsonValue {
    match value {
        Value::Null(_) => JsonValue::Null,
        Value::Int(i) => json!(i),
        Value::Float(f) => json!(f),
        Value::Decimal(d) => json!(d.to_f64()),
        Value::String(s) => json!(s),
        Value::Map(map) => JsonValue::Object(
            map.iter()
                .map(|(k, v)| (k.to_owned(), to_json(v)))
                .collect(),
        ),
        other => panic!("not in the workload: {other:?}"),
    }
}

/// The names zen selected.
fn zen_selected(result: &JsonValue) -> Vec<String> {
    let name = |row: &JsonValue| row["decision"].as_str().unwrap().to_owned();
    match result {
        JsonValue::Array(rows) => rows.iter().map(name).collect(),
        row => vec![name(row)],
    }
}

/// Both engines select the same rules for every applicant, and zen leaves the
/// shared inputs as they were.
fn check_agreement(policy: HitPolicy, zen: &Decision, inputs: &[Value], variables: &[Variable]) {
    let rules = common::ruleset(policy);
    for (input, variable) in inputs.iter().zip(variables) {
        let ours: Vec<String> = rules
            .evaluate(input)
            .unwrap()
            .selected()
            .map(|r| r.rule.name().to_owned())
            .collect();
        let before = variable.to_value();
        let response = block_on(zen.evaluate(variable.clone())).unwrap();
        let theirs = zen_selected(&response.result.to_value());
        assert_eq!(ours, theirs, "{policy:?} disagrees on {}", to_json(input));
        assert_eq!(variable.to_value(), before, "zen changed its input");
    }
}

fn compare(c: &mut Criterion) {
    let inputs = common::inputs(1_000);
    let variables: Vec<Variable> = inputs.iter().map(|i| to_json(i).into()).collect();
    let mut group = c.benchmark_group("compare");
    group.throughput(Throughput::Elements(inputs.len() as u64));
    for (policy, name) in [(HitPolicy::First, "first"), (HitPolicy::Collect, "collect")] {
        let rules = common::ruleset(policy);
        let zen = decision(name);
        check_agreement(policy, &zen, &inputs, &variables);
        group.bench_function(format!("cleanrules/batch_1000/{name}"), |b| {
            b.iter(|| {
                for input in black_box(&inputs) {
                    black_box(rules.evaluate(input).unwrap());
                }
            });
        });
        group.bench_function(format!("zen/batch_1000/{name}"), |b| {
            b.iter(|| {
                for variable in black_box(&variables) {
                    black_box(block_on(zen.evaluate(variable.clone())).unwrap());
                }
            });
        });
    }
    group.finish();
}

criterion_group!(benches, compare);
criterion_main!(benches);
