//! Node binding of cleanrules: the native addon of the `cleanrules` npm
//! package, built with napi-rs.
//!
//! A thin layer over the core (`CLAUDE.md`, invariant 2): it reads and writes
//! documents, converts data and translates errors, and never evaluates on its
//! own. The addon has a JavaScript half, in `lib/`, which the package entry
//! (`index.js`) registers with [`set_hooks`] when it loads:
//!
//! - the reader of native data (`lib/read.js`), which walks a JavaScript input
//!   along the paths a rule set reads (`spec/data-model.md`, *Reading native
//!   data*) and hands this layer a compact encoding of the sparse value tree
//!   (`wire.rs`);
//! - the error classes (`lib/errors.js`), built from an error's JSON form
//!   (`spec/errors.md`);
//! - the calls to actions, so that an action that throws never leaves an
//!   exception pending in this layer.
//!
//! The idiomatic TypeScript API (rule builders, operators) arrives in phase
//! F.2, on top of these classes.

// napi-rs hands arguments over by value, and ties JavaScript values to the
// lifetime of `&'env Env`: these two lints would ask for signatures it cannot
// generate.
#![allow(clippy::needless_pass_by_value, clippy::trivially_copy_pass_by_ref)]

mod decision;
mod document;
mod hooks;
mod limits;
mod node;
mod read;
mod ruleset;
mod wire;

use napi_derive::napi;

pub use decision::Decision;
pub use document::Document;
pub use hooks::set_hooks;
pub use limits::{Aggregate, DocumentKind, Format, HitPolicy, Limits, ReadOptions, Truth};
pub use node::{Node, TreeOptions, ValueTreeOptions};
pub use ruleset::{ActionCall, RuleSet};

/// The version of the JSON format this build reads and writes
/// (`spec/json-format.md`).
#[napi]
pub const FORMAT_VERSION: u32 = 1;

/// The version of the package.
#[napi]
#[must_use]
pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// Why `text` is not a decimal literal, or `null` when it is one; for the
/// `Decimal` marker of `lib/values.js`. A literal too large for 38 digits is a
/// decimal: as data it reads as null with reason `decimal_overflow`.
#[napi]
#[must_use]
pub fn decimal_problem(text: String) -> Option<String> {
    match cleanrules::Decimal::parse(&text) {
        Err(cleanrules::value::DecimalError::Malformed) => {
            Some("not a decimal literal, such as \"0.10\", \"-3\", \"1E+3\" or \"inf\"".to_owned())
        }
        _ => None,
    }
}

// The literal above is for the generated TypeScript declarations; it must be
// the core's.
const _: () = assert!(FORMAT_VERSION == cleanrules::FORMAT_VERSION);
