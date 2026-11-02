//! Decisions (`spec/rules.md`, `spec/json-format.md`, *Decisions*): evaluating
//! an input, reading a recorded decision, and what a decision says.

use std::ffi::c_char;
use std::sync::Arc;

use cleanrules::explain::explain;
use cleanrules::format::{
    ValueContext, parse_as, read_parsed_decision_document, read_value, write_decision_as,
    write_explanation_as, write_value,
};
use cleanrules::json::{Json, Limits, parse_bytes, to_string};
use cleanrules::rules::{Decision, RuleSet};
use cleanrules::{Map, Value};

use crate::boundary::{
    Outcome, bytes, call, clear, format, free, give, give_text, handle, limits_of,
};
use crate::error::cr_error;
use crate::limits::cr_limits;
use crate::ruleset::cr_ruleset;
use crate::{cr_format, cr_status};

/// Where a decision comes from: it is made again from this whenever it is read,
/// which gives the same decision (CLAUDE.md, invariant 7).
enum Source {
    /// An input the rule set evaluated.
    Input(Value),
    /// A recorded decision the rule set accepted.
    Recorded(Json),
}

/// A decision: what a rule set made of one input, or a recorded decision read
/// with its rule set. Each function that reads it evaluates the input again,
/// or checks the recorded decision again.
pub struct cr_decision {
    ruleset: Arc<RuleSet>,
    source: Source,
    limits: Limits,
}

impl cr_decision {
    /// The decision, made again; it succeeded before, so it succeeds now.
    fn with<T>(&self, read: impl FnOnce(&Decision<'_>) -> Outcome<T>) -> Outcome<T> {
        let decision = match &self.source {
            Source::Input(input) => self.ruleset.evaluate(input)?,
            Source::Recorded(json) => {
                read_parsed_decision_document(json, &self.ruleset, &self.limits)?
            }
        };
        read(&decision)
    }
}

/// Evaluate the rule set on an input: a data value as JSON
/// (`spec/json-format.md`, *Values*), such as `{"customer": {"years": 7}}`. In
/// strict mode, a field the input lacks is a `CR_MISSING_FIELD`, and so on
/// (`spec/rules.md`).
///
/// # Safety
///
/// `ruleset` is `NULL` or a live `cr_ruleset`; `input` is `NULL` or points to
/// `len` readable bytes; `limits` is `NULL` or a live `cr_limits`; `out` is
/// `NULL` or points to a writable `cr_decision *`; `error` is `NULL` or points
/// to a writable `cr_error *` set to `NULL`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cr_ruleset_evaluate(
    ruleset: *const cr_ruleset,
    input: *const c_char,
    len: usize,
    limits: *const cr_limits,
    out: *mut *mut cr_decision,
    error: *mut *mut cr_error,
) -> cr_status {
    let body = || {
        // SAFETY: `out` is NULL or writable (# Safety).
        unsafe { clear(out, "out") }?;
        // SAFETY: `ruleset` is NULL or live (# Safety).
        let ruleset = Arc::clone(&unsafe { handle(ruleset, "ruleset") }?.ruleset);
        // SAFETY: `limits` is NULL or live (# Safety).
        let limits = unsafe { limits_of(limits) };
        // SAFETY: `input` and `len` (# Safety).
        let input = unsafe { bytes(input, len, "input") }?;
        let input = read_value(&parse_bytes(input, &limits)?, "", ValueContext::Data)?;
        // Evaluated once here, so that its errors are this call's.
        ruleset.evaluate(&input)?;
        let decision = cr_decision {
            ruleset,
            source: Source::Input(input),
            limits,
        };
        // SAFETY: `out` passed `clear`.
        unsafe { give(out, decision) };
        Ok(())
    };
    // SAFETY: `error` is NULL or writable (# Safety).
    unsafe { call("cr_ruleset_evaluate", error, body) }
}

/// Read a recorded decision, in JSON or YAML (`CR_FORMAT_*`), with the rule set
/// that made it (`spec/json-format.md`, *Reading a decision*): one that does
/// not belong to it is a `CR_DECISION_MISMATCH`. The rule syntax and CSV hold
/// no decisions: `CR_UNSUPPORTED`.
///
/// # Safety
///
/// `ruleset` is `NULL` or a live `cr_ruleset`; `text` is `NULL` or points to
/// `len` readable bytes; `limits` is `NULL` or a live `cr_limits`; `out` is
/// `NULL` or points to a writable `cr_decision *`; `error` is `NULL` or points
/// to a writable `cr_error *` set to `NULL`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cr_ruleset_read_decision(
    ruleset: *const cr_ruleset,
    text: *const c_char,
    len: usize,
    format: cr_format,
    limits: *const cr_limits,
    out: *mut *mut cr_decision,
    error: *mut *mut cr_error,
) -> cr_status {
    let body = || {
        // SAFETY: `out` is NULL or writable (# Safety).
        unsafe { clear(out, "out") }?;
        // SAFETY: `ruleset` is NULL or live (# Safety).
        let ruleset = Arc::clone(&unsafe { handle(ruleset, "ruleset") }?.ruleset);
        // SAFETY: `limits` is NULL or live (# Safety).
        let limits = unsafe { limits_of(limits) };
        let format = self::format(format)?;
        // SAFETY: `text` and `len` (# Safety).
        let text = unsafe { bytes(text, len, "text") }?;
        let json = parse_as(text, format, &limits)?;
        read_parsed_decision_document(&json, &ruleset, &limits)?;
        let decision = cr_decision {
            ruleset,
            source: Source::Recorded(json),
            limits,
        };
        // SAFETY: `out` passed `clear`.
        unsafe { give(out, decision) };
        Ok(())
    };
    // SAFETY: `error` is NULL or writable (# Safety).
    unsafe { call("cr_ruleset_read_decision", error, body) }
}

