//! Documents (`spec/json-format.md`, *Documents*): `{"version": 1, <content>}`.

use crate::FORMAT_VERSION;
use crate::ast::{Kind, Node};
use crate::error::{Error, ErrorCode, Result};
use crate::format::condition::{build, shape, write_node};
use crate::format::decision::read_decision;
use crate::format::rules::{
    build_rule, build_ruleset, shape_rule, shape_ruleset, write_rule, write_ruleset,
};
use crate::format::table::{read_table, write_table};
use crate::json::{Json, JsonNumber, Limits, parse, pointer_token};
use crate::rules::{Decision, Rule, RuleSet};
use crate::table::DecisionTable;

/// The content keys a document may have, in the order of the spec.
const CONTENT_KEYS: [&str; 7] = [
    "condition",
    "rule",
    "ruleset",
    "decision",
    "explanation",
    "summary",
    "decision_table",
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
    /// A decision table (`spec/tables.md`).
    DecisionTable(DecisionTable),
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
    read_parsed_document(&parse(text, limits)?, limits)
}

/// Read a document already parsed into JSON values, such as one read from YAML
/// (`spec/json-format.md`, *YAML*): every check after parsing, in the same order.
///
/// # Errors
///
/// As [`read_document`], except syntax errors, which belong to the parser.
pub fn read_parsed_document(json: &Json, limits: &Limits) -> Result<Document> {
    let (members, content) = envelope(json)?;
    let get = |key: &str| members.iter().find(|(k, _)| k == key).map(|(_, v)| v);
    let check_top_level = || top_level(members, content);
    match content {
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
                    format!("'{}' is not a condition", node.tag()),
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
        "decision_table" => {
            check_top_level()?;
            Ok(Document::DecisionTable(read_table(
                get("decision_table").unwrap_or(&Json::Null),
                "/decision_table",
                limits,
            )?))
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

/// Steps 1 and 2 of reading (`spec/json-format.md`, *Errors and limits*): a JSON
/// object with a known `version` and exactly one content key.
fn envelope(json: &Json) -> Result<(&[(String, Json)], &'static str)> {
    let Json::Object(members) = json else {
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
    let present: Vec<&'static str> = CONTENT_KEYS
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
    Ok((members, content))
}

/// The top-level keys besides the content: `$schema` a string, nothing unknown.
fn top_level(members: &[(String, Json)], content: &str) -> Result<()> {
    if let Some((_, schema)) = members.iter().find(|(k, _)| k == "$schema")
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
}

/// Read a decision document with the rule set that produced it
/// (`spec/json-format.md`, *Reading a decision*).
///
/// # Errors
///
/// As [`read_document`], then `decision_mismatch` for the first failed check.
pub fn read_decision_document<'a>(
    text: &str,
    ruleset: &'a RuleSet,
    limits: &Limits,
) -> Result<Decision<'a>> {
    read_parsed_decision_document(&parse(text, limits)?, ruleset, limits)
}

/// Read a YAML document (`spec/json-format.md`, *YAML*): parsed by
/// [`crate::yaml::parse`], then read as [`read_parsed_document`] reads JSON.
///
/// # Errors
///
/// As [`crate::yaml::parse`], then as [`read_parsed_document`].
///
/// # Examples
///
/// ```
/// use cleanrules::format::{Document, read_yaml_document};
/// use cleanrules::json::Limits;
///
/// let text = "version: 1\ncondition: {op: gt, left: {op: field, path: [age]}, right: {op: const, value: 17}}\n";
/// let Document::Condition(node) = read_yaml_document(text, &Limits::default())? else {
///     unreachable!()
/// };
/// assert_eq!(node.tag(), "gt");
/// # Ok::<(), cleanrules::Error>(())
/// ```
#[cfg(feature = "yaml")]
pub fn read_yaml_document(text: &str, limits: &Limits) -> Result<Document> {
    read_parsed_document(&crate::yaml::parse(text, limits)?, limits)
}

/// Read a YAML decision document with its rule set.
///
/// # Errors
///
/// As [`crate::yaml::parse`], then as [`read_parsed_decision_document`].
#[cfg(feature = "yaml")]
pub fn read_yaml_decision_document<'a>(
    text: &str,
    ruleset: &'a RuleSet,
    limits: &Limits,
) -> Result<Decision<'a>> {
    read_parsed_decision_document(&crate::yaml::parse(text, limits)?, ruleset, limits)
}

/// Read a decision document already parsed into JSON values, with its rule set.
///
/// # Errors
///
/// As [`read_decision_document`], except syntax errors.
pub fn read_parsed_decision_document<'a>(
    json: &Json,
    ruleset: &'a RuleSet,
    limits: &Limits,
) -> Result<Decision<'a>> {
    let (members, content) = envelope(json)?;
    if content != "decision" {
        return Err(format_error("", "this is not a decision document"));
    }
    let null = Json::Null;
    let decision = members
        .iter()
        .find(|(k, _)| k == "decision")
        .map_or(&null, |(_, v)| v);
    // The shape of the content comes before unknown top-level keys; reading the
    // decision checks both shape and consistency, so the order is kept by checking
    // the top level when the content's shape is valid.
    top_level(members, content)?;
    read_decision(decision, "/decision", ruleset, limits.max_depth)
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
        Document::DecisionTable(table) => Json::Object(vec![
            ("version".into(), version),
            ("decision_table".into(), write_table(table)),
        ]),
    }
}
