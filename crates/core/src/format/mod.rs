//! The JSON format of `spec/json-format.md`: reading and writing documents.

pub mod value;

pub use value::{ValueContext, read_value, write_value};
