//! Decision tables built from a grid (`spec/tables.md`, *Grids*): a header and
//! rows of cells, pushed one row at a time.

use std::ffi::c_char;

use cleanrules::Value;
use cleanrules::format::Document;
use cleanrules::table::{Cell, DecisionTable, GridBuilder, HeaderItem, TableConfig};

use crate::boundary::{
    Failure, Outcome, bytes, call, clear, free, give, handle_mut, limits_of, utf8_arg,
};
use crate::cr_status;
use crate::document::cr_document;
use crate::error::cr_error;
use crate::limits::cr_limits;
use crate::value::{borrow_value, cr_value};

/// A decision table being built, row by row.
pub struct cr_grid {
    builder: GridBuilder,
    /// A row failed: the table would lack it, so the grid can only be freed.
    failed: bool,
}

fn spoiled() -> Failure {
    Failure::Invalid(
        "a row of this grid failed, so its table would lack a rule".into(),
        "free the grid with cr_grid_free and build it again",
    )
}

/// A list argument's items.
fn items<'a>(value: &'a Value, what: &str) -> Outcome<&'a [Value]> {
    match value {
        Value::List(items) => Ok(items),
        _ => Err(Failure::Invalid(
            format!("{what} is not a list"),
            "pass a list made with cr_value_new_list",
        )),
    }
}

/// An optional name argument: `None` for `NULL`.
///
/// # Safety
///
/// `name` is `NULL` or points to `len` readable bytes.
unsafe fn name<'a>(name: *const c_char, len: usize, what: &str) -> Outcome<Option<&'a str>> {
    if name.is_null() {
        return Ok(None);
    }
    // SAFETY: the caller's promise is `bytes`'.
    utf8_arg(unsafe { bytes(name, len, what) }?, what).map(Some)
}

/// Start a table from its settings and its header (`spec/tables.md`,
/// *Grids*). The settings are names, as in JSON: the hit policy (`first` for
/// `NULL`), the aggregate (none for `NULL`) and the logic
/// (`boolean` for `NULL`). The header is a list of strings, each read as a CSV
/// header field: `RULE`, `PRIORITY`, `DESCRIPTION`, `THEN key [IN (...)]` or an
/// input expression.
///
/// # Safety
///
/// Each name is `NULL` or points to its length's readable bytes; `header` is
/// `NULL` or a live `cr_value`; `limits` is `NULL` or a live `cr_limits`; `out`
/// is `NULL` or points to a writable `cr_grid *`; `error` is `NULL` or points to
/// a writable `cr_error *` set to `NULL`.
#[unsafe(no_mangle)]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn cr_grid_new(
    hit_policy: *const c_char,
    hit_policy_len: usize,
    aggregate: *const c_char,
    aggregate_len: usize,
    logic: *const c_char,
    logic_len: usize,
    strict: bool,
    header: *const cr_value,
    limits: *const cr_limits,
    out: *mut *mut cr_grid,
    error: *mut *mut cr_error,
) -> cr_status {
    let body = || {
        // SAFETY: `out` is NULL or writable (# Safety).
        unsafe { clear(out, "out") }?;
        // SAFETY: each name and its length (# Safety).
        let (policy, aggregate, logic) = unsafe {
            (
                name(hit_policy, hit_policy_len, "hit_policy")?,
                name(aggregate, aggregate_len, "aggregate")?,
                name(logic, logic_len, "logic")?,
            )
        };
        let config = TableConfig::from_names(policy.unwrap_or("first"), aggregate, logic, strict)?;
        // SAFETY: `header` is NULL or live (# Safety).
        let header = items(unsafe { borrow_value(header, "header") }?, "header")?;
        let header = header
            .iter()
            .enumerate()
            .map(|(c, item)| match item {
                Value::String(text) => Ok(HeaderItem::Text(text.clone())),
                _ => Err(Failure::Invalid(
                    format!("header item {c} is not a string"),
                    "write each column as its CSV header text, such as THEN risk IN ('high', 'low')",
                )),
            })
            .collect::<Outcome<Vec<_>>>()?;
        // SAFETY: `limits` is NULL or live (# Safety).
        let limits = unsafe { limits_of(limits) };
        let builder = DecisionTable::grid(config, header, &limits)?;
        // SAFETY: `out` passed `clear`.
        unsafe {
            give(
                out,
                cr_grid {
                    builder,
                    failed: false,
                },
            );
        };
        Ok(())
    };
    // SAFETY: `error` is NULL or writable (# Safety).
    unsafe { call("cr_grid_new", error, body) }
}

