//! cleanrules: an explainable rules engine.
//!
//! This crate is the single implementation of cleanrules's semantics. Every
//! language binding is a thin layer over it. The behaviour is defined by the
//! specification (`spec/` in the repository) and checked by the conformance
//! vectors (`conformance/`).
//!
//! The crate is being built: phase B of the roadmap adds the value model, the
//! condition AST, the evaluator, rules and rule sets, and explanations.

pub mod ast;
pub mod build;
mod error;
pub mod eval;
pub mod format;
pub mod json;
pub mod logic;
pub mod rules;
pub mod value;

pub use error::{Context, Error, ErrorCode, Result, Segment};
pub use value::{Decimal, Group, Map, NullCause, Number, Value};

/// The version of the JSON format this crate reads and writes
/// (`spec/json-format.md`).
pub const FORMAT_VERSION: u32 = 1;

#[cfg(test)]
mod tests {
    use super::FORMAT_VERSION;

    #[test]
    fn format_version_is_the_specified_one() {
        assert_eq!(FORMAT_VERSION, 1);
    }
}
