//! Bundles (`spec/bundles.md`): made from rules, compared. A bundle is a
//! document (`CR_DOCUMENT_BUNDLE`): it is read, written, hashed and drawn with
//! the `cr_document_*` functions, and gives its rule set to
//! `cr_ruleset_from_document`.

use std::ffi::c_char;

use cleanrules::bundle::{Bundle, BundleRules, CaseFile};
use cleanrules::format::Document;

use crate::boundary::{
    Failure, Outcome, bytes, call, clear, give, give_text, handle, limits_of, utf8_arg,
};
use crate::cr_status;
use crate::document::{cr_document, not_holding};
use crate::error::cr_error;
use crate::limits::cr_limits;

/// A file of test cases given to `cr_bundle_new`: its path, relative to the
/// bundle's directory, as UTF-8 bytes, and the file's content. The library
/// keeps the path and the SHA-256 digest of the content, never the pointers.
#[repr(C)]
pub struct cr_case_file {
    /// The path: `len` bytes of UTF-8, such as `cases/basic.json`.
    pub path: *const c_char,
    /// The length of `path` in bytes.
    pub path_len: usize,
    /// The file's bytes, any content.
    pub bytes: *const u8,
    /// The number of bytes.
    pub len: usize,
}

/// Make a bundle of the rules `rules` holds, a `CR_DOCUMENT_RULESET` or
/// `CR_DOCUMENT_DECISION_TABLE` document, with this engine
/// (`spec/bundles.md`, *Making a bundle*): the identifier and revision its
/// author gives, its `case_count` case files in order, and `limits` (the
/// defaults for `NULL`), which it records. The bundle is a new
/// `CR_DOCUMENT_BUNDLE` document. An identifier, revision or path that does
/// not have its form, or a path given twice, is a `CR_FORMAT_ERROR`; a limit
/// of 0 or above 2^53 - 1 is `CR_UNSUPPORTED`; a document of another kind is
/// `CR_INVALID_ARGUMENT`.
///
/// # Safety
///
/// `rules` is `NULL` or a live `cr_document`; `id` and `revision` are `NULL`
/// or point to `id_len` and `revision_len` readable bytes; `cases` is `NULL`
/// with a `case_count` of 0, or points to `case_count` readable `cr_case_file`
/// whose `path` and `bytes` are `NULL` or point to `path_len` and `len`
/// readable bytes; `limits` is `NULL` or a live `cr_limits`; `out` is `NULL`
/// or points to a writable `cr_document *`; `error` is `NULL` or points to a
/// writable `cr_error *` set to `NULL`.
#[unsafe(no_mangle)]
#[allow(clippy::too_many_arguments)] // C has no structs of arguments here.
pub unsafe extern "C" fn cr_bundle_new(
    rules: *const cr_document,
    id: *const c_char,
    id_len: usize,
    revision: *const c_char,
    revision_len: usize,
    cases: *const cr_case_file,
    case_count: usize,
    limits: *const cr_limits,
    out: *mut *mut cr_document,
    error: *mut *mut cr_error,
) -> cr_status {
    let body = || {
        // SAFETY: `out` is NULL or writable (# Safety).
        unsafe { clear(out, "out") }?;
        // SAFETY: `rules` is NULL or live (# Safety).
        let rules: BundleRules = match &unsafe { handle(rules, "rules") }?.document {
            Document::RuleSet(ruleset) => ruleset.clone().into(),
            Document::DecisionTable(table) => table.clone().into(),
            other => return Err(not_holding("a rule set or a decision table", other)),
        };
        // SAFETY: `id` and `id_len` (# Safety).
        let id = utf8_arg(unsafe { bytes(id, id_len, "id") }?, "the identifier")?;
        // SAFETY: `revision` and `revision_len` (# Safety).
        let revision = utf8_arg(
            unsafe { bytes(revision, revision_len, "revision") }?,
            "the revision",
        )?;
        // SAFETY: `cases` and `case_count` (# Safety).
        let cases = unsafe { case_files(cases, case_count) }?;
        // SAFETY: `limits` is NULL or live (# Safety).
        let limits = unsafe { limits_of(limits) };
        let bundle = Bundle::new(rules, id, revision, cases, &limits)?;
        // SAFETY: `out` passed `clear`.
        unsafe {
            give(
                out,
                cr_document {
                    document: bundle.into_document(),
                },
            );
        };
        Ok(())
    };
    // SAFETY: `error` is NULL or writable (# Safety).
    unsafe { call(error, body) }
}

/// The case files of `cr_bundle_new`, in order.
///
/// # Safety
///
/// As `cr_bundle_new`'s `cases` and `case_count`.
unsafe fn case_files(cases: *const cr_case_file, count: usize) -> Outcome<Vec<CaseFile>> {
    if count == 0 {
        return Ok(Vec::new());
    }
    if cases.is_null() {
        return Err(Failure::Invalid(
            format!("cases is NULL with a count of {count}"),
            "pass the case files, or a count of 0",
        ));
    }
    // SAFETY: the caller promises `count` readable items at `cases`.
    let items = unsafe { std::slice::from_raw_parts(cases, count) };
    items
        .iter()
        .map(|item| {
            // SAFETY: each item's `path` and `path_len` (# Safety).
            let path = utf8_arg(
                unsafe { bytes(item.path, item.path_len, "a case path") }?,
                "a case path",
            )?;
            // SAFETY: each item's `bytes` and `len` (# Safety).
            let content = unsafe { bytes(item.bytes.cast(), item.len, "a case file's bytes") }?;
            Ok(CaseFile::new(path, content)?)
        })
        .collect()
}

/// Compare two bundles (`spec/bundles.md`, *Comparing bundles*): the JSON
/// object of the comparison, with `relation`, `identical` and `differences`,
/// such as `{"relation":"same_rules_other_engine","identical":false,
/// "differences":["engine.version"]}`. Documents that are not bundles are
/// `CR_INVALID_ARGUMENT`.
///
/// # Safety
///
/// `a` and `b` are `NULL` or live `cr_document`s; `out` is `NULL` or points to
/// a writable `char *`; `len` is `NULL` or points to a writable `size_t`;
/// `error` is `NULL` or points to a writable `cr_error *` set to `NULL`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cr_bundle_compare(
    a: *const cr_document,
    b: *const cr_document,
    out: *mut *mut c_char,
    len: *mut usize,
    error: *mut *mut cr_error,
) -> cr_status {
    let body = || {
        // SAFETY: `out` is NULL or writable (# Safety).
        unsafe { clear(out, "out") }?;
        let bundle = |document: *const cr_document, what: &str| -> Outcome<&Bundle> {
            // SAFETY: `a` and `b` are NULL or live (# Safety).
            match &unsafe { handle(document, what) }?.document {
                Document::Bundle(bundle) => Ok(bundle),
                other => Err(not_holding("a bundle", other)),
            }
        };
        let comparison = bundle(a, "a")?.compare(bundle(b, "b")?);
        let text = cleanrules::json::to_string(&comparison.to_json());
        // SAFETY: `out` passed `clear`; `len` is NULL or writable (# Safety).
        unsafe { give_text(out, len, text) }
    };
    // SAFETY: `error` is NULL or writable (# Safety).
    unsafe { call(error, body) }
}
