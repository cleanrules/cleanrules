//! The JSON form of an audit (`spec/audit.md`, *JSON*), output only.

use crate::audit::{Audit, UndecidedReason};
use crate::format::{write_path, write_value};
use crate::json::{Json, JsonNumber};

fn string(text: &str) -> Json {
    Json::String(text.to_owned())
}

fn number(n: usize) -> Json {
    Json::Number(JsonNumber::from_literal(n.to_string()))
}

fn strings<S: AsRef<str>>(items: &[S]) -> Json {
    Json::Array(items.iter().map(|s| string(s.as_ref())).collect())
}

/// The content of an audit document: the object under `"audit"`.
#[must_use]
pub fn write_audit(audit: &Audit) -> Json {
    let mut members = vec![("source".to_owned(), string(audit.source.as_str()))];
    if let Some(document) = audit.document {
        members.push(("document".into(), string(document.as_str())));
    }
    members.push(("hit_policy".into(), string(audit.hit_policy.as_str())));
    if let Some(aggregate) = audit.aggregate {
        members.push(("aggregate".into(), string(aggregate.as_str())));
    }
    members.push(("logic".into(), string(audit.logic.as_str())));
    if audit.strict {
        members.push(("strict".into(), Json::Bool(true)));
    }
    if let Some(digest) = audit.ruleset_digest {
        members.push(("ruleset_digest".into(), string(&digest.to_string())));
    }
    members.push((
        "limits".into(),
        Json::Object(vec![
            ("max_terms".into(), number(audit.limits.max_terms)),
            ("max_steps".into(), number(audit.limits.max_steps)),
        ]),
    ));
    let rules = audit
        .rules
        .iter()
        .map(|rule| {
            let mut entry = vec![("name".to_owned(), string(&rule.name))];
            if let Some(id) = &rule.id {
                entry.push(("id".into(), string(id)));
            }
            if let Some(row) = rule.row {
                entry.push(("row".into(), number(row)));
            }
            Json::Object(entry)
        })
        .collect();
    members.push(("rules".into(), Json::Array(rules)));
    let checks = audit
        .checks
        .iter()
        .map(|c| {
            Json::Object(vec![
                ("check".into(), string(c.check.as_str())),
                ("status".into(), string(c.status.as_str())),
            ])
        })
        .collect();
    members.push(("checks".into(), Json::Array(checks)));
    let not_modelled = audit
        .not_modelled
        .iter()
        .map(|n| {
            let mut entry = vec![
                ("rule".to_owned(), string(&n.rule)),
                ("op".to_owned(), string(n.op)),
                ("pointer".to_owned(), string(&n.pointer)),
            ];
            if let Some(limit) = n.limit {
                entry.push(("limit".into(), string(limit.as_str())));
            }
            Json::Object(entry)
        })
        .collect();
    members.push(("not_modelled".into(), Json::Array(not_modelled)));
    let undecided = audit
        .undecided
        .iter()
        .map(|u| {
            let mut entry = vec![
                ("check".to_owned(), string(u.check.as_str())),
                ("rules".to_owned(), strings(&u.rules)),
                ("reason".to_owned(), string(u.reason.as_str())),
            ];
            if let UndecidedReason::Limit(limit) = u.reason {
                entry.push(("limit".into(), string(limit.as_str())));
            }
            if let Some(remaining) = u.remaining {
                entry.push(("remaining".into(), number(remaining)));
            }
            Json::Object(entry)
        })
        .collect();
    members.push(("undecided".into(), Json::Array(undecided)));
    let diagnostics = audit.diagnostics.iter().map(write_diagnostic).collect();
    members.push(("diagnostics".into(), Json::Array(diagnostics)));
    Json::Object(members)
}

fn write_diagnostic(d: &crate::audit::Diagnostic) -> Json {
    let mut entry = vec![
        ("code".to_owned(), string(d.code.as_str())),
        ("severity".to_owned(), string(d.severity.as_str())),
        ("rules".to_owned(), strings(&d.rules)),
        ("message".to_owned(), string(&d.message)),
    ];
    if let Some(pointer) = &d.pointer {
        entry.push(("pointer".into(), string(pointer)));
    }
    if let Some(path) = &d.path {
        entry.push(("path".into(), write_path(path)));
    }
    if !d.groups.is_empty() {
        let groups: Vec<&str> = d.groups.iter().map(|g| g.as_str()).collect();
        entry.push(("groups".into(), strings(&groups)));
    }
    if let Some(witness) = &d.witness {
        entry.push(("witness".into(), write_value(witness)));
    }
    Json::Object(entry)
}
