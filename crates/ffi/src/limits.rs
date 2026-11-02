//! Reading limits (`spec/json-format.md`, *Errors and limits*).

use cleanrules::json::Limits;

use crate::boundary::{Failure, Outcome, call, free, handle, handle_mut, write_out};
use crate::cr_status;
use crate::error::cr_error;

/// A limit of `cr_limits` (`CR_LIMIT_*`).
pub type cr_limit = i32;

/// The largest text of every format, in bytes. Set, it sets the JSON, YAML and
/// text limits at once; read, it gives the JSON one.
pub const CR_LIMIT_MAX_SIZE: cr_limit = 1;
/// The deepest nesting of JSON arrays and objects.
pub const CR_LIMIT_MAX_NESTING: cr_limit = 2;
/// The deepest condition or payload value, in nodes.
pub const CR_LIMIT_MAX_DEPTH: cr_limit = 3;
/// The most cells and nodes a decision table may build.
pub const CR_LIMIT_MAX_TABLE_SIZE: cr_limit = 4;
/// The largest YAML text, in bytes (1 MiB by default: the YAML parser holds a
/// flow collection whole).
pub const CR_LIMIT_MAX_YAML_SIZE: cr_limit = 5;
/// The largest text in the rule syntax or CSV, in bytes.
pub const CR_LIMIT_MAX_TEXT_SIZE: cr_limit = 6;

/// Limits for reading documents and inputs. Functions that take a
/// `const cr_limits *` use the defaults for `NULL`. Unlike the other handles,
/// limits change: set them before sharing them between threads.
pub struct cr_limits {
    pub(crate) limits: Limits,
}

/// New limits, with the defaults. Free them with `cr_limits_free`.
#[unsafe(no_mangle)]
pub extern "C" fn cr_limits_new() -> *mut cr_limits {
    Box::into_raw(Box::new(cr_limits {
        limits: Limits::default(),
    }))
}

fn which(limit: cr_limit) -> Outcome<cr_limit> {
    if (CR_LIMIT_MAX_SIZE..=CR_LIMIT_MAX_TEXT_SIZE).contains(&limit) {
        Ok(limit)
    } else {
        Err(Failure::Invalid(
            format!("limit {limit} is not a CR_LIMIT_* constant"),
            "use CR_LIMIT_MAX_SIZE (1) to CR_LIMIT_MAX_TEXT_SIZE (6)",
        ))
    }
}

/// Set one limit (`CR_LIMIT_*`).
///
/// # Safety
///
/// `limits` is `NULL` or a live `cr_limits` that no other thread uses during
/// the call; `error` is `NULL` or points to a writable `cr_error *` set to
/// `NULL`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cr_limits_set(
    limits: *mut cr_limits,
    limit: cr_limit,
    value: usize,
    error: *mut *mut cr_error,
) -> cr_status {
    let body = || {
        // SAFETY: `limits` is live and used by this call only (# Safety).
        let l = &mut unsafe { handle_mut(limits, "limits") }?.limits;
        match which(limit)? {
            CR_LIMIT_MAX_SIZE => *l = l.with_max_size(value),
            CR_LIMIT_MAX_NESTING => l.max_nesting = value,
            CR_LIMIT_MAX_DEPTH => l.max_depth = value,
            CR_LIMIT_MAX_TABLE_SIZE => l.max_table_size = value,
            CR_LIMIT_MAX_YAML_SIZE => l.max_yaml_size = value,
            _ => l.max_text_size = value,
        }
        Ok(())
    };
    // SAFETY: `error` is NULL or writable (# Safety).
    unsafe { call("cr_limits_set", error, body) }
}

/// Read one limit (`CR_LIMIT_*`) into `value`.
///
/// # Safety
///
/// `limits` is `NULL` or a live `cr_limits`; `value` is `NULL` or points to a
/// writable `size_t`; `error` is `NULL` or points to a writable `cr_error *`
/// set to `NULL`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cr_limits_get(
    limits: *const cr_limits,
    limit: cr_limit,
    value: *mut usize,
    error: *mut *mut cr_error,
) -> cr_status {
    let body = || {
        // SAFETY: `limits` is NULL or live (# Safety).
        let l = &unsafe { handle(limits, "limits") }?.limits;
        let read = match which(limit)? {
            CR_LIMIT_MAX_SIZE => l.max_size,
            CR_LIMIT_MAX_NESTING => l.max_nesting,
            CR_LIMIT_MAX_DEPTH => l.max_depth,
            CR_LIMIT_MAX_TABLE_SIZE => l.max_table_size,
            CR_LIMIT_MAX_YAML_SIZE => l.max_yaml_size,
            _ => l.max_text_size,
        };
        // SAFETY: `value` is NULL or writable (# Safety).
        unsafe { write_out(value, read, "value") }
    };
    // SAFETY: `error` is NULL or writable (# Safety).
    unsafe { call("cr_limits_get", error, body) }
}

/// Free limits. `NULL` is ignored.
///
/// # Safety
///
/// `limits` is `NULL` or a `cr_limits` from this library, not freed before.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cr_limits_free(limits: *mut cr_limits) {
    // SAFETY: the caller's promise is `free`'s.
    unsafe { free(limits) };
}
