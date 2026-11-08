//! Errors (`spec/errors.md`): a status, and a `cr_error` with the message, the
//! hint and the JSON form.

use std::ffi::{CStr, CString, c_char};
use std::ptr;

use cleanrules::format::write_error;
use cleanrules::json::{Json, to_string};
use cleanrules::rules::{ActionFailure, action_error};
use cleanrules::value::too_many_values;
use cleanrules::{ErrorCode, Value};

use crate::boundary::{Failure, bytes, call, clear, free, give, utf8_arg};
use crate::decision::cr_decision;
use crate::value::borrow_value;
use crate::{CR_INTERNAL_ERROR, CR_INVALID_ARGUMENT, cr_status};

/// `format_error`.
pub const CR_FORMAT_ERROR: cr_status = 1;
/// `unsupported_version`.
pub const CR_UNSUPPORTED_VERSION: cr_status = 2;
/// `limit_exceeded`.
pub const CR_LIMIT_EXCEEDED: cr_status = 3;
/// `decision_mismatch`.
pub const CR_DECISION_MISMATCH: cr_status = 4;
/// `expression_error`.
pub const CR_EXPRESSION_ERROR: cr_status = 5;
/// `rule_definition_error`.
pub const CR_RULE_DEFINITION_ERROR: cr_status = 6;
/// `rule_syntax_error`.
pub const CR_RULE_SYNTAX_ERROR: cr_status = 7;
/// `registry_error`.
pub const CR_REGISTRY_ERROR: cr_status = 8;
/// `rule_not_found`.
pub const CR_RULE_NOT_FOUND: cr_status = 9;
/// `evaluation_error`.
pub const CR_EVALUATION_ERROR: cr_status = 10;
/// `missing_field`.
pub const CR_MISSING_FIELD: cr_status = 11;
/// `type_mismatch`.
pub const CR_TYPE_MISMATCH: cr_status = 12;
/// `hit_policy_error`.
pub const CR_HIT_POLICY_ERROR: cr_status = 13;
/// `action_error`.
pub const CR_ACTION_ERROR: cr_status = 14;
/// `unsupported`.
pub const CR_UNSUPPORTED: cr_status = 15;

/// The number of an error code (`spec/errors.md`, *Numeric codes*).
fn number(code: ErrorCode) -> cr_status {
    match code {
        ErrorCode::FormatError => CR_FORMAT_ERROR,
        ErrorCode::UnsupportedVersion => CR_UNSUPPORTED_VERSION,
        ErrorCode::LimitExceeded => CR_LIMIT_EXCEEDED,
        ErrorCode::DecisionMismatch => CR_DECISION_MISMATCH,
        ErrorCode::ExpressionError => CR_EXPRESSION_ERROR,
        ErrorCode::RuleDefinitionError => CR_RULE_DEFINITION_ERROR,
        ErrorCode::RuleSyntaxError => CR_RULE_SYNTAX_ERROR,
        ErrorCode::RegistryError => CR_REGISTRY_ERROR,
        ErrorCode::RuleNotFound => CR_RULE_NOT_FOUND,
        ErrorCode::EvaluationError => CR_EVALUATION_ERROR,
        ErrorCode::MissingField => CR_MISSING_FIELD,
        ErrorCode::TypeMismatch => CR_TYPE_MISMATCH,
        ErrorCode::HitPolicyError => CR_HIT_POLICY_ERROR,
        ErrorCode::ActionError => CR_ACTION_ERROR,
        ErrorCode::Unsupported => CR_UNSUPPORTED,
        // A code added to the core without a number here is a bug, caught by
        // the tests.
        _ => CR_INTERNAL_ERROR,
    }
}

fn code_of(status: cr_status) -> Option<ErrorCode> {
    ErrorCode::ALL
        .into_iter()
        .find(|&code| number(code) == status)
}

/// A C string with any NUL written as `\0`.
fn c_text(text: &str) -> CString {
    CString::new(text.replace('\0', "\\0")).unwrap_or_default()
}