/// Add a row: a list of cells in column order. A string is text, read as a
/// CSV field (`'VIP'` is the string, `VIP` the field); null is an empty cell;
/// any other value is a constant. The row stays the caller's. After a row
/// fails, the grid can only be freed: adding rows or finishing it is
/// `CR_INVALID_ARGUMENT`.
///
/// # Safety
///
/// `grid` is `NULL` or a live `cr_grid` that no other thread uses during the
/// call; `row` is `NULL` or a live `cr_value`; `error` is `NULL` or points to a
/// writable `cr_error *` set to `NULL`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cr_grid_push_row(
    grid: *mut cr_grid,
    row: *const cr_value,
    error: *mut *mut cr_error,
) -> cr_status {
    let body = || {
        // SAFETY: `grid` is NULL or live and used by this call only (# Safety).
        let grid = unsafe { handle_mut(grid, "grid") }?;
        if grid.failed {
            return Err(spoiled());
        }
        // SAFETY: `row` is NULL or live (# Safety).
        let cells = items(unsafe { borrow_value(row, "row") }?, "row")?
            .iter()
            .map(|cell| match cell {
                Value::String(text) => Cell::Text(text.clone()),
                other => Cell::Value(other.clone()),
            })
            .collect();
        grid.builder.push_row(cells).map_err(|error| {
            grid.failed = true;
            Failure::from(error)
        })
    };
    // SAFETY: `error` is NULL or writable (# Safety).
    unsafe { call("cr_grid_push_row", error, body) }
}

/// Check and compile the table: a document holding it, which
/// `cr_ruleset_from_document` turns into a rule set and `cr_document_write`
/// writes as CSV. The grid is consumed, even when the call fails, and also
/// when one of its rows failed, which is `CR_INVALID_ARGUMENT`.
///
/// # Safety
///
/// `grid` is `NULL` or a `cr_grid` owned by the caller, which gives it up;
/// `out` is `NULL` or points to a writable `cr_document *`; `error` is `NULL`
/// or points to a writable `cr_error *` set to `NULL`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cr_grid_finish(
    grid: *mut cr_grid,
    out: *mut *mut cr_document,
    error: *mut *mut cr_error,
) -> cr_status {
    let body = || {
        // SAFETY: `out` is NULL or writable (# Safety).
        let cleared = unsafe { clear(out, "out") };
        if grid.is_null() {
            return Err(Failure::Invalid(
                "grid is NULL".into(),
                "pass a grid made by cr_grid_new",
            ));
        }
        // SAFETY: the caller gives up a box made by `give`.
        let grid = *unsafe { Box::from_raw(grid) };
        cleared?;
        if grid.failed {
            return Err(spoiled());
        }
        let document = Document::DecisionTable(grid.builder.finish()?);
        // SAFETY: `out` passed `clear`.
        unsafe { give(out, cr_document { document }) };
        Ok(())
    };
    // SAFETY: `error` is NULL or writable (# Safety).
    unsafe { call("cr_grid_finish", error, body) }
}

/// Free a grid that was not finished. `NULL` is ignored.
///
/// # Safety
///
/// `grid` is `NULL` or a `cr_grid` from this library, not freed or finished
/// before.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cr_grid_free(grid: *mut cr_grid) {
    // SAFETY: the caller's promise is `free`'s.
    unsafe { free(grid) };
}
