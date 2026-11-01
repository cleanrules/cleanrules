//! Documents (`spec/json-format.md`, *Documents*): `{"version": 1, <content>}`.

use crate::FORMAT_VERSION;
use crate::ast::{Kind, Node};
use crate::error::{Error, ErrorCode, Result};
use crate::format::condition::{build, shape, write_node};
use crate::format::rules::{
    build_rule, build_ruleset, shape_rule, shape_ruleset, write_rule, write_ruleset,
};
use crate::json::{Json, JsonNumber, Limits, parse, pointer_token};
use crate::rules::{Rule, RuleSet};

/// The content keys a document may have, in the order of the spec.
const CONTENT_KEYS: [&str; 6] = [
    "condition",
    "rule",
    "ruleset",
    "decision",
    "explanation",
    "summary",
];

/// What a document holds.
#[derive(Clone, Debug)]
#[non_exhaustive]
pub enum Document {
    /// A condition.
    Condition(Node),
    /// A rule.
    Rule(Rule),
    /// A rule set.
    RuleSet(RuleSet),
}

impl Document {
    /// The condition, if the document holds one.
    #[must_use]
    pub fn into_condition(self) -> Option<Node> {
        match self {
            Document::Condition(node) => Some(node),
            _ => None,
        }
    }
}

fn format_error(pointer: &str, message: impl Into<String>) -> Error {
    Error::new(ErrorCode::FormatError, message).at_pointer(pointer)
}

/// Read a document from JSON text.
///
/// Errors follow the order of `spec/json-format.md` (*Errors and limits*): size,
/// nesting and syntax; then `version`; then the shape, in pre-order; then the
/// construction rules, in post-order.
///
/// # Errors
///
/// `format_error`, `unsupported_version`, `limit_exceeded`, `expression_error`, or
/// `unsupported` for an output-only document and for content this version of the
/// core does not read yet.
pub fn read_document(text: &str, limits: &Limits) -> Result<Document> {
    let json = parse(text, limits)?;
    let Json::Object(members) = &json else {
        return Err(format_error("", "a document is a JSON object"));
    };
    let get = |key: &str| members.iter().find(|(k, _)| k == key).map(|(_, v)| v);
    let version_ok = match get("version") {
        Some(Json::Number(n)) => {
            n.is_integer_literal() && n.as_i128() == Some(i128::from(FORMAT_VERSION))
        }
        _ => false,
    };
    if !version_ok {
        return Err(Error::new(
            ErrorCode::UnsupportedVersion,
            format!("this reader knows format version {FORMAT_VERSION}"),
        )
        .at_pointer("/version"));
    }
    let present: Vec<&str> = CONTENT_KEYS
        .iter()
        .copied()
        .filter(|k| get(k).is_some())
        .collect();
    let [content] = present.as_slice() else {
        return Err(format_error(
            "",
            format!("a document has exactly one of {}", CONTENT_KEYS.join(", ")),
        ));
    };
    let check_top_level = || -> Result<()> {
        if let Some(schema) = get("$schema")
            && !matches!(schema, Json::String(_))
        {
            return Err(format_error("/$schema", "\"$schema\" must be a string"));
        }
        match members
            .iter()
            .find(|(k, _)| !matches!(k.as_str(), "version" | "$schema") && k != content)
        {
            Some((key, _)) => Err(format_error(
                &format!("/{}", pointer_token(key)),
                format!("unknown key {key:?}"),
            )
            .with_hint("it may come from a newer version of cleanrules")),
            None => Ok(()),
        }
    };
    match *content {
        "condition" => {
            let raw = shape(
                get("condition").unwrap_or(&Json::Null),
                "/condition",
                1,
                limits,
            )?;
            check_top_level()?;
            let node = build(raw)?;
            if node.is_null_constant() || node.kind() == Kind::Value {
                return Err(Error::new(
                    ErrorCode::ExpressionError,
                    format!("a {} is not a condition", node.tag()),
                )
                .at_pointer("/condition"));
            }
            Ok(Document::Condition(node))
        }
        "rule" => {
            let raw = shape_rule(get("rule").unwrap_or(&Json::Null), "/rule", limits)?;
            check_top_level()?;
            Ok(Document::Rule(build_rule(raw)?))
        }
        "ruleset" => {
            let raw = shape_ruleset(get("ruleset").unwrap_or(&Json::Null), "/ruleset", limits)?;
            check_top_level()?;
            Ok(Document::RuleSet(build_ruleset(raw)?))
        }
        "explanation" | "summary" => {
            check_top_level()?;
            Err(Error::new(
                ErrorCode::Unsupported,
                format!("{content} documents are output only"),
            ))
        }
        other => Err(Error::new(
            ErrorCode::Unsupported,
            format!("{other} documents are not read yet"),
        )),
    }
}

/// Write a document in canonical form.
#[must_use]
pub fn write_document(document: &Document) -> Json {
    let version = Json::Number(JsonNumber::from_literal(FORMAT_VERSION.to_string()));
    match document {
        Document::Condition(node) => Json::Object(vec![
            ("version".into(), version),
            ("condition".into(), write_node(node)),
        ]),
        Document::Rule(rule) => Json::Object(vec![
            ("version".into(), version),
            ("rule".into(), write_rule(rule)),
        ]),
        Document::RuleSet(ruleset) => Json::Object(vec![
            ("version".into(), version),
            ("ruleset".into(), write_ruleset(ruleset)),
        ]),
    }
}
