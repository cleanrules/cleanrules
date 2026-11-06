//! Decision tables built from a grid (`spec/tables.md`, *Grids*): a header and
//! rows of cells, pushed one row at a time; and what a table holds.

use std::ffi::c_char;

use cleanrules::format::Document;
use cleanrules::json::Limits;
use cleanrules::table::{Cell, DecisionTable, GridBuilder, HeaderItem, TableConfig, TableOutput};
use cleanrules::{Map, NullCause, Value};

use crate::boundary::{
    Failure, Outcome, bytes, call, clear, free, give, handle, handle_mut, limits_of, utf8_arg,
};
use crate::construct::build;
use crate::cr_status;
use crate::document::{cr_document, not_holding};
use crate::error::cr_error;
use crate::limits::cr_limits;
use crate::node::cr_node;
use crate::value::{borrow_value, cr_value, give_new_value};

/// Where a grid is: taking its header's columns, or its rows.
enum Stage {
    /// The header is open (`cr_grid_new_open`): columns come one at a time.
    Header {
        config: TableConfig,
        items: Vec<HeaderItem>,
        limits: Limits,
    },
    /// The header was read; rows come next.
    Rows(GridBuilder),
    /// Reading the header failed.
    Spoiled,
}

/// A decision table being built, row by row.
pub struct cr_grid {
    stage: Stage,
    /// A row failed: the table would lack it, so the grid can only be freed.
    failed: bool,
}

impl cr_grid {
    /// The builder of rows, reading the header first if it is still open. A
    /// header that fails spoils the grid.
    fn rows(&mut self) -> Outcome<&mut GridBuilder> {
        if self.failed {
            return Err(spoiled());
        }
        if let Stage::Header { .. } = self.stage {
            let Stage::Header {
                config,
                items,
                limits,
            } = std::mem::replace(&mut self.stage, Stage::Spoiled)
            else {
                return Err(Failure::Internal(
                    "a grid lost its header; please report it".into(),
                ));
            };
            match DecisionTable::grid(config, items, &limits) {
                Ok(builder) => self.stage = Stage::Rows(builder),
                Err(error) => {
                    self.failed = true;
                    return Err(error.into());
                }
            }
        }
        match &mut self.stage {
            Stage::Rows(builder) => Ok(builder),
            _ => Err(spoiled()),
        }
    }

    /// The open header's columns, to add one.
    fn header(&mut self) -> Outcome<&mut Vec<HeaderItem>> {
        if self.failed {
            return Err(spoiled());
        }
        match &mut self.stage {
            Stage::Header { items, .. } => Ok(items),
            _ => Err(Failure::Invalid(
                "the grid's header is closed: it was given to cr_grid_new, or a row came".into(),
                "add every column with cr_grid_add_column_* before the first row",
            )),
        }
    }

    /// Push a row of cells, spoiling the grid if it fails.
    fn push(&mut self, cells: Vec<Cell>) -> Outcome {
        let builder = self.rows()?;
        let pushed = builder.push_row(cells);
        pushed.map_err(|error| {
            self.failed = true;
            Failure::from(error)
        })
    }
}

fn spoiled() -> Failure {
    Failure::Invalid(
        "a row or the header of this grid failed, so its table would lack it".into(),
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

/// The table's settings from their names (`cr_grid_new`).
///
/// # Safety
///
/// Each name is `NULL` or points to its length's readable bytes.
unsafe fn config(
    hit_policy: (*const c_char, usize),
    aggregate: (*const c_char, usize),
    logic: (*const c_char, usize),
    strict: bool,
) -> Outcome<TableConfig> {
    // SAFETY: each name and its length (# Safety).
    let (policy, aggregate, logic) = unsafe {
        (
            name(hit_policy.0, hit_policy.1, "hit_policy")?,
            name(aggregate.0, aggregate.1, "aggregate")?,
            name(logic.0, logic.1, "logic")?,
        )
    };
    Ok(TableConfig::from_names(
        policy.unwrap_or("first"),
        aggregate,
        logic,
        strict,
    )?)
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
        let config = unsafe {
            self::config(
                (hit_policy, hit_policy_len),
                (aggregate, aggregate_len),
                (logic, logic_len),
                strict,
            )
        }?;
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
                    stage: Stage::Rows(builder),
                    failed: false,
                },
            );
        };
        Ok(())
    };
    // SAFETY: `error` is NULL or writable (# Safety).
    unsafe { call("cr_grid_new", error, body) }
}

