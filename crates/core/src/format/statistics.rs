//! The JSON form of a run's statistics (`spec/tests.md`, *Statistics*,
//! *JSON*): the `statistics` member of a test run, output only.

use crate::cases::{Missed, Statistics};
use crate::format::write_value;
use crate::json::{Json, JsonNumber};

fn string(text: &str) -> Json {
    Json::String(text.to_owned())
}

fn number(n: usize) -> Json {
    Json::Number(JsonNumber::from_literal(n.to_string()))
}

/// The statistics of a run, as a test run writes them under `statistics`.
#[must_use]
pub fn write_statistics(statistics: &Statistics) -> Json {
    let rules = statistics
        .rules
        .iter()
        .map(|rule| {
            let mut members = vec![("name".to_owned(), string(&rule.name))];
            if let Some(id) = &rule.id {
                members.push(("id".into(), string(id)));
            }
            if let Some(row) = rule.row {
                members.push(("row".into(), number(row)));
            }
            members.push(("pointer".into(), string(&rule.pointer)));
            members.extend([
                ("selected".into(), number(rule.selected)),
                ("fired".into(), number(rule.fired)),
                ("not_fired".into(), number(rule.not_fired)),
                ("unknown".into(), number(rule.unknown)),
            ]);
            let conditions = rule
                .conditions
                .iter()
                .map(|c| {
                    Json::Object(vec![
                        ("pointer".into(), string(&c.pointer)),
                        ("op".into(), string(c.op)),
                        ("true".into(), number(c.true_count)),
                        ("false".into(), number(c.false_count)),
                        ("unknown".into(), number(c.unknown_count)),
                    ])
                })
                .collect();
            members.push(("conditions".into(), Json::Array(conditions)));
            Json::Object(members)
        })
        .collect();
    let mut members = vec![
        ("cases".to_owned(), number(statistics.cases)),
        ("decisions".to_owned(), number(statistics.decisions)),
        ("rules".to_owned(), Json::Array(rules)),
    ];
    if let Some(outputs) = &statistics.outputs {
        let outputs = outputs
            .iter()
            .map(|output| {
                let values = output
                    .values
                    .iter()
                    .map(|v| {
                        Json::Object(vec![
                            ("value".into(), write_value(&v.value)),
                            ("selected".into(), number(v.selected)),
                        ])
                    })
                    .collect();
                Json::Object(vec![
                    ("output".into(), string(&output.output)),
                    ("values".into(), Json::Array(values)),
                ])
            })
            .collect();
        members.push(("outputs".into(), Json::Array(outputs)));
    }
    let coverage = statistics
        .coverage()
        .iter()
        .map(|measure| {
            let missed = measure.missed.iter().map(write_missed).collect();
            let mut members = vec![
                ("measure".to_owned(), string(measure.measure.as_str())),
                ("covered".to_owned(), number(measure.covered)),
                ("total".to_owned(), number(measure.total)),
            ];
            if measure.unreachable > 0 {
                members.push(("unreachable".into(), number(measure.unreachable)));
            }
            members.push(("missed".into(), Json::Array(missed)));
            Json::Object(members)
        })
        .collect();
    members.push(("coverage".into(), Json::Array(coverage)));
    Json::Object(members)
}

/// An item a measure missed, with its place.
fn write_missed(item: &Missed) -> Json {
    Json::Object(match item {
        Missed::Rule { rule, pointer } | Missed::Condition { rule, pointer } => vec![
            ("rule".into(), string(rule)),
            ("pointer".into(), string(pointer)),
        ],
        Missed::OutputValue {
            output,
            value,
            pointer,
        } => vec![
            ("output".into(), string(output)),
            ("value".into(), write_value(value)),
            ("pointer".into(), string(pointer)),
        ],
    })
}
