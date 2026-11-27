//! Documents (`spec/json-format.md`, *Documents*): `{"version": 1, <content>}`.

use crate::FORMAT_VERSION;
use crate::ast::{Kind, Node};
use crate::bundle::Bundle;
use crate::cases::Cases;
use crate::error::{Error, ErrorCode, Result};
use crate::format::bundle::{read_bundle, write_bundle};
use crate::format::cases::{read_cases, write_cases};
use crate::format::condition::{build, shape, write_node};
use crate::format::decision::read_decision;
use crate::format::rules::{
    build_rule, build_ruleset, shape_rule, shape_ruleset, write_decision, write_rule, write_ruleset,
};
use crate::format::table::{read_table, write_table};
use crate::json::{Json, JsonNumber, Limits, by_key, parse, pointer_token};
use crate::rules::{Decision, Rule, RuleSet};
use crate::table::DecisionTable;

/// The content keys a document may have, in the order of the spec.
const CONTENT_KEYS: [&str; 12] = [
    "condition",
    "rule",
    "ruleset",
    "decision",
    "explanation",
    "summary",
    "decision_table",
    "bundle",
    "cases",
    "audit",
    "test_run",
    "test_comparison",
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
    /// A bundle: a reproducible package of rules (`spec/bundles.md`).
    Bundle(Box<Bundle>),
    /// A case file: test cases and their expectations (`spec/tests.md`).
    Cases(Cases),
}

impl Document {
    /// A document holding a condition (`spec/json-format.md`, *Documents*):
    /// every reader of condition documents and every writer of one built in
    /// code goes through this check.
    ///
    /// # Errors
    ///
    /// `expression_error` for a value node, such as `x + 1`, or the null
    /// constant: only a condition can be a document. The caller locates it.
    pub fn condition(node: Node) -> Result<Self> {
        if node.is_null_constant() || node.kind() == Kind::Value {
            return Err(Error::new(
                ErrorCode::ExpressionError,
                format!("only a condition can be a document, not the value {node}"),
            )
            .with_hint("compare it first, such as x > 0"));
        }
        Ok(Document::Condition(node))
    }

    /// The rule set the document gives (`spec/json-format.md`, *A rule set from
    /// a document*): its rule set, the one its decision table compiles to, the
    /// one its bundle gives, or a rule set of its one rule with the default
    /// settings.
    ///
    /// # Errors
    ///
    /// `format_error` for a condition, which is not a rule, and for a case
    /// file, which holds no rules.
    pub fn into_ruleset(self) -> Result<RuleSet> {
        match self {
            Document::RuleSet(ruleset) => Ok(ruleset),
            Document::DecisionTable(table) => Ok(table.ruleset().clone()),
            Document::Bundle(bundle) => Ok(bundle.ruleset().clone()),
            Document::Rule(rule) => RuleSet::new(vec![rule], crate::rules::Config::default()),
            Document::Condition(_) => Err(Error::new(
                ErrorCode::FormatError,
                "this document holds a condition, not rules",
            )
            .with_hint("make it the condition of a rule (RULE name IF ...)")),
            Document::Cases(_) => Err(Error::new(
                ErrorCode::FormatError,
                "this document holds test cases, not rules",
            )
            .with_hint("read the rules to test from their own document")),
        }
    }

    /// The document's content key (`spec/json-format.md`, *Documents*):
    /// `condition`, `rule`, `ruleset`, `decision_table`, `bundle` or `cases`.
    /// The output-only keys (`decision`, `explanation`, `summary`, `audit`,
    /// `test_run`, `test_comparison`) are never read into a document.
    #[must_use]
    pub fn content_key(&self) -> &'static str {
        match self {
            Document::Condition(_) => "condition",
            Document::Rule(_) => "rule",
            Document::RuleSet(_) => "ruleset",
            Document::DecisionTable(_) => "decision_table",
            Document::Bundle(_) => "bundle",
            Document::Cases(_) => "cases",
        }
    }

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
            Document::condition(build(raw)?).map_err(|e| e.at_pointer("/condition"))
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
        "bundle" => Ok(Document::Bundle(Box::new(read_bundle(
            get("bundle").unwrap_or(&Json::Null),
            "/bundle",
            limits,
            check_top_level,
        )?))),
        "cases" => {
            let cases = read_cases(get("cases").unwrap_or(&Json::Null), "/cases")?;
            check_top_level()?;
            Ok(Document::Cases(cases))
        }
        "explanation" | "summary" | "audit" | "test_run" | "test_comparison" => {
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
    match by_key(members)
        .into_iter()
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
        Document::Bundle(bundle) => Json::Object(vec![
            ("version".into(), version),
            ("bundle".into(), write_bundle(bundle)),
        ]),
        Document::Cases(cases) => Json::Object(vec![
            ("version".into(), version),
            ("cases".into(), write_cases(cases)),
        ]),
    }
}