/// A cell of a row given as a value: a string is text, anything else a value.
fn cell(value: &Value) -> Cell {
    match value {
        Value::String(text) => Cell::Text(text.clone()),
        other => Cell::Value(other.clone()),
    }
}

/// Start a table from its settings, as `cr_grid_new` does, with its header
/// open: add its columns in order with `cr_grid_add_column_text`,
/// `cr_grid_add_column_input` and `cr_grid_add_column_output`, then its rows.
/// The header is read, and checked, when the first row comes or the grid is
/// finished; its errors then point at `/header/<c>`.
///
/// # Safety
///
/// Each name is `NULL` or points to its length's readable bytes; `limits` is
/// `NULL` or a live `cr_limits`; `out` is `NULL` or points to a writable
/// `cr_grid *`; `error` is `NULL` or points to a writable `cr_error *` set to
/// `NULL`.
#[unsafe(no_mangle)]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn cr_grid_new_open(
    hit_policy: *const c_char,
    hit_policy_len: usize,
    aggregate: *const c_char,
    aggregate_len: usize,
    logic: *const c_char,
    logic_len: usize,
    strict: bool,
    limits: *const cr_limits,
    out: *mut *mut cr_grid,
    error: *mut *mut cr_error,
) -> cr_status {
    let body = || {
        // SAFETY: `out` is NULL or writable (# Safety).
        unsafe { clear(out, "out") }?;
        // SAFETY: each name and its length (# Safety).
        let config = unsafe {
            self::config(
                (hit_policy, hit_policy_len),
                (aggregate, aggregate_len),
                (logic, logic_len),
                strict,
            )
        }?;
        // SAFETY: `limits` is NULL or live (# Safety).
        let limits = unsafe { limits_of(limits) };
        let stage = Stage::Header {
            config,
            items: Vec::new(),
            limits,
        };
        // SAFETY: `out` passed `clear`.
        unsafe {
            give(
                out,
                cr_grid {
                    stage,
                    failed: false,
                },
            );
        };
        Ok(())
    };
    // SAFETY: `error` is NULL or writable (# Safety).
    unsafe { call("cr_grid_new_open", error, body) }
}

/// Add a column to an open header.
///
/// # Safety
///
/// `grid` is `NULL` or a live `cr_grid` that no other thread uses during the
/// call; `error` is `NULL` or points to a writable `cr_error *` set to `NULL`.
unsafe fn add_column(
    name: &str,
    grid: *mut cr_grid,
    error: *mut *mut cr_error,
    item: impl FnOnce() -> Outcome<HeaderItem>,
) -> cr_status {
    let body = || {
        // SAFETY: `grid` is NULL or live and used by this call only (# Safety).
        let grid = unsafe { handle_mut(grid, "grid") }?;
        let items = grid.header()?;
        items.push(item()?);
        Ok(())
    };
    // SAFETY: `error` is NULL or writable (# Safety).
    unsafe { call(name, error, body) }
}

/// Add a column to an open header as its CSV header text: `RULE`,
/// `PRIORITY`, `DESCRIPTION`, `THEN key [IN (...)]` or an input expression.
/// After the header is closed (by `cr_grid_new`, or by a row), adding a
/// column is `CR_INVALID_ARGUMENT`.
///
/// # Safety
///
/// `grid` is `NULL` or a live `cr_grid` that no other thread uses during the
/// call; `text` is `NULL` or points to `len` readable bytes; `error` is `NULL`
/// or points to a writable `cr_error *` set to `NULL`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cr_grid_add_column_text(
    grid: *mut cr_grid,
    text: *const c_char,
    len: usize,
    error: *mut *mut cr_error,
) -> cr_status {
    let item = || {
        // SAFETY: `text` and `len` (# Safety).
        let text = utf8_arg(unsafe { bytes(text, len, "text") }?, "text")?;
        Ok(HeaderItem::Text(text.to_owned()))
    };
    // SAFETY: the caller's promises are `add_column`'s.
    unsafe { add_column("cr_grid_add_column_text", grid, error, item) }
}

