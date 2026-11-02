//! The JSON format of `spec/json-format.md`: reading and writing documents.

mod condition;
mod decision;
mod document;
mod error;
mod explain;
pub mod result;
mod rules;
pub(crate) mod table;
pub mod value;

pub use condition::write_node;
pub use document::{
    Document, DocumentFormat, parse_as, read_decision_document, read_decision_document_as,
    read_document, read_document_as, read_parsed_decision_document, read_parsed_document,
    write_decision_as, write_document, write_document_as, write_explanation_as, write_summary_as,
};
#[cfg(feature = "yaml")]
pub use document::{read_yaml_decision_document, read_yaml_document};
pub use error::write_error;
pub use explain::{write_explanation, write_summary};
pub use result::{write_path, write_reason, write_result, write_truth};
pub use rules::{write_decision, write_rule, write_ruleset};
pub use table::write_table;
pub use value::{ValueContext, read_value, write_value};

use crate::ast::Node;
use crate::error::Result;
use crate::json::{Json, Limits};

/// Read a node (a condition or a value) from JSON already parsed, checking its
/// shape and then building it, as inside a document. `pointer` locates it in
/// errors.
///
/// # Errors
///
/// `format_error`, `limit_exceeded` or `expression_error`, as for documents.
pub fn read_node(json: &Json, pointer: &str, limits: &Limits) -> Result<Node> {
    condition::build(condition::shape(json, pointer, 1, limits)?)
}

/// Read a rule set's content (the object under `"ruleset"`) from JSON already
/// parsed, as inside a document. `pointer` locates it in errors.
///
/// # Errors
///
/// As [`read_document`] for a rule set document.
pub fn read_ruleset(json: &Json, pointer: &str, limits: &Limits) -> Result<crate::rules::RuleSet> {
    rules::build_ruleset(rules::shape_ruleset(json, pointer, limits)?)
}
