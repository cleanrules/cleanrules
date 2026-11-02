//! Errors (`spec/errors.md`): a status, and a `cr_error` with the message, the
//! hint and the JSON form.

use std::ffi::{CStr, CString, c_char};
use std::ptr;

use cleanrules::ErrorCode;
use cleanrules::format::write_error;
use cleanrules::json::{Json, to_string};

use crate::boundary::{Failure, free};
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
            json: c_text(&to_string(&json)),
        }
    }
}

impl From<Failure> for cr_error {
    fn from(failure: Failure) -> Self {
        match failure {
            Failure::Core(error) => Self {
                status: number(error.code()),
                message: c_text(error.message()),
                hint: error.hint().map(c_text),
                json: c_text(&to_string(&write_error(&error))),
            },
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