/// Add an input column to an open header: its expression, a value node such
/// as a field (copied). A condition is a `CR_EXPRESSION_ERROR` when the header
/// is read.
///
/// # Safety
///
/// `grid` is `NULL` or a live `cr_grid` that no other thread uses during the
/// call; `input` is `NULL` or a live `cr_node`; `error` is `NULL` or points to
/// a writable `cr_error *` set to `NULL`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cr_grid_add_column_input(
    grid: *mut cr_grid,
    input: *const cr_node,
    error: *mut *mut cr_error,
) -> cr_status {
    let item = || {
        // SAFETY: `input` is NULL or live (# Safety).
        let input = unsafe { handle(input, "input") }?;
        Ok(HeaderItem::Input(input.node.clone()))
    };
    // SAFETY: the caller's promises are `add_column`'s.
    unsafe { add_column("cr_grid_add_column_input", grid, error, item) }
}

/// Add an output column to an open header: its payload key and its output
/// values, highest priority first, as the column `THEN key IN (...)` holds
/// them; `values` is a list (copied), or `NULL` for any value. Values that
/// are not scalars, or repeat, are a `CR_RULE_DEFINITION_ERROR` when the
/// header is read.
///
/// # Safety
///
/// `grid` is `NULL` or a live `cr_grid` that no other thread uses during the
/// call; `name` is `NULL` or points to `len` readable bytes; `values` is
/// `NULL` or a live `cr_value`; `error` is `NULL` or points to a writable
/// `cr_error *` set to `NULL`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cr_grid_add_column_output(
    grid: *mut cr_grid,
    name: *const c_char,
    len: usize,
    values: *const cr_value,
    error: *mut *mut cr_error,
) -> cr_status {
    let item = || {
        // SAFETY: `name` and `len` (# Safety).
        let key = utf8_arg(unsafe { bytes(name, len, "name") }?, "name")?;
        let output = TableOutput::new(key);
        if values.is_null() {
            return Ok(HeaderItem::Output(output));
        }
        // SAFETY: `values` is live (# Safety).
        let list = items(unsafe { borrow_value(values, "values") }?, "values")?;
        Ok(HeaderItem::Output(output.values(list.to_vec())))
    };
    // SAFETY: the caller's promises are `add_column`'s.
    unsafe { add_column("cr_grid_add_column_output", grid, error, item) }
}

/// Add a row whose cells may be nodes: cell `c` is `nodes[c]` when it is not
/// `NULL` (an input's condition, which tests its column, or an output's
/// value; copied), and the row's item `c` otherwise, read as
/// `cr_grid_push_row` reads it. `nodes_len` is the row's length. As with
/// `cr_grid_push_row`, after a row fails the grid can only be freed.
///
/// # Safety
///
/// `grid` is `NULL` or a live `cr_grid` that no other thread uses during the
/// call; `row` is `NULL` or a live `cr_value`; `nodes` is `NULL` or points to
/// `nodes_len` readable pointers, each `NULL` or a live `cr_node`; `error` is
/// `NULL` or points to a writable `cr_error *` set to `NULL`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cr_grid_push_row_nodes(
    grid: *mut cr_grid,
    row: *const cr_value,
    nodes: *const *const cr_node,
    nodes_len: usize,
    error: *mut *mut cr_error,
) -> cr_status {
    let body = || {
        // SAFETY: `grid` is NULL or live and used by this call only (# Safety).
        let grid = unsafe { handle_mut(grid, "grid") }?;
        // SAFETY: `row` is NULL or live (# Safety).
        let values = items(unsafe { borrow_value(row, "row") }?, "row")?;
        if nodes_len != values.len() {
            return Err(Failure::Invalid(
                format!(
                    "nodes_len is {nodes_len}, but the row has {} cells",
                    values.len()
                ),
                "pass one node pointer per cell, NULL for a cell given in the row",
            ));
        }
        let pointers: &[*const cr_node] = if nodes_len == 0 {
            &[]
        } else if nodes.is_null() {
            return Err(Failure::Invalid(
                format!("nodes is NULL with a length of {nodes_len}"),
                "pass an array of node pointers, NULL for a cell given in the row",
            ));
        } else {
            // SAFETY: the caller promises `nodes_len` readable pointers.
            unsafe { std::slice::from_raw_parts(nodes, nodes_len) }
        };
        let cells = values
            .iter()
            .zip(pointers)
            .map(|(value, &node)| {
                // SAFETY: each pointer is NULL or a live node (# Safety).
                match unsafe { node.as_ref() } {
                    Some(node) => Cell::Node(node.node.clone()),
                    None => cell(value),
                }
            })
            .collect();
        grid.push(cells)
    };
    // SAFETY: `error` is NULL or writable (# Safety).
    unsafe { call("cr_grid_push_row_nodes", error, body) }
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
        // SAFETY: `row` is NULL or live (# Safety).
        let cells = items(unsafe { borrow_value(row, "row") }?, "row")?
            .iter()
            .map(cell)
            .collect();
        grid.push(cells)
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
        let mut grid = *unsafe { Box::from_raw(grid) };
        cleared?;
        grid.rows()?;
        let Stage::Rows(builder) = grid.stage else {
            return Err(spoiled());
        };
        let document = Document::DecisionTable(builder.finish()?);
        // SAFETY: `out` passed `clear`.
        unsafe { give(out, cr_document { document }) };
        Ok(())
    };
    // SAFETY: `error` is NULL or writable (# Safety).
    unsafe { call("cr_grid_finish", error, body) }
}

