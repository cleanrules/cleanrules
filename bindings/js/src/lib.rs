//! JavaScript binding of cleanrules: the engine of the `cleanrules` npm
//! package, built two ways from this one crate.
//!
//! - On every native target, a Node addon built with napi-rs (`napi`),
//!   shipped in one package per platform.
//! - On `wasm32`, a WebAssembly module built with wasm-bindgen (`wasm`), for
//!   browsers, Deno and any other JavaScript runtime.
//!
//! Both front ends are thin: they turn JavaScript arguments into the shared
//! code's values, call it (`common`), and turn its problems into JavaScript
//! errors, so the two builds offer the same API with the same checks. Neither
//! evaluates anything on its own (`CLAUDE.md`, invariant 2): the core does.
//! The binding has a JavaScript half (`lib/`), registered with each build when
//! it loads: the reader of native data (`lib/read.js`), which hands this layer
//! a compact encoding of the sparse value tree (`common::wire`); the error
//! classes (`lib/errors.js`), built from an error's JSON form; and the calls
//! to actions, so that an action that throws never leaves an exception pending
//! in native code.
//!
//! The idiomatic TypeScript API (rule builders, operators) arrives in phase
//! F.2, on top of these classes.

// napi-rs and wasm-bindgen hand arguments over by value, and napi-rs ties
// JavaScript values to the lifetime of `&'env Env`: these two lints would ask
// for signatures they cannot generate.
#![allow(clippy::needless_pass_by_value, clippy::trivially_copy_pass_by_ref)]

mod common;

#[cfg(not(target_arch = "wasm32"))]
mod napi;
#[cfg(target_arch = "wasm32")]
mod wasm;

#[cfg(not(target_arch = "wasm32"))]
pub use napi::classes::{
    ActionCall, Aggregate, Decision, Document, DocumentKind, Format, HitPolicy, Limits, Node,
    ReadOptions, RuleSet, TreeOptions, Truth, ValueTreeOptions,
};
#[cfg(not(target_arch = "wasm32"))]
pub use napi::hooks::set_hooks;
#[cfg(not(target_arch = "wasm32"))]
pub use napi::{FORMAT_VERSION, decimal_problem, version};

#[cfg(target_arch = "wasm32")]
pub use wasm::classes::{Decision, Document, Node, RuleSet};
#[cfg(target_arch = "wasm32")]
pub use wasm::{decimal_problem, format_version, not_a_document, set_hooks, version};
