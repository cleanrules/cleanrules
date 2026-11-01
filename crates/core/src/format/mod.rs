//! The JSON format of `spec/json-format.md`: reading and writing documents.

mod condition;
mod document;
pub mod value;

pub use condition::write_node;
pub use document::{Document, read_document, write_document};
pub use value::{ValueContext, read_value, write_value};