/// The decision table a `CR_DOCUMENT_DECISION_TABLE` document holds; a
/// document of another kind is `CR_INVALID_ARGUMENT`.
///
/// # Safety
///
/// `document` is `NULL` or a live `cr_document`.
unsafe fn table<'a>(document: *const cr_document) -> Outcome<&'a DecisionTable> {
    // SAFETY: `document` is NULL or live (# Safety).
    match &unsafe { handle(document, "document") }?.document {
        Document::DecisionTable(table) => Ok(table),
        other => Err(not_holding("a decision table", other)),
    }
}

fn text_or_null(text: Option<&str>) -> Value {
    text.map_or(Value::Null(NullCause::Null), |t| {
        Value::String(t.to_owned())
    })
}

/// What a decision table holds apart from its cells (`spec/tables.md`,
/// *Model*), as a new map freed with `cr_value_free`: `hit_policy` (a
/// table's, `output_order` included), `aggregate` (or null), `logic` (its
/// name, `boolean` when the table names none), `strict`, `input_count` (the
/// inputs, read with `cr_document_table_input`), `outputs` (a list of maps
/// with `name` and `values`, highest priority first, empty for any value) and
/// `row_count` (the rows, read with `cr_document_table_row` and
/// `cr_document_table_cell`). Maps may gain keys in later versions.
///
/// # Safety
///
/// `document` is `NULL` or a live `cr_document`; `out` is `NULL` or points to
/// a writable `cr_value *`; `error` is `NULL` or points to a writable
/// `cr_error *` set to `NULL`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cr_document_table_value(
    document: *const cr_document,
    out: *mut *mut cr_value,
    error: *mut *mut cr_error,
) -> cr_status {
    let body = || {
        // SAFETY: `document` is NULL or live (# Safety).
        let table = unsafe { table(document) }?;
        let config = table.config();
        let mut map = Map::new();
        map.insert(
            "hit_policy",
            Value::String(config.hit_policy.as_str().into()),
        );
        map.insert(
            "aggregate",
            text_or_null(config.aggregate.map(cleanrules::rules::Aggregate::as_str)),
        );
        let logic = config.logic.map_or("boolean", |l| l.as_str());
        map.insert("logic", Value::String(logic.into()));
        map.insert("strict", Value::Bool(config.strict));
        let count = |n: usize| Value::Int(i64::try_from(n).unwrap_or(i64::MAX));
        map.insert("input_count", count(table.inputs().len()));
        let outputs = table
            .outputs()
            .iter()
            .map(|output| {
                let mut column = Map::new();
                column.insert("name", Value::String(output.name.clone()));
                column.insert("values", Value::List(output.values.clone()));
                Value::Map(column)
            })
            .collect();
        map.insert("outputs", Value::List(outputs));
        map.insert("row_count", count(table.rows().len()));
        // SAFETY: `out` is NULL or writable (# Safety).
        unsafe { give_new_value(out, Value::Map(map)) }
    };
    // SAFETY: `error` is NULL or writable (# Safety).
    unsafe { call("cr_document_table_value", error, body) }
}