/// An error: its status, message, hint and JSON form.
pub struct cr_error {
    status: cr_status,
    message: CString,
    hint: Option<CString>,
    json: CString,
    /// For `CR_ACTION_ERROR`, the decision whose action failed.
    decision: Option<Box<cr_decision>>,
}

impl cr_error {
    pub(crate) fn status(&self) -> cr_status {
        self.status
    }

    fn own(status: cr_status, code: &str, message: &str, hint: Option<&str>) -> Self {
        let mut members = vec![
            ("code".into(), Json::String(code.into())),
            ("message".into(), Json::String(message.into())),
        ];
        if let Some(hint) = hint {
            members.push(("hint".into(), Json::String(hint.into())));
        }
        let json = Json::Object(members);
        Self {
            status,
            message: c_text(message),
            hint: hint.map(c_text),
            decision: None,
            json: c_text(&to_string(&json)),
        }
    }
}

impl cr_error {
    fn core(error: &cleanrules::Error, decision: Option<Box<cr_decision>>) -> Self {
        Self {
            status: number(error.code()),
            message: c_text(error.message()),
            hint: error.hint().map(c_text),
            json: c_text(&to_string(&write_error(error))),
            decision,
        }
    }
}

impl From<Failure> for cr_error {
    fn from(failure: Failure) -> Self {
        match failure {
            Failure::Core(error) => Self::core(&error, None),
            Failure::Action(error, decision) => Self::core(&error, Some(decision)),
            Failure::Invalid(message, hint) => Self::own(
                CR_INVALID_ARGUMENT,
                "invalid_argument",
                &message,
                Some(hint),
            ),
            Failure::Internal(message) => {
                Self::own(CR_INTERNAL_ERROR, "internal_error", &message, None)
            }
        }
    }
}

/// A new `action_error` (`spec/rules.md`, *Actions*), made by the core as for
/// every binding that runs actions itself (Java's and JavaScript's idiomatic
/// `run`): the action of `rule` (named `action`, or `NULL` for an action
/// given as code) threw what `detail` describes, after the actions of the
/// rules in `executed` (`NULL`, or a list of strings) had run. The error has
/// the message, the hint and the JSON form the core writes, with `rule`,
/// `executed` and the detail as `cause`; it is given through `out`, owned by
/// the caller and freed with `cr_error_free`. The call's own failures go to
/// `error`.
///
/// # Safety
///
/// `rule`, `action` and `detail` are `NULL` or point to as many readable
/// bytes as their lengths say; `executed` is `NULL` or a live `cr_value`;
/// `out` is `NULL` or points to a writable `cr_error *`; `error` is `NULL` or
/// points to a writable `cr_error *` set to `NULL`.
#[unsafe(no_mangle)]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn cr_error_new_action(
    rule: *const c_char,
    rule_len: usize,
    action: *const c_char,
    action_len: usize,
    detail: *const c_char,
    detail_len: usize,
    executed: *const crate::value::cr_value,
    out: *mut *mut cr_error,
    error: *mut *mut cr_error,
) -> cr_status {
    let body = || {
        // SAFETY: `out` is NULL or writable (# Safety).
        unsafe { clear(out, "out") }?;
        // SAFETY: the texts and their lengths (# Safety).
        let (rule, detail) = unsafe {
            (
                utf8_arg(bytes(rule, rule_len, "rule")?, "rule")?,
                utf8_arg(bytes(detail, detail_len, "detail")?, "detail")?,
            )
        };
        let action = if action.is_null() {
            None
        } else {
            // SAFETY: `action` and `action_len` (# Safety).
            Some(utf8_arg(
                unsafe { bytes(action, action_len, "action") }?,
                "action",
            )?)
        };
        let names = if executed.is_null() {
            Vec::new()
        } else {
            // SAFETY: `executed` is live (# Safety).
            let Value::List(items) = (unsafe { borrow_value(executed, "executed") })? else {
                return Err(Failure::Invalid(
                    "executed is not a list".into(),
                    "pass a list of rule names, or NULL",
                ));
            };
            let mut names = Vec::with_capacity(items.len());
            for item in items {
                let Value::String(name) = item else {
                    return Err(Failure::Invalid(
                        "executed holds something other than a string".into(),
                        "pass a list of rule names",
                    ));
                };
                names.push(name.clone());
            }
            names
        };
        let failed = action_error(
            rule,
            action,
            &ActionFailure::Threw(detail.to_owned()),
            names,
        );
        // SAFETY: `out` passed `clear`.
        unsafe { give(out, cr_error::core(&failed, None)) };
        Ok(())
    };
    // SAFETY: `error` is NULL or writable (# Safety).
    unsafe { call(error, body) }
}

