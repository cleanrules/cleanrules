//! Documents (`spec/json-format.md`, *Documents*): read and written in JSON,
//! YAML, the rule syntax or CSV; made of a condition or a rule, and what they
//! hold.

use std::ffi::c_char;

use cleanrules::format::{Document, read_document_as, write_document_as};

use crate::boundary::{
    Failure, Outcome, bytes, call, clear, format, free, give, give_text, handle, limits_of,
};
use crate::error::cr_error;
use crate::limits::cr_limits;
use crate::node::cr_node;
use crate::rule::cr_rule;
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
    unsafe { call(error, body) }
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
    unsafe { call(error, body) }
}

/// Hand a new document out.
///
/// # Safety
///
/// `out` is `NULL` or points to a writable `cr_document *`; `error` is `NULL`
/// or points to a writable `cr_error *` set to `NULL`.
unsafe fn make(
    out: *mut *mut cr_document,
    error: *mut *mut cr_error,
    content: impl FnOnce() -> Outcome<Document>,
) -> cr_status {
    let body = || {
        // SAFETY: `out` is NULL or writable (# Safety).
        unsafe { clear(out, "out") }?;
        let document = content()?;
        // SAFETY: `out` passed `clear`.
        unsafe { give(out, cr_document { document }) };
        Ok(())
    };
    // SAFETY: `error` is NULL or writable (# Safety).
    unsafe { call(error, body) }
}

/// A document holding a condition (copied), to write it with
/// `cr_document_write`. A value node, such as `a + 1`, or the null constant is
/// a `CR_EXPRESSION_ERROR`: only a condition can be a document.
///
/// # Safety
///
/// `node` is `NULL` or a live `cr_node`; `out` is `NULL` or points to a
/// writable `cr_document *`; `error` is `NULL` or points to a writable
/// `cr_error *` set to `NULL`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cr_document_new_condition(
    node: *const cr_node,
    out: *mut *mut cr_document,
    error: *mut *mut cr_error,
) -> cr_status {
    let content = || {
        // SAFETY: `node` is NULL or live (# Safety).
        let node = &unsafe { handle(node, "node") }?.node;
        Ok(Document::condition(node.clone())?)
    };
    // SAFETY: `out` and `error` (# Safety).
    unsafe { make(out, error, content) }
}

/// A document holding a rule (copied), to write it with `cr_document_write`.
///
/// # Safety
///
/// `rule` is `NULL` or a live `cr_rule`; `out` is `NULL` or points to a
/// writable `cr_document *`; `error` is `NULL` or points to a writable
/// `cr_error *` set to `NULL`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cr_document_new_rule(
    rule: *const cr_rule,
    out: *mut *mut cr_document,
    error: *mut *mut cr_error,
) -> cr_status {
    let content = || {
        // SAFETY: `rule` is NULL or live (# Safety).
        let rule = unsafe { handle(rule, "rule") }?;
        Ok(Document::Rule(rule.rule.clone()))
    };
    // SAFETY: `out` and `error` (# Safety).
    unsafe { make(out, error, content) }
}

/// A document of another kind than the function reads.
pub(crate) fn not_holding(what: &str, document: &Document) -> Failure {
    let held = match document {
        Document::Condition(_) => "a condition",
        Document::Rule(_) => "a rule",
        Document::RuleSet(_) => "a rule set",
        Document::DecisionTable(_) => "a decision table",
        _ => "something else",
    };
    Failure::Invalid(
        format!("the document holds {held}, not {what}"),
        "check what it holds with cr_document_kind first",
    )
}

/// A copy of the condition a `CR_DOCUMENT_CONDITION` document holds, freed
/// with `cr_node_free`. A document of another kind is `CR_INVALID_ARGUMENT`.
///
/// # Safety
///
/// `document` is `NULL` or a live `cr_document`; `out` is `NULL` or points to a
/// writable `cr_node *`; `error` is `NULL` or points to a writable
/// `cr_error *` set to `NULL`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cr_document_condition(
    document: *const cr_document,
    out: *mut *mut cr_node,
    error: *mut *mut cr_error,
) -> cr_status {
    let make = || {
        // SAFETY: `document` is NULL or live (# Safety).
        match &unsafe { handle(document, "document") }?.document {
            Document::Condition(node) => Ok(node.clone()),
            other => Err(not_holding("a condition", other)),
        }
    };
    // SAFETY: `out` and `error` (# Safety).
    unsafe { crate::construct::build(out, error, make) }
}

/// A copy of the rule a `CR_DOCUMENT_RULE` document holds, freed with
/// `cr_rule_free`. A document of another kind is `CR_INVALID_ARGUMENT`.
///
/// # Safety
///
/// `document` is `NULL` or a live `cr_document`; `out` is `NULL` or points to a
/// writable `cr_rule *`; `error` is `NULL` or points to a writable
/// `cr_error *` set to `NULL`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cr_document_rule(
    document: *const cr_document,
    out: *mut *mut cr_rule,
    error: *mut *mut cr_error,
) -> cr_status {
    let body = || {
        // SAFETY: `out` is NULL or writable (# Safety).
        unsafe { clear(out, "out") }?;
        // SAFETY: `document` is NULL or live (# Safety).
        let rule = match &unsafe { handle(document, "document") }?.document {
            Document::Rule(rule) => rule.clone(),
            other => return Err(not_holding("a rule", other)),
        };
        // SAFETY: `out` passed `clear`.
        unsafe { give(out, cr_rule { rule }) };
        Ok(())
    };
    // SAFETY: `error` is NULL or writable (# Safety).
    unsafe { call(error, body) }
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
