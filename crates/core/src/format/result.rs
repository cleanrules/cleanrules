//! Result trees in JSON (`spec/json-format.md`, *Result trees*).

use crate::error::Segment;
use crate::eval::{Evaluated, Outcome, Reason};
use crate::format::value::write_value;
use crate::json::{Json, JsonNumber};
use crate::logic::Truth;

/// A field path as JSON: strings and integers.
#[must_use]
pub fn write_path(path: &[Segment]) -> Json {
    Json::Array(
        path.iter()
            .map(|segment| match segment {
                Segment::Key(key) => Json::String(key.clone()),
                Segment::Index(index) => Json::Number(JsonNumber::from_literal(index.to_string())),
            })
            .collect(),
    )
}

/// A reason as JSON, omitting empty `paths` and `types` and an absent `detail`.
#[must_use]
pub fn write_reason(reason: &Reason) -> Json {
    let mut members = vec![("code".to_owned(), Json::String(reason.code.as_str().into()))];
    if !reason.paths.is_empty() {
        members.push((
            "paths".into(),
            Json::Array(reason.paths.iter().map(|p| write_path(p)).collect()),
        ));
    }
    if !reason.types.is_empty() {
        let types = reason
            .types
            .iter()
            .map(|g| Json::String(g.as_str().into()))
            .collect();
        members.push(("types".into(), Json::Array(types)));
    }
    if let Some(detail) = &reason.detail {
        members.push(("detail".into(), Json::String(detail.clone())));
    }
    Json::Object(members)
}

/// A Kleene truth value: `true`, `false`, or `null` for unknown.
#[must_use]
pub fn write_truth(truth: Truth) -> Json {
    match truth {
        Truth::True => Json::Bool(true),
        Truth::False => Json::Bool(false),
        Truth::Unknown => Json::Null,
    }
}

/// A result tree under Kleene logic, in canonical form.
#[must_use]
pub fn write_result(result: &Evaluated<'_, Truth>) -> Json {
    let mut members = vec![("op".to_owned(), Json::String(result.node.tag().into()))];
    if let crate::ast::Expr::Field(path) = result.node.expr() {
        members.push(("path".into(), write_path(path)));
    }
    match &result.outcome {
        Outcome::Truth(truth) => members.push(("truth".into(), write_truth(*truth))),
        Outcome::Value(value) => members.push(("value".into(), write_value(value))),
    }
    if let Some(reason) = &result.reason {
        members.push(("reason".into(), write_reason(reason)));
    }
    if !result.children.is_empty() {
        members.push((
            "children".into(),
            Json::Array(result.children.iter().map(write_result).collect()),
        ));
    }
    Json::Object(members)
}
