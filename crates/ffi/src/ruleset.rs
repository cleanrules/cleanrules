//! Rule sets (`spec/rules.md`): read from a document, written, summarized.

use std::ffi::c_char;
use std::sync::Arc;

use cleanrules::explain::summary;
use cleanrules::format::{Document, read_document_as, write_document_as, write_summary_as};
use cleanrules::rules::RuleSet;

use crate::boundary::{bytes, call, clear, format, free, give, give_text, handle, limits_of};
use crate::document::cr_document;
use crate::error::cr_error;
use crate::limits::cr_limits;
use crate::{cr_format, cr_status};

/// A rule set, ready to evaluate.
pub struct cr_ruleset {
    pub(crate) ruleset: Arc<RuleSet>,
}

/// The rule set a document gives (`spec/json-format.md`, *Documents*): its rule
/// set, the one its decision table compiles to, or a rule set of its one rule
/// with the default settings. A condition is a `CR_FORMAT_ERROR`.
///
/// # Safety
///
/// `document` is `NULL` or a live `cr_document`; `out` is `NULL` or points to a
/// writable `cr_ruleset *`; `error` is `NULL` or points to a writable
/// `cr_error *` set to `NULL`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cr_ruleset_from_document(
    document: *const cr_document,
    out: *mut *mut cr_ruleset,
    error: *mut *mut cr_error,
) -> cr_status {
    let body = || {
        // SAFETY: `out` is NULL or writable (# Safety).
        unsafe { clear(out, "out") }?;
        // SAFETY: `document` is NULL or live (# Safety).
        let document = &unsafe { handle(document, "document") }?.document;
        let ruleset = document.clone().into_ruleset()?;
        // SAFETY: `out` passed `clear`.
        unsafe {
            give(
                out,
                cr_ruleset {
                    ruleset: Arc::new(ruleset),
                },
            );
        };
        Ok(())
    };
    // SAFETY: `error` is NULL or writable (# Safety).
    unsafe { call("cr_ruleset_from_document", error, body) }
}

/// Read a rule set in `format` (`CR_FORMAT_*`): a rule set, a decision table,
/// which is compiled, or one rule (`cr_ruleset_from_document`).
///
/// # Safety
///
/// `text` is `NULL` or points to `len` readable bytes; `limits` is `NULL` or a
/// live `cr_limits`; `out` is `NULL` or points to a writable `cr_ruleset *`;
/// `error` is `NULL` or points to a writable `cr_error *` set to `NULL`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cr_ruleset_read(
    text: *const c_char,
    len: usize,
    format: cr_format,
    limits: *const cr_limits,
    out: *mut *mut cr_ruleset,
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
        let ruleset = read_document_as(text, format, &limits)?.into_ruleset()?;
        // SAFETY: `out` passed `clear`.
        unsafe {
            give(
                out,
                cr_ruleset {
                    ruleset: Arc::new(ruleset),
                },
            );
        };
        Ok(())
    };
    // SAFETY: `error` is NULL or writable (# Safety).
    unsafe { call("cr_ruleset_read", error, body) }
}

/// Write a rule set in `format` (`CR_FORMAT_*`): JSON, YAML or the rule syntax.
///
/// # Safety
///
/// `ruleset` is `NULL` or a live `cr_ruleset`; `out` is `NULL` or points to a
/// writable `char *`; `len` is `NULL` or points to a writable `size_t`;
/// `error` is `NULL` or points to a writable `cr_error *` set to `NULL`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cr_ruleset_write(
    ruleset: *const cr_ruleset,
    format: cr_format,
    out: *mut *mut c_char,
    len: *mut usize,
    error: *mut *mut cr_error,
) -> cr_status {
    let body = || {
        // SAFETY: `out` is NULL or writable (# Safety).
        unsafe { clear(out, "out") }?;
        // SAFETY: `ruleset` is NULL or live (# Safety).
        let ruleset = &unsafe { handle(ruleset, "ruleset") }?.ruleset;
        let document = Document::RuleSet(RuleSet::clone(ruleset));
        let text = write_document_as(&document, self::format(format)?)?;
        // SAFETY: `out` passed `clear`; `len` is NULL or writable (# Safety).
        unsafe { give_text(out, len, text) }
    };
    // SAFETY: `error` is NULL or writable (# Safety).
    unsafe { call("cr_ruleset_write", error, body) }
}

/// The rule set's summary (`spec/explain.md`, *Summaries*) as a JSON or YAML
/// document, or as text (`CR_FORMAT_TEXT`).
///
/// # Safety
///
/// `ruleset` is `NULL` or a live `cr_ruleset`; `out` is `NULL` or points to a
/// writable `char *`; `len` is `NULL` or points to a writable `size_t`;
/// `error` is `NULL` or points to a writable `cr_error *` set to `NULL`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cr_ruleset_summary(
    ruleset: *const cr_ruleset,
    format: cr_format,
    out: *mut *mut c_char,
    len: *mut usize,
    error: *mut *mut cr_error,
) -> cr_status {
    let body = || {
        // SAFETY: `out` is NULL or writable (# Safety).
        unsafe { clear(out, "out") }?;
        // SAFETY: `ruleset` is NULL or live (# Safety).
        let ruleset = &unsafe { handle(ruleset, "ruleset") }?.ruleset;
        let text = write_summary_as(&summary(ruleset), self::format(format)?)?;
        // SAFETY: `out` passed `clear`; `len` is NULL or writable (# Safety).
        unsafe { give_text(out, len, text) }
    };
    // SAFETY: `error` is NULL or writable (# Safety).
    unsafe { call("cr_ruleset_summary", error, body) }
}

/// Free a rule set. `NULL` is ignored. Its decisions stay valid.
///
/// # Safety
///
/// `ruleset` is `NULL` or a `cr_ruleset` from this library, not freed before.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cr_ruleset_free(ruleset: *mut cr_ruleset) {
    // SAFETY: the caller's promise is `free`'s.
    unsafe { free(ruleset) };
}
