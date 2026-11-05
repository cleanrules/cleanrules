//! Documents for the idiomatic API (`lib/api/documents.js`): writing a
//! condition or a rule as a document, reading one rule from text, the parts of
//! a rule read from a document, and the size check of a file before it is
//! read. The core reads and writes every document (`spec/json-format.md`,
//! `spec/text.md`); JavaScript builds its objects from what this gives.

use cleanrules::ast::Node;
use cleanrules::format::{Document, DocumentFormat, write_document_as};
use cleanrules::json::{Json, to_string, utf8_text};
use cleanrules::rules::Rule;
use cleanrules::{Error, ErrorCode};

use super::{Outcome, Raw, RawLimits, limits, out};

/// The condition document of `node` in `format` (JSON when left out). The
/// JavaScript half refuses value nodes first, as Python does.
pub(crate) fn write_condition(node: &Node, format: &Raw) -> Outcome<String> {
    let format = super::format(format, DocumentFormat::Json)?;
    Ok(write_document_as(
        &Document::Condition(node.clone()),
        format,
    )?)
}

/// The rule document of `rule` in `format` (JSON when left out).
pub(crate) fn write_rule(rule: &Rule, format: &Raw) -> Outcome<String> {
    let format = super::format(format, DocumentFormat::Json)?;
    Ok(write_document_as(&Document::Rule(rule.clone()), format)?)
}

/// One rule written in the text syntax (`spec/text.md`, `rule_from_text`).
pub(crate) fn rule_text(text: &[u8], raw: &RawLimits) -> Outcome<Rule> {
    let limits = limits(raw)?;
    Ok(cleanrules::text::parse_rule(utf8_text(text)?, &limits)?)
}

/// What the idiomatic `Rule` keeps of a rule read from a document, as JSON:
/// `[name, priority, description, keys]`, the priority in `out`'s encoding
/// and `keys` the payload's keys, in order. Tags, metadata and the action's
/// name come from `build::rule_details`.
pub(crate) fn rule_parts(rule: &Rule) -> String {
    let keys = rule
        .then()
        .iter()
        .map(|(key, _)| Json::String(key.clone()))
        .collect();
    to_string(&Json::Array(vec![
        Json::String(rule.name().to_owned()),
        out::int(rule.get_priority()),
        rule.get_description()
            .map_or(Json::Null, |text| Json::String(text.to_owned())),
        Json::Array(keys),
    ]))
}

/// A rule's nodes: its condition, then its payload's values in order.
pub(crate) fn rule_nodes(rule: &Rule) -> Vec<Node> {
    std::iter::once(rule.when().clone())
        .chain(rule.then().iter().map(|(_, node)| node.clone()))
        .collect()
}

/// Check, before reading it, that a file of `size` bytes named `name` is
/// within the size limit of `format` (`spec/json-format.md`, *Errors and
/// limits*): `maxSize` for JSON, the YAML and text limits for YAML, text and
/// CSV, as the core applies them to the text.
pub(crate) fn size_check(name: &str, size: f64, format: &Raw, raw: &RawLimits) -> Outcome<()> {
    let limits = limits(raw)?;
    let limit = match super::format(format, DocumentFormat::Json)? {
        DocumentFormat::Json => limits.max_size,
        DocumentFormat::Yaml => limits.max_yaml_size,
        // Text and CSV (the core's formats are `#[non_exhaustive]`).
        _ => limits.max_text_size,
    };
    // Sizes beyond 2^53 bytes do not exist.
    #[allow(clippy::cast_precision_loss)]
    if size > limit as f64 {
        return Err(Error::new(
            ErrorCode::LimitExceeded,
            format!("{name} has {size} bytes, more than the limit of {limit}"),
        )
        .with_hint("raise limits.maxSize if the file is trusted")
        .into());
    }
    Ok(())
}