/// The row at position `row` of a decision table, apart from its cells, as a
/// new map freed with `cr_value_free`: `name`, `priority` and `description`,
/// each null when the row has none (a row without a name is the rule
/// `row_<n>`, counting from 1).
///
/// # Safety
///
/// `document` is `NULL` or a live `cr_document`; `out` is `NULL` or points to
/// a writable `cr_value *`; `error` is `NULL` or points to a writable
/// `cr_error *` set to `NULL`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cr_document_table_row(
    document: *const cr_document,
    row: usize,
    out: *mut *mut cr_value,
    error: *mut *mut cr_error,
) -> cr_status {
    let body = || {
        // SAFETY: `document` is NULL or live (# Safety).
        let table = unsafe { table(document) }?;
        let found = table_row(table, row)?;
        let mut map = Map::new();
        map.insert("name", text_or_null(found.name.as_deref()));
        map.insert(
            "priority",
            found
                .priority
                .map_or(Value::Null(NullCause::Null), Value::Int),
        );
        map.insert("description", text_or_null(found.description.as_deref()));
        // SAFETY: `out` is NULL or writable (# Safety).
        unsafe { give_new_value(out, Value::Map(map)) }
    };
    // SAFETY: `error` is NULL or writable (# Safety).
    unsafe { call("cr_document_table_row", error, body) }
}

fn table_row(table: &DecisionTable, row: usize) -> Outcome<&cleanrules::table::TableRow> {
    let rows = table.rows();
    rows.get(row).ok_or_else(|| {
        Failure::Invalid(
            format!("row {row} is out of the table's {}", rows.len()),
            "pass a position below row_count (cr_document_table_value)",
        )
    })
}

/// A copy of the expression of the input at position `input`, freed with
/// `cr_node_free`.
///
/// # Safety
///
/// `document` is `NULL` or a live `cr_document`; `out` is `NULL` or points to
/// a writable `cr_node *`; `error` is `NULL` or points to a writable
/// `cr_error *` set to `NULL`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cr_document_table_input(
    document: *const cr_document,
    input: usize,
    out: *mut *mut cr_node,
    error: *mut *mut cr_error,
) -> cr_status {
    let make = || {
        // SAFETY: `document` is NULL or live (# Safety).
        let inputs = unsafe { table(document) }?.inputs();
        inputs.get(input).cloned().ok_or_else(|| {
            Failure::Invalid(
                format!("input {input} is out of the table's {}", inputs.len()),
                "pass a position below input_count (cr_document_table_value)",
            )
        })
    };
    // SAFETY: `out` and `error` (# Safety).
    unsafe { build("cr_document_table_input", out, error, make) }
}

/// A copy of the node of the cell at row `row` and column `column`, freed
/// with `cr_node_free`, counting the inputs first, then the outputs: an
/// input's condition, which tests its column, or an output's value; `*out` is
/// `NULL` for an empty cell (any value, or no output).
///
/// # Safety
///
/// `document` is `NULL` or a live `cr_document`; `out` is `NULL` or points to
/// a writable `cr_node *`; `error` is `NULL` or points to a writable
/// `cr_error *` set to `NULL`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cr_document_table_cell(
    document: *const cr_document,
    row: usize,
    column: usize,
    out: *mut *mut cr_node,
    error: *mut *mut cr_error,
) -> cr_status {
    let body = || {
        // SAFETY: `out` is NULL or writable (# Safety).
        unsafe { clear(out, "out") }?;
        // SAFETY: `document` is NULL or live (# Safety).
        let table = unsafe { table(document) }?;
        let found = table_row(table, row)?;
        let inputs = found.inputs.len();
        let cell = if column < inputs {
            &found.inputs[column]
        } else {
            found.outputs.get(column - inputs).ok_or_else(|| {
                Failure::Invalid(
                    format!(
                        "column {column} is out of the table's {}",
                        inputs + found.outputs.len()
                    ),
                    "pass a column below input_count plus the number of outputs",
                )
            })?
        };
        if let Some(node) = cell {
            // SAFETY: `out` passed `clear`.
            unsafe { give(out, cr_node { node: node.clone() }) };
        }
        Ok(())
    };
    // SAFETY: `error` is NULL or writable (# Safety).
    unsafe { call("cr_document_table_cell", error, body) }
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