/// A syntax documents are read from and written in (`spec/json-format.md`,
/// `spec/text.md`, `spec/tables.md`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum DocumentFormat {
    /// JSON (`.json`).
    Json,
    /// YAML (`.yaml`), with the `yaml` feature.
    Yaml,
    /// The rule syntax (`.rules`).
    Text,
    /// CSV, for decision tables only (`.csv`).
    Csv,
}

impl DocumentFormat {
    /// The format named `json`, `yaml`, `text` or `csv`.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "json" => Self::Json,
            "yaml" => Self::Yaml,
            "text" => Self::Text,
            "csv" => Self::Csv,
            _ => return None,
        })
    }

    /// The format's name.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Json => "json",
            Self::Yaml => "yaml",
            Self::Text => "text",
            Self::Csv => "csv",
        }
    }
}

/// The size limit of a document in `format`, in bytes (`spec/json-format.md`,
/// *Errors and limits*): `max_size` for JSON, `max_yaml_size` for YAML and
/// `max_text_size` for the rule syntax and CSV.
#[must_use]
pub fn size_limit(format: DocumentFormat, limits: &Limits) -> usize {
    match format {
        DocumentFormat::Json => limits.max_size,
        DocumentFormat::Yaml => limits.max_yaml_size,
        DocumentFormat::Text | DocumentFormat::Csv => limits.max_text_size,
    }
}

/// The `limit_exceeded` error of a document in `format` larger than its size
/// limit ([`size_limit`]): with its `size`, in bytes, when it is known, or
/// saying only that it has more bytes than the limit, for one that its caller
/// did not load whole (a command line reads at most one byte more than the
/// limit).
#[must_use]
pub fn too_large(format: DocumentFormat, size: Option<u64>, limits: &Limits) -> Error {
    let limit = size_limit(format, limits);
    let (what, hint) = match format {
        DocumentFormat::Json => (
            "the document",
            "raise the size limit if the document is trusted",
        ),
        DocumentFormat::Yaml => (
            "the YAML text",
            "raise the YAML size limit if the file is trusted",
        ),
        DocumentFormat::Text | DocumentFormat::Csv => {
            ("the text", "raise the size limit if the text is trusted")
        }
    };
    let message = match size {
        Some(size) => format!("{what} has {size} bytes, more than the limit of {limit}"),
        None => format!("{what} has more bytes than the limit of {limit}"),
    };
    Error::new(ErrorCode::LimitExceeded, message).with_hint(hint)
}

/// The error of `bytes` beyond the size limit of `format`, which is checked
/// before they are decoded as UTF-8 (`spec/json-format.md`, *Errors and
/// limits*).
fn check_size(bytes: &[u8], format: DocumentFormat, limits: &Limits) -> Result<()> {
    if bytes.len() > size_limit(format, limits) {
        return Err(too_large(format, u64::try_from(bytes.len()).ok(), limits));
    }
    Ok(())
}

/// Read a document from bytes in `format`.
///
/// # Errors
///
/// `limit_exceeded` for bytes beyond the size limit of `format`, before they
/// are decoded; `format_error` for bytes that are not UTF-8, at the first
/// invalid byte; then the errors of that format's reader; `unsupported` for
/// YAML in a build without the `yaml` feature.
pub fn read_document_as(bytes: &[u8], format: DocumentFormat, limits: &Limits) -> Result<Document> {
    check_size(bytes, format, limits)?;
    match format {
        DocumentFormat::Json => {
            read_parsed_document(&crate::json::parse_bytes(bytes, limits)?, limits)
        }
        #[cfg(feature = "yaml")]
        DocumentFormat::Yaml => read_yaml_document(crate::json::utf8_text(bytes)?, limits),
        #[cfg(not(feature = "yaml"))]
        DocumentFormat::Yaml => Err(no_yaml()),
        DocumentFormat::Text => crate::text::parse(crate::json::utf8_text(bytes)?, limits),
        DocumentFormat::Csv => DecisionTable::from_csv(crate::json::utf8_text(bytes)?, limits)
            .map(Document::DecisionTable),
    }
}

