//! The JSON form of an error (`spec/errors.md`, *JSON form*).

use std::error::Error as _;

use crate::error::{Context, Error};
use crate::format::{write_node, write_path};
use crate::json::{Json, JsonNumber};

fn strings(items: &[String]) -> Json {
    Json::Array(items.iter().map(|s| Json::String(s.clone())).collect())
}

fn number(n: u64) -> Json {
    Json::Number(JsonNumber::from_literal(n.to_string()))
}

/// Write an error: its code, message and hint, the context fields that apply,
/// and the message of its cause.
#[must_use]
pub fn write_error(error: &Error) -> Json {
    // Every field, so that a new one cannot be left out of the JSON form.
    let Context {
        pointer,
        line,
        column,
        rule,
        rules,
        path,
        types,
        row,
        name,
        available,
        executed,
        node,
        operation,
        reason,
    } = error.context();
    let mut members = vec![
        (
            "code".to_owned(),
            Json::String(error.code().as_str().into()),
        ),
        ("message".to_owned(), Json::String(error.message().into())),
    ];
    let mut add = |key: &str, value: Json| members.push((key.to_owned(), value));
    if let Some(hint) = error.hint() {
        add("hint", Json::String(hint.into()));
    }
    if let Some(pointer) = pointer {
        add("pointer", Json::String(pointer.clone()));
    }
    if let Some(line) = line {
        add("line", number(*line));
    }
    if let Some(column) = column {
        add("column", number(*column));
    }
    if let Some(rule) = rule {
        add("rule", Json::String(rule.clone()));
    }
    if !rules.is_empty() {
        add("rules", strings(rules));
    }
    if let Some(path) = path {
        add("path", write_path(path));
    }
    if !types.is_empty() {
        let types = types.iter().map(|g| Json::String(g.as_str().into()));
        add("types", Json::Array(types.collect()));
    }
    if let Some(row) = row {
        add("row", number(*row));
    }
    if let Some(name) = name {
        add("name", Json::String(name.clone()));
    }
    if !available.is_empty() {
        add("available", strings(available));
    }
    if !executed.is_empty() {
        add("executed", strings(executed));
    }
    if let Some(node) = node {
        add("node", write_node(node));
    }
    if let Some(operation) = operation {
        add("operation", Json::String(operation.clone()));
    }
    if let Some(reason) = reason {
        add("reason", Json::String(reason.clone()));
    }
    if let Some(cause) = error.source() {
        add("cause", Json::String(cause.to_string()));
    }
    members.shrink_to_fit();
    Json::Object(members)
}

#[cfg(test)]
mod tests {
    use super::write_error;
    use crate::error::{Context, Error, ErrorCode, Segment};
    use crate::json::to_string;
    use crate::value::Group;

    #[test]
    fn members_in_order_and_only_when_they_apply() {
        let error = Error::new(ErrorCode::FormatError, "bad").at_position(2, 5);
        assert_eq!(
            to_string(&write_error(&error)),
            r#"{"code":"format_error","message":"bad","line":2,"column":5}"#
        );
        let context = Context {
            pointer: Some("/rule/when".into()),
            rule: Some("r".into()),
            rules: vec!["a".into(), "b".into()],
            path: Some(vec![Segment::Key("items".into()), Segment::Index(0)]),
            types: vec![Group::Number, Group::String],
            row: Some(3),
            name: Some("n".into()),
            available: vec!["m".into()],
            executed: vec!["a".into()],
            operation: Some("csv".into()),
            reason: Some("tables only".into()),
            ..Context::default()
        };
        let error = Error::new(ErrorCode::TypeMismatch, "mixed")
            .with_hint("convert it")
            .with_context(context)
            .with_node(&crate::build::field("x").gt(1).build().unwrap())
            .caused_by(std::fmt::Error);
        assert_eq!(
            to_string(&write_error(&error)),
            concat!(
                r#"{"code":"type_mismatch","message":"mixed","hint":"convert it","#,
                r#""pointer":"/rule/when","rule":"r","rules":["a","b"],"path":["items",0],"#,
                r#""types":["number","string"],"row":3,"name":"n","available":["m"],"#,
                r#""executed":["a"],"node":{"op":"gt","left":{"op":"field","path":["x"]},"#,
                r#""right":{"op":"const","value":1}},"operation":"csv","reason":"tables only","#,
                r#""cause":"an error occurred when formatting an argument"}"#
            )
        );
    }
}
