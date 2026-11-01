//! The JSON format of `spec/json-format.md`: reading and writing documents.

mod condition;
mod decision;
mod document;
mod explain;
pub mod result;
mod rules;
pub mod value;

pub use condition::write_node;
pub use document::{Document, read_decision_document, read_document, write_document};
pub use explain::{write_explanation, write_summary};
pub use result::{write_path, write_reason, write_result, write_truth};
pub use rules::{write_decision, write_rule, write_ruleset};
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
