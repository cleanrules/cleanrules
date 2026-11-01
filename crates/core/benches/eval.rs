//! Evaluation: one input, a batch of 1,000, one condition tree, and reading the
//! rule set from JSON.

// A benchmark: a panic is the failure report.
#![allow(clippy::unwrap_used, clippy::panic, missing_docs)]

mod common;

use std::hint::black_box;

use cleanrules::eval::{Options, evaluate_tree};
use cleanrules::format::{read_document, write_ruleset};
use cleanrules::json::{Json, JsonNumber, Limits, to_string};
use cleanrules::logic::Kleene;
use cleanrules::rules::HitPolicy;
use criterion::{Criterion, Throughput, criterion_group, criterion_main};

const POLICIES: [HitPolicy; 3] = [HitPolicy::First, HitPolicy::Priority, HitPolicy::Collect];

fn evaluate(c: &mut Criterion) {
    let inputs = common::inputs(1_000);
    let mut group = c.benchmark_group("evaluate");
    for policy in POLICIES {
        let rules = common::ruleset(policy);
        group.throughput(Throughput::Elements(1));
        group.bench_function(format!("one/{}", policy.as_str()), |b| {
            let mut next = inputs.iter().cycle();
            b.iter(|| rules.evaluate(black_box(next.next().unwrap())).unwrap());
        });
        group.throughput(Throughput::Elements(inputs.len() as u64));
        group.bench_function(format!("batch_1000/{}", policy.as_str()), |b| {
            b.iter(|| rules.evaluate_many(black_box(&inputs)).unwrap());
        });
    }
    group.finish();
}

fn condition(c: &mut Criterion) {
    let inputs = common::inputs(1_000);
    let rules = common::ruleset(HitPolicy::First);
    let prime = rules.rules().iter().find(|r| r.name() == "prime").unwrap();
    c.bench_function("condition/prime", |b| {
        let mut next = inputs.iter().cycle();
        b.iter(|| {
            evaluate_tree(
                prime.when(),
                black_box(next.next().unwrap()),
                &Kleene,
                Options::default(),
            )
            .unwrap()
        });
    });
}

fn read(c: &mut Criterion) {
    let rules = common::ruleset(HitPolicy::Collect);
    let document = Json::Object(vec![
        (
            "version".into(),
            Json::Number(JsonNumber::from_literal("1")),
        ),
        ("ruleset".into(), write_ruleset(&rules)),
    ]);
    let text = to_string(&document);
    let mut group = c.benchmark_group("json");
    group.throughput(Throughput::Bytes(text.len() as u64));
    group.bench_function("read_ruleset", |b| {
        b.iter(|| read_document(black_box(&text), &Limits::default()).unwrap());
    });
    group.bench_function("write_ruleset", |b| {
        b.iter(|| to_string(&write_ruleset(black_box(&rules))));
    });
    group.finish();
}

criterion_group!(benches, evaluate, condition, read);
criterion_main!(benches);
