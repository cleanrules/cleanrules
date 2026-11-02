//! Explanations and summaries in JSON (`spec/json-format.md`, *Explanations*,
//! *Summaries*). Both are output only.

use crate::eval::Evaluated;
use crate::explain::{Cause, Explanation, Gap, GroupKind, Requirement, Summary};
use crate::format::condition::write_node;
use crate::format::result::{write_path, write_reason, write_truth};
use crate::format::value::write_value;
use crate::json::Json;
use crate::logic::Truth;
use crate::rules::Decision;
use crate::value::Value;

fn string(text: &str) -> Json {
    Json::String(text.to_owned())
}

/// The JSON Pointer of every result of the decision's condition trees, by address.
fn pointers<'d>(decision: &'d Decision<'_>) -> Vec<(*const Evaluated<'d, Truth>, String)> {
    fn walk<'d>(
        result: &'d Evaluated<'_, Truth>,
        pointer: &str,
        out: &mut Vec<(*const Evaluated<'d, Truth>, String)>,
    ) {
        out.push((std::ptr::from_ref(result).cast(), pointer.to_owned()));
        for (i, child) in result.children.iter().enumerate() {
            walk(child, &format!("{pointer}/children/{i}"), out);
        }
    }
    let mut out = Vec::new();
    for (i, result) in decision.results().iter().enumerate() {
        walk(
            &result.when,
            &format!("/decision/results/{i}/when"),
            &mut out,
        );
    }
    out
}