/// A new `limit_exceeded` for native data beyond `max_values`
/// (`spec/data-model.md`, *Reading native data*, *Limits*), made by the core
/// for the bindings that read native data on this ABI and count what they
/// convert: its context has `name` = `max_values`, the `row` of a batch when
/// `row` is 0 or more, and the `rule` whose metadata or payload constant it is
/// when `rule` is not `NULL`. It is given through `out`, owned by the caller
/// and freed with `cr_error_free`; the call's own failures go to `error`.
///
/// # Safety
///
/// `rule` is `NULL` or points to `rule_len` readable bytes; `out` is `NULL` or
/// points to a writable `cr_error *`; `error` is `NULL` or points to a
/// writable `cr_error *` set to `NULL`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cr_error_new_max_values(
    max_values: usize,
    row: i64,
    rule: *const c_char,
    rule_len: usize,
    out: *mut *mut cr_error,
    error: *mut *mut cr_error,
) -> cr_status {
    let body = || {
        // SAFETY: `out` is NULL or writable (# Safety).
        unsafe { clear(out, "out") }?;
        let rule = if rule.is_null() {
            None
        } else {
            // SAFETY: `rule` and `rule_len` (# Safety).
            Some(utf8_arg(unsafe { bytes(rule, rule_len, "rule") }?, "rule")?.to_owned())
        };
        let mut failed = too_many_values(max_values);
        failed.context_mut().row = u64::try_from(row).ok();
        failed.context_mut().rule = rule;
        // SAFETY: `out` passed `clear`.
        unsafe { give(out, cr_error::core(&failed, None)) };
        Ok(())
    };
    // SAFETY: `error` is NULL or writable (# Safety).
    unsafe { call(error, body) }
}

/// The error's status: an error code, `CR_INVALID_ARGUMENT` or
/// `CR_INTERNAL_ERROR`; `CR_INVALID_ARGUMENT` for `NULL`.
///
/// # Safety
///
/// `error` is `NULL` or a live `cr_error`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cr_error_code(error: *const cr_error) -> cr_status {
    // SAFETY: the caller promises a live handle when it is not null.
    unsafe { error.as_ref() }.map_or(CR_INVALID_ARGUMENT, |e| e.status)
}

/// The error's message, owned by the error; `NULL` for `NULL`.
///
/// # Safety
///
/// `error` is `NULL` or a live `cr_error`. The message lives as long as it.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cr_error_message(error: *const cr_error) -> *const c_char {
    // SAFETY: the caller promises a live handle when it is not null.
    unsafe { error.as_ref() }.map_or(ptr::null(), |e| e.message.as_ptr())
}

/// The error's hint, owned by the error, or `NULL` when it has none.
///
/// # Safety
///
/// `error` is `NULL` or a live `cr_error`. The hint lives as long as it.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cr_error_hint(error: *const cr_error) -> *const c_char {
    // SAFETY: the caller promises a live handle when it is not null.
    unsafe { error.as_ref() }
        .and_then(|e| e.hint.as_ref())
        .map_or(ptr::null(), |hint| hint.as_ptr())
}

