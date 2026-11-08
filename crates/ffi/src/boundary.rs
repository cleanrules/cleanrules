//! Crossing the boundary: panics caught, arguments checked and results handed
//! back, so that the exported functions only call the core.
//!
//! Every helper that touches a raw pointer is an `unsafe fn` whose contract is
//! one clause of the exported function's `# Safety`, and every call to one is
//! its own `unsafe` block.

use std::any::Any;
use std::ffi::{CString, c_char};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::ptr;

use cleanrules::format::DocumentFormat;
use cleanrules::json::Limits;

use crate::error::cr_error;
use crate::limits::cr_limits;
use crate::{
    CR_FORMAT_CSV, CR_FORMAT_JSON, CR_FORMAT_TEXT, CR_FORMAT_YAML, CR_OK, cr_format, cr_status,
};

/// Why a call failed.
pub(crate) enum Failure {
    /// An error of the core.
    Core(cleanrules::Error),
    /// The caller broke the ABI's rules (`CR_INVALID_ARGUMENT`): what, and how
    /// to fix it.
    Invalid(String, &'static str),
    /// An action failed (`CR_ACTION_ERROR`); the error keeps the decision.
    Action(cleanrules::Error, Box<crate::decision::cr_decision>),
    /// A panic (`CR_INTERNAL_ERROR`).
    Internal(String),
}

impl From<cleanrules::Error> for Failure {
    fn from(error: cleanrules::Error) -> Self {
        Self::Core(error)
    }
}

pub(crate) type Outcome<T = ()> = Result<T, Failure>;

fn null(what: &str) -> Failure {
    Failure::Invalid(
        format!("{what} is NULL"),
        "pass a pointer, as the function's documentation in cleanrules.h says",
    )
}

fn panic_message(panic: &(dyn Any + Send)) -> String {
    let what = panic
        .downcast_ref::<&str>()
        .copied()
        .or_else(|| panic.downcast_ref::<String>().map(String::as_str))
        .unwrap_or("a panic");
    format!("cleanrules failed ({what}); please report it")
}

/// Run the body of an exported function, catching panics, and report a
/// failure through `error`. Messages never name the C function, so that every
/// binding shows them as they are (`spec/errors.md`, *JSON form*).
///
/// # Safety
///
/// `error` is `NULL` or points to a writable `cr_error *`.
pub(crate) unsafe fn call(error: *mut *mut cr_error, body: impl FnOnce() -> Outcome) -> cr_status {
    let outcome = catch_unwind(AssertUnwindSafe(body))
        .unwrap_or_else(|panic| Err(Failure::Internal(panic_message(panic.as_ref()))));
    match outcome {
        Ok(()) => CR_OK,
        Err(failure) => {
            let failure = cr_error::from(failure);
            let status = failure.status();
            if !error.is_null() {
                // SAFETY: the caller promises that a non-null `error` is writable.
                unsafe { *error = Box::into_raw(Box::new(failure)) };
            }
            status
        }
    }
}

/// The bytes of a text argument.
///
/// # Safety
///
/// `text` is `NULL` or points to `len` bytes that stay readable and unchanged
/// during the call.
pub(crate) unsafe fn bytes<'a>(text: *const c_char, len: usize, what: &str) -> Outcome<&'a [u8]> {
    if text.is_null() {
        return if len == 0 {
            Ok(&[])
        } else {
            Err(Failure::Invalid(
                format!("{what} is NULL with a length of {len}"),
                "pass the text, or a length of 0 for the empty text",
            ))
        };
    }
    // SAFETY: the caller promises `len` readable bytes at `text`, which a C
    // object cannot make larger than `isize::MAX`.
    Ok(unsafe { std::slice::from_raw_parts(text.cast::<u8>(), len) })
}

/// A small text argument (a key, a name, a string), which must be UTF-8: a
/// `format_error` that names it otherwise.
pub(crate) fn utf8_arg<'a>(bytes: &'a [u8], what: &str) -> Outcome<&'a str> {
    std::str::from_utf8(bytes).map_err(|error| {
        Failure::Core(
            cleanrules::Error::new(
                cleanrules::ErrorCode::FormatError,
                format!("{what} is not valid UTF-8 at byte {}", error.valid_up_to()),
            )
            .with_hint("pass UTF-8 text"),
        )
    })
}

/// A handle argument.
///
/// # Safety
///
/// `handle` is `NULL` or a live handle of type `T` from this library.
pub(crate) unsafe fn handle<'a, T>(handle: *const T, what: &str) -> Outcome<&'a T> {
    // SAFETY: the caller promises a live handle when it is not null.
    unsafe { handle.as_ref() }.ok_or_else(|| null(what))
}

/// A handle argument the call changes.
///
/// # Safety
///
/// `handle` is `NULL` or a live handle of type `T` from this library, which no
/// other thread uses during the call.
pub(crate) unsafe fn handle_mut<'a, T>(handle: *mut T, what: &str) -> Outcome<&'a mut T> {
    // SAFETY: the caller promises a live handle, used by this call only.
    unsafe { handle.as_mut() }.ok_or_else(|| null(what))
}

/// The limits argument: the defaults for `NULL`.
///
/// # Safety
///
/// `limits` is `NULL` or a live `cr_limits`.
pub(crate) unsafe fn limits_of(limits: *const cr_limits) -> Limits {
    // SAFETY: the caller promises a live handle when it is not null.
    unsafe { limits.as_ref() }.map_or_else(Limits::default, |l| l.limits)
}

