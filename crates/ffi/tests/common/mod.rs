//! A safe wrapper over the C ABI for the tests: handles freed on drop, texts
//! copied out, errors read as their JSON form.

// Each test binary uses a different subset.
#![allow(dead_code, unreachable_pub, clippy::unwrap_used, clippy::panic)]

use std::ffi::{CStr, c_char};
use std::ptr;

use cleanrules::json::{Json, Limits, parse};
use cleanrules_ffi::*;

/// A failed call: its status and the error's JSON form.
#[derive(Debug)]
pub struct Failure {
    pub status: cr_status,
    pub json: Json,
}

impl Failure {
    pub fn code(&self) -> &str {
        match &self.json {
            Json::Object(members) => match members.iter().find(|(k, _)| k == "code") {
                Some((_, Json::String(code))) => code,
                _ => panic!("an error without a code"),
            },
            _ => panic!("an error that is not an object"),
        }
    }
}

/// Read and free an error.
pub fn take(error: *mut cr_error) -> Failure {
    assert!(!error.is_null(), "a failure without an error");
    // SAFETY: `error` is a live error from the library; its JSON lives until it
    // is freed below.
    let (status, json) = unsafe {
        let status = cr_error_code(error);
        let json = CStr::from_ptr(cr_error_json(error))
            .to_str()
            .unwrap()
            .to_owned();
        cr_error_free(error);
        (status, json)
    };
    let json = parse(&json, &Limits::default()).unwrap();
    Failure { status, json }
}

/// Call a function that gives out a handle.
pub fn handle<T>(
    call: impl FnOnce(*mut *mut T, *mut *mut cr_error) -> cr_status,
) -> Result<*mut T, Failure> {
    let mut out = ptr::null_mut();
    let mut error = ptr::null_mut();
    let status = call(&raw mut out, &raw mut error);
    if status == CR_OK {
        assert!(!out.is_null() && error.is_null());
        Ok(out)
    } else {
        assert!(out.is_null(), "a failure that gave out a handle");
        let failure = take(error);
        assert_eq!(failure.status, status);
        Err(failure)
    }
}

/// Call a function that gives out a text.
pub fn text(
    call: impl FnOnce(*mut *mut c_char, *mut usize, *mut *mut cr_error) -> cr_status,
) -> Result<String, Failure> {
    let mut out = ptr::null_mut();
    let mut len = usize::MAX;
    let mut error = ptr::null_mut();
    let status = call(&raw mut out, &raw mut len, &raw mut error);
    if status == CR_OK {
        // SAFETY: on success `out` is a NUL-terminated string from the library,
        // freed once here.
        let text = unsafe {
            let text = CStr::from_ptr(out).to_str().unwrap().to_owned();
            cr_string_free(out);
            text
        };
        assert_eq!(len, text.len());
        Ok(text)
    } else {
        assert!(out.is_null(), "a failure that gave out a text");
        Err(take(error))
    }
}

/// Text arguments as a pointer and a length.
pub fn arg(text: &[u8]) -> (*const c_char, usize) {
    (text.as_ptr().cast(), text.len())
}

macro_rules! owned {
    ($name:ident, $raw:ty, $free:ident) => {
        /// An owned handle, freed on drop.
        pub struct $name(pub *mut $raw);

        impl Drop for $name {
            fn drop(&mut self) {
                // SAFETY: the handle came from the library and is freed once.
                unsafe { $free(self.0) };
            }
        }
    };
}

owned!(Document, cr_document, cr_document_free);
owned!(RuleSet, cr_ruleset, cr_ruleset_free);
owned!(Decision, cr_decision, cr_decision_free);
owned!(Limits_, cr_limits, cr_limits_free);

/// Limits with some values set (`CR_LIMIT_*`).
pub fn limits(values: &[(i32, usize)]) -> Limits_ {
    let limits = Limits_(cr_limits_new());
    for &(which, value) in values {
        // SAFETY: a live handle that only this thread uses; no error asked for.
        let status = unsafe { cr_limits_set(limits.0, which, value, ptr::null_mut()) };
        assert_eq!(status, CR_OK);
    }
    limits
}