fn write_cause(cause: &Cause<'_, '_>, pointers: &[(*const Evaluated<'_, Truth>, String)]) -> Json {
    let address: *const Evaluated<'_, Truth> = std::ptr::from_ref(cause.result).cast();
    let at = pointers
        .iter()
        .find(|(p, _)| std::ptr::eq(*p, address))
        .map_or("", |(_, s)| s.as_str());
    let values = cause
        .values
        .iter()
        .map(|(path, value)| {
            Json::Object(vec![
                ("path".into(), write_path(path)),
                ("value".into(), write_value(value)),
            ])
        })
        .collect();
    let mut members = vec![
        ("condition".to_owned(), write_node(&cause.result.node)),
        ("at".to_owned(), string(at)),
        ("truth".to_owned(), write_truth(cause.truth())),
        ("negated".to_owned(), Json::Bool(cause.negated)),
        ("values".to_owned(), Json::Array(values)),
    ];
    if let Some(reason) = cause.reason() {
        members.push(("reason".into(), write_reason(reason)));
    }
    if let Some(counterfactual) = &cause.counterfactual {
        let mut fields = vec![
            ("path".to_owned(), write_path(&counterfactual.path)),
            ("op".to_owned(), string(counterfactual.op.tag())),
            ("target".to_owned(), write_value(&counterfactual.target)),
        ];
        match &counterfactual.gap {
            Some(Gap::Number(value)) => fields.push(("gap".into(), write_value(value))),
            Some(Gap::Days(days)) => fields.push((
                "gap".into(),
                Json::Object(vec![("days".into(), write_value(&Value::Int(*days)))]),
            )),
            Some(Gap::Microseconds(micros)) => fields.push((
                "gap".into(),
                Json::Object(vec![(
                    "microseconds".into(),
                    write_value(&Value::Int(*micros)),
                )]),
            )),
            None => {}
        }
        members.push(("counterfactual".into(), Json::Object(fields)));
    }
    Json::Object(members)
}

fn write_requirement(
    requirement: &Requirement<'_, '_>,
    pointers: &[(*const Evaluated<'_, Truth>, String)],
) -> Json {
    match requirement {
        Requirement::Cause(cause) => write_cause(cause, pointers),
        Requirement::Group(kind, items) => {
            let key = if *kind == GroupKind::All {
                "all"
            } else {
                "any"
            };
            Json::Object(vec![(
                key.into(),
                Json::Array(
                    items
                        .iter()
                        .map(|i| write_requirement(i, pointers))
                        .collect(),
                ),
            )])
        }
    }
}

/// Write an explanation's content (the object under `"explanation"`).
#[must_use]
pub fn write_explanation(explanation: &Explanation<'_, '_>) -> Json {
    let decision = explanation.decision;
    let ruleset = decision.ruleset();
    let pointers = pointers(decision);
    let rules = explanation
        .rules
        .iter()
        .map(|entry| {
            let mut members = vec![
                ("rule".to_owned(), string(entry.rule.name())),
                ("status".to_owned(), string(entry.status.as_str())),
            ];
            if let Some(note) = &entry.note {
                let mut fields = vec![
                    ("kind".to_owned(), string(note.kind.as_str())),
                    ("winner".to_owned(), string(&note.winner)),
                ];
                if let Some((mine, theirs)) = note.priorities {
                    fields.push(("priority".into(), write_value(&Value::Int(mine))));
                    fields.push(("winner_priority".into(), write_value(&Value::Int(theirs))));
                }
                members.push(("note".into(), Json::Object(fields)));
            }
            if let Some(achievable) = entry.achievable {
                members.push(("achievable".into(), Json::Bool(achievable)));
            }
            members.push((
                "requirement".into(),
                write_requirement(&entry.requirement, &pointers),
            ));
            Json::Object(members)
        })
        .collect();
    Json::Object(vec![
        ("hit_policy".into(), string(ruleset.hit_policy().as_str())),
        ("logic".into(), string(ruleset.logic().as_str())),
        (
            "selected".into(),
            Json::Array(decision.selected().map(|r| string(r.rule.name())).collect()),
        ),
        ("rules".into(), Json::Array(rules)),
    ])
}

/// Write a summary's content (the object under `"summary"`).
#[must_use]
pub fn write_summary(summary: &Summary<'_>) -> Json {
    let ruleset = summary.ruleset;
    let mut members = vec![(
        "hit_policy".to_owned(),
        string(ruleset.hit_policy().as_str()),
    )];
    if let Some(aggregate) = ruleset.aggregate() {
        members.push(("aggregate".into(), string(aggregate.as_str())));
    }
    members.push(("logic".into(), string(ruleset.logic().as_str())));
    if ruleset.strict() {
        members.push(("strict".into(), Json::Bool(true)));
    }
    let rules = ruleset
        .rules()
        .iter()
        .enumerate()
        .map(|(i, rule)| {
            let mut fields = vec![
                ("name".to_owned(), string(rule.name())),
                (
                    "priority".to_owned(),
                    write_value(&Value::Int(rule.get_priority())),
                ),
                (
                    "outputs".to_owned(),
                    Json::Array(rule.then().iter().map(|(k, _)| string(k)).collect()),
                ),
            ];
            if summary.actions.get(i).copied().unwrap_or(false) {
                fields.push(("action".into(), Json::Bool(true)));
            }
            if let Some(action) = rule.get_action_name() {
                fields.push(("do".into(), string(action)));
            }
            Json::Object(fields)
        })
        .collect();
    members.push(("rules".into(), Json::Array(rules)));
    let fields = summary
        .fields
        .iter()
        .map(|usage| {
            Json::Object(vec![
                ("path".into(), write_path(&usage.path)),
                (
                    "conditions".into(),
                    Json::Array(usage.conditions.iter().map(|n| string(n)).collect()),
                ),
                (
                    "payloads".into(),
                    Json::Array(
                        usage
                            .payloads
                            .iter()
                            .map(|(rule, key)| {
                                Json::Object(vec![
                                    ("rule".into(), string(rule)),
                                    ("key".into(), string(key)),
                                ])
                            })
                            .collect(),
                    ),
                ),
            ])
        })
        .collect();
    members.push(("fields".into(), Json::Array(fields)));
    members.push((
        "outputs".into(),
        Json::Array(summary.outputs.iter().map(|k| string(k)).collect()),
    ));
    let tags = summary
        .tags
        .iter()
        .map(|(tag, names)| {
            Json::Object(vec![
                ("tag".into(), string(tag)),
                (
                    "rules".into(),
                    Json::Array(names.iter().map(|n| string(n)).collect()),
                ),
            ])
        })
        .collect();
    members.push(("tags".into(), Json::Array(tags)));
    Json::Object(members)
}