/// Write a decision as a JSON or YAML document (`CR_FORMAT_*`). A decision has
/// no text form: `CR_UNSUPPORTED`.
///
/// # Safety
///
/// `decision` is `NULL` or a live `cr_decision`; `out` is `NULL` or points to a
/// writable `char *`; `len` is `NULL` or points to a writable `size_t`;
/// `error` is `NULL` or points to a writable `cr_error *` set to `NULL`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cr_decision_write(
    decision: *const cr_decision,
    format: cr_format,
    out: *mut *mut c_char,
    len: *mut usize,
    error: *mut *mut cr_error,
) -> cr_status {
    let body = || {
        // SAFETY: `out` is NULL or writable (# Safety).
        unsafe { clear(out, "out") }?;
        // SAFETY: `decision` is NULL or live (# Safety).
        let decision = unsafe { handle(decision, "decision") }?;
        let format = self::format(format)?;
        let text = decision.with(|d| Ok(write_decision_as(d, format)?))?;
        // SAFETY: `out` passed `clear`; `len` is NULL or writable (# Safety).
        unsafe { give_text(out, len, text) }
    };
    // SAFETY: `error` is NULL or writable (# Safety).
    unsafe { call("cr_decision_write", error, body) }
}

/// The decision's single output (`spec/rules.md`, *Outputs*), as a JSON data
/// value: the winner's payload as an object, the aggregate, or `null` when no
/// rule was selected. Policies with several outputs (`rule_order`, `collect`
/// without an aggregate) give `CR_HIT_POLICY_ERROR`: use
/// `cr_decision_outputs`.
///
/// # Safety
///
/// `decision` is `NULL` or a live `cr_decision`; `out` is `NULL` or points to a
/// writable `char *`; `len` is `NULL` or points to a writable `size_t`;
/// `error` is `NULL` or points to a writable `cr_error *` set to `NULL`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cr_decision_output(
    decision: *const cr_decision,
    out: *mut *mut c_char,
    len: *mut usize,
    error: *mut *mut cr_error,
) -> cr_status {
    let body = || {
        // SAFETY: `out` is NULL or writable (# Safety).
        unsafe { clear(out, "out") }?;
        // SAFETY: `decision` is NULL or live (# Safety).
        let decision = unsafe { handle(decision, "decision") }?;
        let output = decision.with(|d| Ok(d.output()?))?;
        let json = output.map_or(Json::Null, |value| write_value(&value));
        // SAFETY: `out` passed `clear`; `len` is NULL or writable (# Safety).
        unsafe { give_text(out, len, to_string(&json)) }
    };
    // SAFETY: `error` is NULL or writable (# Safety).
    unsafe { call("cr_decision_output", error, body) }
}

/// The outputs of the selected rules, in rule order, as a JSON array of data
/// values (objects).
///
/// # Safety
///
/// `decision` is `NULL` or a live `cr_decision`; `out` is `NULL` or points to a
/// writable `char *`; `len` is `NULL` or points to a writable `size_t`;
/// `error` is `NULL` or points to a writable `cr_error *` set to `NULL`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cr_decision_outputs(
    decision: *const cr_decision,
    out: *mut *mut c_char,
    len: *mut usize,
    error: *mut *mut cr_error,
) -> cr_status {
    let body = || {
        // SAFETY: `out` is NULL or writable (# Safety).
        unsafe { clear(out, "out") }?;
        // SAFETY: `decision` is NULL or live (# Safety).
        let decision = unsafe { handle(decision, "decision") }?;
        let outputs = decision.with(|d| Ok(d.outputs()))?;
        let json = outputs
            .into_iter()
            .map(|pairs| {
                let mut map = Map::new();
                for (key, value) in pairs {
                    map.insert(key, value);
                }
                write_value(&Value::Map(map))
            })
            .collect();
        // SAFETY: `out` passed `clear`; `len` is NULL or writable (# Safety).
        unsafe { give_text(out, len, to_string(&Json::Array(json))) }
    };
    // SAFETY: `error` is NULL or writable (# Safety).
    unsafe { call("cr_decision_outputs", error, body) }
}

/// The decision's explanation (`spec/explain.md`) as a JSON or YAML document,
/// or as text (`CR_FORMAT_TEXT`).
///
/// # Safety
///
/// `decision` is `NULL` or a live `cr_decision`; `out` is `NULL` or points to a
/// writable `char *`; `len` is `NULL` or points to a writable `size_t`;
/// `error` is `NULL` or points to a writable `cr_error *` set to `NULL`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cr_decision_explain(
    decision: *const cr_decision,
    format: cr_format,
    out: *mut *mut c_char,
    len: *mut usize,
    error: *mut *mut cr_error,
) -> cr_status {
    let body = || {
        // SAFETY: `out` is NULL or writable (# Safety).
        unsafe { clear(out, "out") }?;
        // SAFETY: `decision` is NULL or live (# Safety).
        let decision = unsafe { handle(decision, "decision") }?;
        let format = self::format(format)?;
        let text = decision.with(|d| Ok(write_explanation_as(&explain(d), format)?))?;
        // SAFETY: `out` passed `clear`; `len` is NULL or writable (# Safety).
        unsafe { give_text(out, len, text) }
    };
    // SAFETY: `error` is NULL or writable (# Safety).
    unsafe { call("cr_decision_explain", error, body) }
}

/// Free a decision. `NULL` is ignored.
///
/// # Safety
///
/// `decision` is `NULL` or a `cr_decision` from this library, not freed before.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cr_decision_free(decision: *mut cr_decision) {
    // SAFETY: the caller's promise is `free`'s.
    unsafe { free(decision) };
}
