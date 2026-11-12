//! Rule sets (`spec/rules.md`): read from a document, written, summarized.

use std::ffi::{CStr, c_char};
use std::sync::Arc;

use cleanrules::explain::summary;
use cleanrules::format::{Document, read_document_as, write_document_as, write_summary_as};
use cleanrules::logic::LogicName;
use cleanrules::rules::{Aggregate, Config, HitPolicy, RuleSet};
use cleanrules::{Segment, Value};

use crate::boundary::{
    Failure, Outcome, bytes, call, clear, clear_borrowed, format, free, give, give_text, handle,
    limits_of, utf8_arg, write_out,
};
use crate::document::cr_document;
use crate::error::cr_error;
use crate::limits::cr_limits;
use crate::rule::cr_rule;
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
    unsafe { call(error, body) }
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
    unsafe { call(error, body) }
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
    unsafe { call(error, body) }
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
    unsafe { call(error, body) }
}

/// A name argument that may be left out (`NULL`).
///
/// # Safety
///
/// `name` is `NULL` or points to `len` readable bytes.
unsafe fn optional_name<'a>(
    name: *const c_char,
    len: usize,
    what: &str,
) -> Outcome<Option<&'a str>> {
    if name.is_null() {
        return Ok(None);
    }
    // SAFETY: the caller's promise is `bytes`'.
    utf8_arg(unsafe { bytes(name, len, what) }?, what).map(Some)
}

/// A rule set of `len` rules, in order (copied; they stay the caller's), with
/// a hit policy, an aggregate and a logic by name, as in JSON
/// (`spec/rules.md`; the core's `Config::from_names`): `NULL` leaves each out,
/// which is `first`, no aggregate and `boolean`. An unknown hit policy or aggregate, a repeated rule name or
/// an aggregate without `collect` is a `CR_RULE_DEFINITION_ERROR`; an unknown
/// logic, a `CR_REGISTRY_ERROR`.
///
/// # Safety
///
/// `rules` is `NULL` or points to `len` readable pointers, each `NULL` or a
/// live `cr_rule`; `hit_policy`, `aggregate` and `logic` are `NULL` or point
/// to as many readable bytes as their lengths say; `out` is `NULL` or points
/// to a writable `cr_ruleset *`; `error` is `NULL` or points to a writable
/// `cr_error *` set to `NULL`.
#[unsafe(no_mangle)]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn cr_ruleset_new(
    rules: *const *const cr_rule,
    len: usize,
    hit_policy: *const c_char,
    hit_policy_len: usize,
    aggregate: *const c_char,
    aggregate_len: usize,
    logic: *const c_char,
    logic_len: usize,
    strict: bool,
    out: *mut *mut cr_ruleset,
    error: *mut *mut cr_error,
) -> cr_status {
    let body = || {
        // SAFETY: `out` is NULL or writable (# Safety).
        unsafe { clear(out, "out") }?;
        let pointers: &[*const cr_rule] = if rules.is_null() {
            if len != 0 {
                return Err(Failure::Invalid(
                    format!("rules is NULL with a length of {len}"),
                    "pass an array of rules, or a length of 0",
                ));
            }
            &[]
        } else {
            // SAFETY: the caller promises `len` readable pointers at `rules`.
            unsafe { std::slice::from_raw_parts(rules, len) }
        };
        let mut list = Vec::with_capacity(len);
        for &rule in pointers {
            // SAFETY: each pointer is NULL or a live rule (# Safety).
            list.push(unsafe { handle(rule, "a rule") }?.rule.clone());
        }
        // SAFETY: the names and their lengths (# Safety).
        let (policy, aggregate, logic) = unsafe {
            (
                optional_name(hit_policy, hit_policy_len, "hit_policy")?,
                optional_name(aggregate, aggregate_len, "aggregate")?,
                optional_name(logic, logic_len, "logic")?,
            )
        };
        let config = Config::from_names(policy, aggregate, logic, strict)?;
        let ruleset = RuleSet::new(list, config)?;
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
    unsafe { call(error, body) }
}

/// The position of the rule named `name`, for a lookup by name: a name the
/// rule set lacks is a `CR_RULE_NOT_FOUND` with the name and the rule names
/// (`available`, sorted).
///
/// # Safety
///
/// `ruleset` is `NULL` or a live `cr_ruleset`; `name` is `NULL` or points to
/// `len` readable bytes; `out` is `NULL` or points to a writable `size_t`;
/// `error` is `NULL` or points to a writable `cr_error *` set to `NULL`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cr_ruleset_rule_position(
    ruleset: *const cr_ruleset,
    name: *const c_char,
    len: usize,
    out: *mut usize,
    error: *mut *mut cr_error,
) -> cr_status {
    let body = || {
        // SAFETY: `ruleset` is NULL or live (# Safety).
        let ruleset = &unsafe { handle(ruleset, "ruleset") }?.ruleset;
        // SAFETY: `name` and `len` (# Safety).
        let name = utf8_arg(unsafe { bytes(name, len, "name") }?, "name")?;
        let position = ruleset.rule_position(name)?;
        // SAFETY: `out` is NULL or writable (# Safety).
        unsafe { write_out(out, position, "out") }
    };
    // SAFETY: `error` is NULL or writable (# Safety).
    unsafe { call(error, body) }
}

