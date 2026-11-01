//! The JSON format of `spec/json-format.md`: reading and writing documents.

mod condition;
mod document;
pub mod result;
pub mod value;

pub use condition::write_node;
pub use document::{Document, read_document, write_document};
pub use result::{write_path, write_reason, write_result, write_truth};
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