/// The error's JSON form (`spec/errors.md`, *JSON form*), owned by the error:
/// the code, message, hint, context and cause. `NULL` for `NULL`.
///
/// # Safety
///
/// `error` is `NULL` or a live `cr_error`. The text lives as long as it.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cr_error_json(error: *const cr_error) -> *const c_char {
    // SAFETY: the caller promises a live handle when it is not null.
    unsafe { error.as_ref() }.map_or(ptr::null(), |e| e.json.as_ptr())
}

/// For a `CR_ACTION_ERROR` from `cr_ruleset_run`, the decision whose action
/// failed, borrowed from the error; `NULL` for any other error and for `NULL`.
///
/// # Safety
///
/// `error` is `NULL` or a live `cr_error`. The decision lives as long as the
/// error and must never be freed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cr_error_decision(error: *const cr_error) -> *const cr_decision {
    // SAFETY: the caller promises a live handle when it is not null.
    unsafe { error.as_ref() }
        .and_then(|e| e.decision.as_deref())
        .map_or(ptr::null(), ptr::from_ref)
}

/// Free an error. `NULL` is ignored.
///
/// # Safety
///
/// `error` is `NULL` or a `cr_error` from this library, not freed before.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cr_error_free(error: *mut cr_error) {
    // SAFETY: the caller's promise is `free`'s.
    unsafe { free(error) };
}

/// The name of a status (`ok`, `format_error`, `invalid_argument`, ...), a
/// static string, never freed; `NULL` for an unknown number.
#[unsafe(no_mangle)]
pub extern "C" fn cr_status_name(status: cr_status) -> *const c_char {
    const NAMES: [&CStr; 15] = [
        c"format_error",
        c"unsupported_version",
        c"limit_exceeded",
        c"decision_mismatch",
        c"expression_error",
        c"rule_definition_error",
        c"rule_syntax_error",
        c"registry_error",
        c"rule_not_found",
        c"evaluation_error",
        c"missing_field",
        c"type_mismatch",
        c"hit_policy_error",
        c"action_error",
        c"unsupported",
    ];
    let name = match status {
        crate::CR_OK => Some(c"ok"),
        CR_INVALID_ARGUMENT => Some(c"invalid_argument"),
        CR_INTERNAL_ERROR => Some(c"internal_error"),
        _ => code_of(status).and_then(|code| {
            NAMES
                .iter()
                .find(|name| name.to_bytes() == code.as_str().as_bytes())
                .copied()
        }),
    };
    name.map_or(ptr::null(), CStr::as_ptr)
}

/// The parent of an error code in the hierarchy of `spec/errors.md`
/// (`CR_FORMAT_ERROR` for `CR_LIMIT_EXCEEDED`); `CR_OK` for a code
/// without a parent and for any other number.
#[unsafe(no_mangle)]
pub extern "C" fn cr_error_parent(status: cr_status) -> cr_status {
    code_of(status)
        .and_then(ErrorCode::parent)
        .map_or(crate::CR_OK, number)
}

/// Whether `status` is `ancestor` or, for an error code, one of its
/// descendants: `cr_error_is(CR_MISSING_FIELD, CR_EVALUATION_ERROR)` is true,
/// and so is `cr_error_is(CR_INVALID_ARGUMENT, CR_INVALID_ARGUMENT)`.
#[unsafe(no_mangle)]
pub extern "C" fn cr_error_is(status: cr_status, ancestor: cr_status) -> bool {
    match (code_of(status), code_of(ancestor)) {
        (Some(code), Some(ancestor)) => code.is_a(ancestor),
        _ => status == ancestor && status != crate::CR_OK,
    }
}

#[cfg(test)]
mod tests {
    use super::{ErrorCode, code_of, number};

    #[test]
    fn every_code_has_the_number_of_the_spec() {
        // `spec/errors.md`, *Numeric codes*: the order of the table.
        for (i, code) in ErrorCode::ALL.into_iter().enumerate() {
            assert_eq!(usize::try_from(number(code)).unwrap(), i + 1, "{code}");
            assert_eq!(code_of(number(code)), Some(code));
        }
        assert_eq!(code_of(0), None);
        assert_eq!(code_of(16), None);
    }
}