/// A format argument.
pub(crate) fn format(format: cr_format) -> Outcome<DocumentFormat> {
    match format {
        CR_FORMAT_JSON => Ok(DocumentFormat::Json),
        CR_FORMAT_YAML => Ok(DocumentFormat::Yaml),
        CR_FORMAT_TEXT => Ok(DocumentFormat::Text),
        CR_FORMAT_CSV => Ok(DocumentFormat::Csv),
        other => Err(Failure::Invalid(
            format!("format {other} is not a CR_FORMAT_* constant"),
            "use CR_FORMAT_JSON (1), CR_FORMAT_YAML (2), CR_FORMAT_TEXT (3) or CR_FORMAT_CSV (4)",
        )),
    }
}

/// Check an out-parameter and set it to `NULL` until there is a result.
///
/// # Safety
///
/// `out` is `NULL` or points to a writable `T *`.
pub(crate) unsafe fn clear<T>(out: *mut *mut T, what: &str) -> Outcome {
    if out.is_null() {
        return Err(null(what));
    }
    // SAFETY: the caller promises that a non-null `out` is writable.
    unsafe { *out = ptr::null_mut() };
    Ok(())
}

/// Set borrowed out-parameters (a text and its length, or an item) to `NULL`
/// and 0 until there is a result.
///
/// # Safety
///
/// `out` is `NULL` or points to a writable `*const T`; `len` is `NULL` or
/// points to a writable `size_t`.
pub(crate) unsafe fn clear_borrowed<T>(out: *mut *const T, len: *mut usize) {
    if !out.is_null() {
        // SAFETY: the caller promises that a non-null `out` is writable.
        unsafe { *out = ptr::null() };
    }
    if !len.is_null() {
        // SAFETY: the caller promises that a non-null `len` is writable.
        unsafe { *len = 0 };
    }
}

/// Write a value through a pointer argument.
///
/// # Safety
///
/// `out` is `NULL` or points to a writable `T`.
pub(crate) unsafe fn write_out<T>(out: *mut T, value: T, what: &str) -> Outcome {
    if out.is_null() {
        return Err(null(what));
    }
    // SAFETY: the caller promises that a non-null `out` is writable.
    unsafe { *out = value };
    Ok(())
}

/// Hand a value to the caller as a new handle.
///
/// # Safety
///
/// `out` passed [`clear`].
pub(crate) unsafe fn give<T>(out: *mut *mut T, value: T) {
    // SAFETY: `clear` checked that `out` is writable.
    unsafe { *out = Box::into_raw(Box::new(value)) };
}

/// Hand an existing box to the caller as its handle, keeping its address.
///
/// # Safety
///
/// `out` passed [`clear`].
pub(crate) unsafe fn give_boxed<T>(out: *mut *mut T, value: Box<T>) {
    // SAFETY: `clear` checked that `out` is writable.
    unsafe { *out = Box::into_raw(value) };
}

/// Hand a text to the caller, with its length when `len` is not `NULL`.
///
/// # Safety
///
/// `out` passed [`clear`]; `len` is `NULL` or points to a writable `size_t`.
pub(crate) unsafe fn give_text(out: *mut *mut c_char, len: *mut usize, text: String) -> Outcome {
    let size = text.len();
    let text = CString::new(text)
        .map_err(|_| Failure::Internal("a text holds a NUL character; please report it".into()))?;
    // SAFETY: `clear` checked that `out` is writable.
    unsafe { *out = text.into_raw() };
    if !len.is_null() {
        // SAFETY: the caller promises that a non-null `len` is writable.
        unsafe { *len = size };
    }
    Ok(())
}

/// Free a handle given out by [`give`].
///
/// # Safety
///
/// `handle` is `NULL` or came from [`give`] with the same `T`, not freed before.
pub(crate) unsafe fn free<T>(handle: *mut T) {
    if !handle.is_null() {
        // SAFETY: the caller promises a box from `give`, freed once.
        drop(unsafe { Box::from_raw(handle) });
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::panic)]

    use std::ptr;

    use super::{Failure, call};
    use crate::error::{cr_error_code, cr_error_free, cr_error_message};
    use crate::{CR_INTERNAL_ERROR, CR_INVALID_ARGUMENT, CR_OK};

    #[test]
    fn a_panic_is_an_internal_error() {
        let mut error = ptr::null_mut();
        // SAFETY: a writable error.
        let status = unsafe { call(&raw mut error, || panic!("boom")) };
        assert_eq!(status, CR_INTERNAL_ERROR);
        // SAFETY: the error the call gave out, freed once.
        unsafe {
            assert_eq!(cr_error_code(error), CR_INTERNAL_ERROR);
            cr_error_free(error);
        }
        // SAFETY: no error asked for.
        let status = unsafe { call(ptr::null_mut(), || Ok(())) };
        assert_eq!(status, CR_OK);
    }

    #[test]
    fn an_invalid_argument_does_not_name_the_function() {
        let mut error = ptr::null_mut();
        // SAFETY: a writable error.
        let status = unsafe {
            call(&raw mut error, || {
                Err(Failure::Invalid("x is NULL".into(), "pass x"))
            })
        };
        assert_eq!(status, CR_INVALID_ARGUMENT);
        // SAFETY: the error the call gave out; its message is read before it
        // is freed.
        unsafe {
            let message = std::ffi::CStr::from_ptr(cr_error_message(error));
            assert_eq!(message.to_str().unwrap(), "x is NULL");
            cr_error_free(error);
        }
    }
}