/// Write a document in `format`.
///
/// # Errors
///
/// `unsupported` for a document the format cannot hold: CSV holds decision
/// tables only, and text has no metadata, actions given as code or NaN
/// (`spec/text.md`); also for YAML in a build without the `yaml` feature.
pub fn write_document_as(document: &Document, format: DocumentFormat) -> Result<String> {
    match (format, document) {
        (DocumentFormat::Json, _) => Ok(crate::json::to_string(&write_document(document))),
        #[cfg(feature = "yaml")]
        (DocumentFormat::Yaml, _) => Ok(crate::yaml::to_string(&write_document(document))),
        #[cfg(not(feature = "yaml"))]
        (DocumentFormat::Yaml, _) => Err(no_yaml()),
        (DocumentFormat::Text, _) => crate::text::to_string(document),
        (DocumentFormat::Csv, Document::DecisionTable(table)) => Ok(table.to_csv()),
        (DocumentFormat::Csv, _) => Err(Error::new(
            ErrorCode::Unsupported,
            "only decision tables are written as CSV",
        )
        .with_hint("write rules and rule sets as JSON, YAML or text")
        .unsupported("csv", "CSV holds decision tables")),
    }
}

#[cfg(not(feature = "yaml"))]
fn no_yaml() -> Error {
    Error::new(
        ErrorCode::Unsupported,
        "this build of cleanrules has no YAML",
    )
    .with_hint("build the core with its `yaml` feature")
    .unsupported("yaml", "the `yaml` feature is off")
}

fn unsupported_as(what: &str, format: DocumentFormat, formats: &str) -> Error {
    Error::new(
        ErrorCode::Unsupported,
        format!("a {what} is not written in {}", format.as_str()),
    )
    .with_hint(format!("use {formats}"))
    .unsupported(
        format.as_str(),
        format!("a {what} has no {} form", format.as_str()),
    )
}

/// Parse a document's text in JSON or YAML into JSON values, for the documents
/// that only these formats hold (decisions).
///
/// # Errors
///
/// As [`crate::json::parse_bytes`] or [`crate::yaml::parse`]; `unsupported`
/// for the rule syntax and CSV, and for YAML without the `yaml` feature.
pub fn parse_as(bytes: &[u8], format: DocumentFormat, limits: &Limits) -> Result<Json> {
    if matches!(format, DocumentFormat::Json | DocumentFormat::Yaml) {
        check_size(bytes, format, limits)?;
    }
    match format {
        DocumentFormat::Json => crate::json::parse_bytes(bytes, limits),
        #[cfg(feature = "yaml")]
        DocumentFormat::Yaml => crate::yaml::parse(crate::json::utf8_text(bytes)?, limits),
        #[cfg(not(feature = "yaml"))]
        DocumentFormat::Yaml => Err(no_yaml()),
        DocumentFormat::Text | DocumentFormat::Csv => {
            Err(unsupported_as("decision", format, "JSON or YAML"))
        }
    }
}

/// Read a decision document in `format` (JSON or YAML) with the rule set that
/// produced it.
///
/// # Errors
///
/// As [`parse_as`], then as [`read_parsed_decision_document`].
pub fn read_decision_document_as<'a>(
    bytes: &[u8],
    format: DocumentFormat,
    ruleset: &'a RuleSet,
    limits: &Limits,
) -> Result<Decision<'a>> {
    read_parsed_decision_document(&parse_as(bytes, format, limits)?, ruleset, limits)
}

/// An output-only document, `{"version": 1, <key>: <content>}`.
fn output_document(key: &str, content: Json) -> Json {
    Json::Object(vec![
        (
            "version".into(),
            Json::Number(JsonNumber::from_literal(FORMAT_VERSION.to_string())),
        ),
        (key.into(), content),
    ])
}

fn write_output_as(
    key: &str,
    content: impl FnOnce() -> Json,
    text: Option<&dyn Fn() -> String>,
    format: DocumentFormat,
) -> Result<String> {
    match (format, text) {
        (DocumentFormat::Json, _) => Ok(crate::json::to_string(&output_document(key, content()))),
        #[cfg(feature = "yaml")]
        (DocumentFormat::Yaml, _) => Ok(crate::yaml::to_string(&output_document(key, content()))),
        #[cfg(not(feature = "yaml"))]
        (DocumentFormat::Yaml, _) => Err(no_yaml()),
        (DocumentFormat::Text, Some(text)) => Ok(text()),
        (format, Some(_)) => Err(unsupported_as(key, format, "JSON, YAML or text")),
        (format, None) => Err(unsupported_as(key, format, "JSON or YAML")),
    }
}

/// Write a decision document in JSON or YAML.
///
/// # Errors
///
/// `unsupported` for the rule syntax and CSV: a decision has no text form.
pub fn write_decision_as(decision: &Decision<'_>, format: DocumentFormat) -> Result<String> {
    write_output_as("decision", || write_decision(decision), None, format)
}

