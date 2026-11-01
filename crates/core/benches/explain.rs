//! Explanation: explaining 1,000 decisions, writing them as JSON, and the
//! summary of the rule set.

// A benchmark: a panic is the failure report.
#![allow(clippy::unwrap_used, clippy::panic, missing_docs)]

mod common;

use std::hint::black_box;

use cleanrules::explain::{explain, summary};
use cleanrules::format::{write_explanation, write_summary};
use cleanrules::json::to_string;
use cleanrules::rules::HitPolicy;
use criterion::{Criterion, Throughput, criterion_group, criterion_main};

fn explanations(c: &mut Criterion) {
    let inputs = common::inputs(1_000);
    let mut group = c.benchmark_group("explain");
    group.throughput(Throughput::Elements(inputs.len() as u64));
    for policy in [HitPolicy::First, HitPolicy::Collect] {
        let rules = common::ruleset(policy);
        let decisions = rules.evaluate_many(&inputs).unwrap();
        group.bench_function(format!("batch_1000/{}", policy.as_str()), |b| {
            b.iter(|| {
                for decision in black_box(&decisions) {
                    black_box(explain(decision));
                }
            });
        });
        group.bench_function(format!("batch_1000_json/{}", policy.as_str()), |b| {
            b.iter(|| {
                for decision in black_box(&decisions) {
                    black_box(to_string(&write_explanation(&explain(decision))));
                }
            });
        });
        group.bench_function(
            format!("evaluate_and_explain_1000/{}", policy.as_str()),
            |b| {
                b.iter(|| {
                    for input in black_box(&inputs) {
                        black_box(explain(&rules.evaluate(input).unwrap()));
                    }
                });
            },
        );
    }
    group.finish();
}

fn summaries(c: &mut Criterion) {
    let rules = common::ruleset(HitPolicy::First);
    c.bench_function("summary", |b| {
        b.iter(|| to_string(&write_summary(&summary(black_box(&rules)))));
    });
}

criterion_group!(benches, explanations, summaries);
criterion_main!(benches);
