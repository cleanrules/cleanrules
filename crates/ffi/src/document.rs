//! Documents (`spec/json-format.md`, *Documents*): read and written in JSON,
//! YAML, the rule syntax or CSV.

use std::ffi::c_char;

use cleanrules::format::{Document, read_document_as, write_document_as};

use crate::boundary::{bytes, call, clear, format, free, give, give_text, handle, limits_of};
use crate::error::cr_error;
use crate::limits::cr_limits;
use crate::{cr_format, cr_status};

/// What a document holds, its content (`CR_DOCUMENT_*`).
pub type cr_document_content = i32;

/// A document holding a condition.
pub const CR_DOCUMENT_CONDITION: cr_document_content = 1;
/// A document holding a rule.
pub const CR_DOCUMENT_RULE: cr_document_content = 2;
/// A document holding a rule set.
pub const CR_DOCUMENT_RULESET: cr_document_content = 3;
/// A document holding a decision table.
pub const CR_DOCUMENT_DECISION_TABLE: cr_document_content = 4;

/// A document read in any format: a condition, a rule, a rule set or a decision
/// table.
pub struct cr_document {
    pub(crate) document: Document,
}

/// Read a document in `format` (`CR_FORMAT_*`): JSON, YAML or the rule syntax,
/// or CSV for a decision table. Bytes that are not UTF-8 are a
/// `CR_FORMAT_ERROR` at the first invalid byte.
///
/// # Safety
///
/// `text` is `NULL` or points to `len` readable bytes; `limits` is `NULL` or a
/// live `cr_limits`; `out` is `NULL` or points to a writable `cr_document *`;
/// `error` is `NULL` or points to a writable `cr_error *` set to `NULL`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cr_document_read(
    text: *const c_char,
    len: usize,
    format: cr_format,
    limits: *const cr_limits,
    out: *mut *mut cr_document,
    error: *mut *mut cr_error,
) -> cr_status {
    let body = || {
        // SAFETY: `out` is NULL or writable (# Safety).
        unsafe { clear(out, "out") }?;
        let format = self::format(format)?;
        // SAFETY: `text` and `len` (# Safety).
        let text = unsafe { bytes(text, len, "text") }?;
        // SAFETY: `limits` is NULL or live (# Safety).
        let limits = unsafe { limits_of(limits) };
        let document = read_document_as(text, format, &limits)?;
        // SAFETY: `out` passed `clear`.
        unsafe { give(out, cr_document { document }) };
        Ok(())
    };
    // SAFETY: `error` is NULL or writable (# Safety).
    unsafe { call("cr_document_read", error, body) }
}

/// What a document holds, its content (`CR_DOCUMENT_*`). 0 for `NULL`.
///
/// # Safety
///
/// `document` is `NULL` or a live `cr_document`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cr_document_kind(document: *const cr_document) -> cr_document_content {
    // SAFETY: the caller promises a live handle when it is not null.
    match unsafe { document.as_ref() }.map(|d| &d.document) {
        Some(Document::Condition(_)) => CR_DOCUMENT_CONDITION,
        Some(Document::Rule(_)) => CR_DOCUMENT_RULE,
        Some(Document::RuleSet(_)) => CR_DOCUMENT_RULESET,
        Some(Document::DecisionTable(_)) => CR_DOCUMENT_DECISION_TABLE,
        _ => 0,
    }
}

/// Write a document in `format` (`CR_FORMAT_*`), in canonical form. CSV holds
/// decision tables only; the rule syntax holds no decision tables, metadata,
/// actions given as code or NaN. Anything else is `CR_UNSUPPORTED`.
///
/// # Safety
///
/// `document` is `NULL` or a live `cr_document`; `out` is `NULL` or points to a
/// writable `char *`; `len` is `NULL` or points to a writable `size_t`;
/// `error` is `NULL` or points to a writable `cr_error *` set to `NULL`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cr_document_write(
    document: *const cr_document,
    format: cr_format,
    out: *mut *mut c_char,
    len: *mut usize,
    error: *mut *mut cr_error,
) -> cr_status {
    let body = || {
        // SAFETY: `out` is NULL or writable (# Safety).
        unsafe { clear(out, "out") }?;
        // SAFETY: `document` is NULL or live (# Safety).
        let document = &unsafe { handle(document, "document") }?.document;
        let text = write_document_as(document, self::format(format)?)?;
        // SAFETY: `out` passed `clear`; `len` is NULL or writable (# Safety).
        unsafe { give_text(out, len, text) }
    };
    // SAFETY: `error` is NULL or writable (# Safety).
    unsafe { call("cr_document_write", error, body) }
}

/// Free a document. `NULL` is ignored.
///
/// # Safety
///
/// `document` is `NULL` or a `cr_document` from this library, not freed before.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cr_document_free(document: *mut cr_document) {
    // SAFETY: the caller's promise is `free`'s.
    unsafe { free(document) };
}