/// The rule set's summary, as `cr_ruleset_summary` gives it, with the rules
/// whose action is code of the program marked (`spec/explain.md`,
/// *Summaries*): `actions` holds one flag per rule, in rule order, or none
/// (`actions_len` 0); a rule that names its action is marked anyway. Another
/// number of flags, or `NULL` with flags, is `CR_INVALID_ARGUMENT`.
///
/// # Safety
///
/// `ruleset` is `NULL` or a live `cr_ruleset`; `actions` is `NULL` or points
/// to `actions_len` readable `bool`s; `out` is `NULL` or points to a writable
/// `char *`; `len` is `NULL` or points to a writable `size_t`; `error` is
/// `NULL` or points to a writable `cr_error *` set to `NULL`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cr_ruleset_summary_actions(
    ruleset: *const cr_ruleset,
    actions: *const bool,
    actions_len: usize,
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
        let count = ruleset.rules().len();
        if actions_len != 0 && actions_len != count {
            return Err(Failure::Invalid(
                format!("actions_len is {actions_len}, and the rule set has {count} rules"),
                "pass one flag per rule, or none",
            ));
        }
        let flags: &[bool] = if actions_len == 0 {
            &[]
        } else if actions.is_null() {
            return Err(Failure::Invalid(
                format!("actions is NULL with a length of {actions_len}"),
                "pass the flags, or a length of 0",
            ));
        } else {
            // SAFETY: the caller promises `actions_len` readable flags.
            unsafe { std::slice::from_raw_parts(actions, actions_len) }
        };
        let overview = summary(ruleset).with_actions(flags.iter().copied());
        let text = write_summary_as(&overview, self::format(format)?)?;
        // SAFETY: `out` passed `clear`; `len` is NULL or writable (# Safety).
        unsafe { give_text(out, len, text) }
    };
    // SAFETY: `error` is NULL or writable (# Safety).
    unsafe { call(error, body) }
}

/// Paths as a list of paths, each a list of strings (keys) and ints (indexes),
/// as `cr_ruleset_field_paths` and `cr_node_field_paths` give them.
pub(crate) fn paths_value<'a>(paths: impl Iterator<Item = &'a [Segment]>) -> Value {
    Value::List(
        paths
            .map(|path| {
                Value::List(
                    path.iter()
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
            .collect(),
    )
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
    unsafe { call(error, body) }
}

/// A copy of the rule at position `rule`, freed with `cr_rule_free`: its
/// name, condition, payload and the rest, as the rule set holds it
/// (`cr_rule_name`, `cr_rule_when`...).
///
/// # Safety
///
/// `ruleset` is `NULL` or a live `cr_ruleset`; `out` is `NULL` or points to a
/// writable `cr_rule *`; `error` is `NULL` or points to a writable
/// `cr_error *` set to `NULL`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cr_ruleset_rule(
    ruleset: *const cr_ruleset,
    rule: usize,
    out: *mut *mut cr_rule,
    error: *mut *mut cr_error,
) -> cr_status {
    let body = || {
        // SAFETY: `out` is NULL or writable (# Safety).
        unsafe { clear(out, "out") }?;
        // SAFETY: `ruleset` is NULL or live (# Safety).
        let rules = unsafe { handle(ruleset, "ruleset") }?.ruleset.rules();
        let found = rules.get(rule).ok_or_else(|| {
            Failure::Invalid(
                format!("rule {rule} is out of the rule set of {}", rules.len()),
                "pass a position below cr_ruleset_len",
            )
        })?;
        let copy = cr_rule {
            rule: found.clone(),
        };
        // SAFETY: `out` passed `clear`.
        unsafe { give(out, copy) };
        Ok(())
    };
    // SAFETY: `error` is NULL or writable (# Safety).
    unsafe { call(error, body) }
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

/// The names of the core's logics as static C strings: one for every
/// `LogicName::ALL` (a test checks it).
const LOGICS: [&CStr; 11] = [
    c"boolean",
    c"dombi",
    c"drastic",
    c"einstein",
    c"frank",
    c"hamacher",
    c"lukasiewicz",
    c"min",
    c"nilpotent",
    c"prod",
    c"yager",
];

/// The rule set's logic (`boolean`, `spec/rules.md`), a static string; `NULL`
/// for `NULL`.
///
/// # Safety
///
/// `ruleset` is `NULL` or a live `cr_ruleset`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cr_ruleset_logic(ruleset: *const cr_ruleset) -> *const c_char {
    // SAFETY: `ruleset` is NULL or live (# Safety).
    let Some(ruleset) = (unsafe { ruleset.as_ref() }) else {
        return std::ptr::null();
    };
    logic_name(ruleset.ruleset.logic()).map_or(std::ptr::null(), CStr::as_ptr)
}

/// The static C string of a logic's name.
fn logic_name(logic: LogicName) -> Option<&'static CStr> {
    LOGICS
        .iter()
        .find(|name| name.to_bytes() == logic.as_str().as_bytes())
        .copied()
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
        let paths = paths_value(summary(ruleset).fields.iter().map(|field| &field.path[..]));
        // SAFETY: `out` is NULL or writable (# Safety).
        unsafe { give_new_value(out, paths) }
    };
    // SAFETY: `error` is NULL or writable (# Safety).
    unsafe { call(error, body) }
}

#[cfg(test)]
mod tests {
    use cleanrules::logic::LogicName;

    use super::logic_name;

    #[test]
    fn every_logic_has_a_static_name() {
        for &logic in LogicName::ALL {
            assert_eq!(
                logic_name(logic).map(|name| name.to_str().ok()),
                Some(Some(logic.as_str()))
            );
        }
    }
}