/// Write an explanation document in JSON or YAML, or its text
/// (`spec/explain.md`, *Text*) for [`DocumentFormat::Text`].
///
/// # Errors
///
/// `unsupported` for CSV.
pub fn write_explanation_as(
    explanation: &crate::explain::Explanation<'_, '_>,
    format: DocumentFormat,
) -> Result<String> {
    write_output_as(
        "explanation",
        || crate::format::write_explanation(explanation),
        Some(&|| explanation.to_string()),
        format,
    )
}

/// Write a summary document in JSON or YAML, or its text (`spec/explain.md`,
/// *Summaries*) for [`DocumentFormat::Text`].
///
/// # Errors
///
/// `unsupported` for CSV.
pub fn write_summary_as(
    summary: &crate::explain::Summary<'_>,
    format: DocumentFormat,
) -> Result<String> {
    write_output_as(
        "summary",
        || crate::format::write_summary(summary),
        Some(&|| summary.to_string()),
        format,
    )
}

/// Write an audit document in JSON or YAML, or its text (`spec/audit.md`,
/// *Text*) for [`DocumentFormat::Text`].
///
/// # Errors
///
/// `unsupported` for CSV.
pub fn write_audit_as(audit: &crate::audit::Audit, format: DocumentFormat) -> Result<String> {
    write_output_as(
        "audit",
        || crate::format::write_audit(audit),
        Some(&|| audit.to_string()),
        format,
    )
}

/// Write a test run document in JSON or YAML, or its text (`spec/tests.md`,
/// *Test runs*, *Text*) for [`DocumentFormat::Text`].
///
/// # Errors
///
/// `unsupported` for CSV.
pub fn write_test_run_as(run: &crate::cases::TestRun, format: DocumentFormat) -> Result<String> {
    write_output_as(
        "test_run",
        || crate::format::write_test_run(run),
        Some(&|| run.to_string()),
        format,
    )
}

/// Write a test comparison document in JSON or YAML, or its text
/// (`spec/comparison.md`, *Text*) for [`DocumentFormat::Text`].
///
/// # Errors
///
/// `unsupported` for CSV.
pub fn write_test_comparison_as(
    comparison: &crate::cases::TestComparison,
    format: DocumentFormat,
) -> Result<String> {
    write_output_as(
        "test_comparison",
        || crate::format::write_test_comparison(comparison),
        Some(&|| comparison.to_string()),
        format,
    )
}

#[cfg(test)]
mod tests {
    use super::{DocumentFormat, read_document_as, size_limit, too_large};
    use crate::error::ErrorCode;
    use crate::json::Limits;

    /// Size first, before the bytes are decoded as UTF-8, in every format
    /// (`spec/json-format.md`, *Errors and limits*).
    #[test]
    fn size_is_checked_before_utf8() {
        let limits = Limits::default().with_max_size(8);
        let bytes = b"a: \xff\xff\xff\xff\xff\xff\xff";
        for format in [
            DocumentFormat::Json,
            DocumentFormat::Yaml,
            DocumentFormat::Text,
            DocumentFormat::Csv,
        ] {
            let error = read_document_as(bytes, format, &limits).unwrap_err();
            assert_eq!(error.code(), ErrorCode::LimitExceeded, "{format:?}");
            assert!(
                error
                    .message()
                    .contains("10 bytes, more than the limit of 8")
            );
            // Within the limit, the bytes that are not UTF-8 (YAML without
            // its feature is unsupported).
            let error = read_document_as(&bytes[..8], format, &limits).unwrap_err();
            if cfg!(feature = "yaml") || format != DocumentFormat::Yaml {
                assert_eq!(error.code(), ErrorCode::FormatError, "{format:?}");
            }
        }
        let error = super::parse_as(bytes, DocumentFormat::Yaml, &limits).unwrap_err();
        assert_eq!(error.code(), ErrorCode::LimitExceeded);
    }

    #[test]
    fn size_limits_by_format() {
        let limits = Limits::default();
        assert_eq!(size_limit(DocumentFormat::Json, &limits), limits.max_size);
        assert_eq!(
            size_limit(DocumentFormat::Yaml, &limits),
            limits.max_yaml_size
        );
        assert_eq!(
            size_limit(DocumentFormat::Text, &limits),
            limits.max_text_size
        );
        assert_eq!(
            size_limit(DocumentFormat::Csv, &limits),
            limits.max_text_size
        );
        let error = too_large(DocumentFormat::Csv, None, &limits);
        assert_eq!(
            error.message(),
            "the text has more bytes than the limit of 4194304"
        );
        assert_eq!(
            error.hint(),
            Some("raise the size limit if the text is trusted")
        );
        let error = too_large(DocumentFormat::Json, Some(20_000_000), &limits);
        assert_eq!(
            error.message(),
            "the document has 20000000 bytes, more than the limit of 16777216"
        );
    }
}
