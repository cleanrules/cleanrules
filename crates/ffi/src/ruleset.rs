//! Rule sets (`spec/rules.md`): read from a document, written, summarized.

use std::ffi::c_char;
use std::sync::Arc;

use cleanrules::explain::summary;
use cleanrules::format::{Document, read_document_as, write_document_as, write_summary_as};
use cleanrules::rules::{Aggregate, HitPolicy, RuleSet};
use cleanrules::{Segment, Value};

use crate::boundary::{
    Failure, bytes, call, clear, clear_borrowed, format, free, give, give_text, handle, limits_of,
};
use crate::document::cr_document;
use crate::error::cr_error;
use crate::limits::cr_limits;
use crate::value::{cr_value, give_new_value};
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

/// The number of rules; 0 for `NULL`.
///
/// # Safety
///
/// `ruleset` is `NULL` or a live `cr_ruleset`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cr_ruleset_len(ruleset: *const cr_ruleset) -> usize {
    // SAFETY: `ruleset` is NULL or live (# Safety).
    unsafe { ruleset.as_ref() }.map_or(0, |r| r.ruleset.rules().len())
}

/// The name of the rule at position `rule`, borrowed from the rule set and not
/// NUL-terminated.
///
/// # Safety
///
/// `ruleset` is `NULL` or a live `cr_ruleset`; `name` and `len` are `NULL` or
/// point to a writable `const char *` and `size_t`; `error` is `NULL` or points
/// to a writable `cr_error *` set to `NULL`. The name lives as long as the rule
/// set.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cr_ruleset_rule_name(
    ruleset: *const cr_ruleset,
    rule: usize,
    name: *mut *const c_char,
    len: *mut usize,
    error: *mut *mut cr_error,
) -> cr_status {
    let body = || {
        // SAFETY: `name` and `len` are NULL or writable (# Safety).
        unsafe { clear_borrowed(name, len) };
        // SAFETY: `ruleset` is NULL or live (# Safety).
        let rules = unsafe { handle(ruleset, "ruleset") }?.ruleset.rules();
        let found = rules.get(rule).ok_or_else(|| {
            Failure::Invalid(
                format!("rule {rule} is out of the rule set of {}", rules.len()),
                "pass a position below cr_ruleset_len",
            )
        })?;
        // SAFETY: `name` and `len` are NULL or writable (# Safety).
        unsafe { crate::value::borrow_text(Some(found.name()), name, len) };
        Ok(())
    };
    // SAFETY: `error` is NULL or writable (# Safety).
    unsafe { call("cr_ruleset_rule_name", error, body) }
}

/// The rule set's hit policy (`first`, `priority`, `unique`, `any`,
/// `rule_order`, `collect`), a static string; `NULL` for `NULL`.
///
/// # Safety
///
/// `ruleset` is `NULL` or a live `cr_ruleset`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cr_ruleset_hit_policy(ruleset: *const cr_ruleset) -> *const c_char {
    // SAFETY: `ruleset` is NULL or live (# Safety).
    let Some(ruleset) = (unsafe { ruleset.as_ref() }) else {
        return std::ptr::null();
    };
    let name = match ruleset.ruleset.hit_policy() {
        HitPolicy::First => c"first",
        HitPolicy::Priority => c"priority",
        HitPolicy::Unique => c"unique",
        HitPolicy::Any => c"any",
        HitPolicy::RuleOrder => c"rule_order",
        HitPolicy::Collect => c"collect",
    };
    name.as_ptr()
}

/// The rule set's aggregate (`sum`, `min`, `max`, `count`), a static string;
/// `NULL` when it has none, and for `NULL`. With one, the single output of a
/// decision is the aggregate.
///
/// # Safety
///
/// `ruleset` is `NULL` or a live `cr_ruleset`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cr_ruleset_aggregate(ruleset: *const cr_ruleset) -> *const c_char {
    // SAFETY: `ruleset` is NULL or live (# Safety).
    let aggregate = unsafe { ruleset.as_ref() }.and_then(|r| r.ruleset.aggregate());
    let name = match aggregate {
        None => return std::ptr::null(),
        Some(Aggregate::Sum) => c"sum",
        Some(Aggregate::Min) => c"min",
        Some(Aggregate::Max) => c"max",
        Some(Aggregate::Count) => c"count",
    };
    name.as_ptr()
}

/// Whether the rule set is strict (`spec/rules.md`, *Strict mode and
/// errors*); false for `NULL`.
///
/// # Safety
///
/// `ruleset` is `NULL` or a live `cr_ruleset`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cr_ruleset_strict(ruleset: *const cr_ruleset) -> bool {
    // SAFETY: `ruleset` is NULL or live (# Safety).
    unsafe { ruleset.as_ref() }.is_some_and(|r| r.ruleset.strict())
}

/// The paths of the input that the rule set reads, its conditions' and
/// payloads', in the order of the summary (`spec/explain.md`, *Summaries*), as
/// a new list of paths, each a list of strings (keys) and ints (indexes), freed
/// with `cr_value_free`. A binding reads only these from native data
/// (`spec/data-model.md`, *Reading native data*).
///
/// # Safety
///
/// `ruleset` is `NULL` or a live `cr_ruleset`; `out` is `NULL` or points to a
/// writable `cr_value *`; `error` is `NULL` or points to a writable
/// `cr_error *` set to `NULL`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cr_ruleset_field_paths(
    ruleset: *const cr_ruleset,
    out: *mut *mut cr_value,
    error: *mut *mut cr_error,
) -> cr_status {
    let body = || {
        // SAFETY: `ruleset` is NULL or live (# Safety).
        let ruleset = &unsafe { handle(ruleset, "ruleset") }?.ruleset;
        let paths = summary(ruleset)
            .fields
            .iter()
            .map(|field| {
                Value::List(
                    field
                        .path
                        .iter()
                        .map(|segment| match segment {
                            Segment::Key(key) => Value::String(key.clone()),
                            // Indexes are below 2^63: they index lists in memory.
                            Segment::Index(index) => {
                                Value::Int(i64::try_from(*index).unwrap_or(i64::MAX))
                            }
                        })
                        .collect(),
                )
            })
            .collect();
        // SAFETY: `out` is NULL or writable (# Safety).
        unsafe { give_new_value(out, Value::List(paths)) }
    };
    // SAFETY: `error` is NULL or writable (# Safety).
    unsafe { call("cr_ruleset_field_paths", error, body) }
}