fn raw(limits: Option<&Limits_>) -> *const cr_limits {
    limits.map_or(ptr::null(), |l| l.0)
}

pub fn read_document(
    text: &[u8],
    format: i32,
    limits: Option<&Limits_>,
) -> Result<Document, Failure> {
    let (p, n) = arg(text);
    // SAFETY: the text outlives the call; the limits are live or NULL.
    handle(|out, error| unsafe { cr_document_read(p, n, format, raw(limits), out, error) })
        .map(Document)
}

pub fn write_document(document: &Document, format: i32) -> Result<String, Failure> {
    // SAFETY: a live document.
    text(|out, len, error| unsafe { cr_document_write(document.0, format, out, len, error) })
}

pub fn kind(document: &Document) -> i32 {
    // SAFETY: a live document.
    unsafe { cr_document_kind(document.0) }
}

pub fn ruleset_of(document: &Document) -> Result<RuleSet, Failure> {
    // SAFETY: a live document.
    handle(|out, error| unsafe { cr_ruleset_from_document(document.0, out, error) }).map(RuleSet)
}

pub fn read_ruleset(text: &[u8], format: i32) -> Result<RuleSet, Failure> {
    let (p, n) = arg(text);
    // SAFETY: the text outlives the call.
    handle(|out, error| unsafe { cr_ruleset_read(p, n, format, ptr::null(), out, error) })
        .map(RuleSet)
}

pub fn write_ruleset(ruleset: &RuleSet, format: i32) -> Result<String, Failure> {
    // SAFETY: a live rule set.
    text(|out, len, error| unsafe { cr_ruleset_write(ruleset.0, format, out, len, error) })
}

pub fn summary(ruleset: &RuleSet, format: i32) -> Result<String, Failure> {
    // SAFETY: a live rule set.
    text(|out, len, error| unsafe { cr_ruleset_summary(ruleset.0, format, out, len, error) })
}

pub fn evaluate(ruleset: &RuleSet, input: &[u8]) -> Result<Decision, Failure> {
    let (p, n) = arg(input);
    // SAFETY: a live rule set; the input outlives the call.
    handle(|out, error| unsafe { cr_ruleset_evaluate(ruleset.0, p, n, ptr::null(), out, error) })
        .map(Decision)
}

pub fn read_decision(
    ruleset: &RuleSet,
    text: &[u8],
    format: i32,
    limits: Option<&Limits_>,
) -> Result<Decision, Failure> {
    let (p, n) = arg(text);
    // SAFETY: a live rule set; the text outlives the call; live or NULL limits.
    handle(|out, error| unsafe {
        cr_ruleset_read_decision(ruleset.0, p, n, format, raw(limits), out, error)
    })
    .map(Decision)
}

pub fn write_decision(decision: &Decision, format: i32) -> Result<String, Failure> {
    // SAFETY: a live decision.
    text(|out, len, error| unsafe { cr_decision_write(decision.0, format, out, len, error) })
}

pub fn explain(decision: &Decision, format: i32) -> Result<String, Failure> {
    // SAFETY: a live decision.
    text(|out, len, error| unsafe { cr_decision_explain(decision.0, format, out, len, error) })
}

pub fn output(decision: &Decision) -> Result<String, Failure> {
    // SAFETY: a live decision.
    text(|out, len, error| unsafe { cr_decision_output(decision.0, out, len, error) })
}

pub fn outputs(decision: &Decision) -> Result<String, Failure> {
    // SAFETY: a live decision.
    text(|out, len, error| unsafe { cr_decision_outputs(decision.0, out, len, error) })
}

/// A JSON text.
pub fn json(text: &str) -> Json {
    parse(text, &Limits::default()).unwrap()
}
