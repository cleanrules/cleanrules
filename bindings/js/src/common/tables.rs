//! Decision tables (`spec/tables.md`) for the idiomatic API
//! (`lib/api/tables.js`): a table read from a document or built from a grid,
//! and what JavaScript reads of it. The core reads, checks, compiles and
//! writes tables, as in Python (`bindings/python/src/tables.rs`).
//!
//! A grid (`spec/tables.md`, *Grids*) arrives one item at a time, because
//! wasm-bindgen cannot borrow the nodes inside an array: the settings first,
//! then the header's items, then each row's cells, each row pushed to the
//! core as it ends, as Python reads rows one at a time.

use cleanrules::ast::Node;
use cleanrules::format::{Document, DocumentFormat, write_document_as};
use cleanrules::json::{Json, Limits, to_string};
use cleanrules::logic::LogicName;
use cleanrules::table::{
    Cell, DecisionTable, GridBuilder, HeaderItem, TableConfig, TableOutput, TablePolicy,
};
use cleanrules::{Error, ErrorCode, Value};

use super::build::{aggregate_of, data, definition_plain, name_or, not_constant};
use super::core::RulesCore;
use super::{Outcome, Problem, Raw, RawLimits, limits, out};

/// A decision table, immutable.
pub(crate) struct TableCore {
    pub(crate) table: DecisionTable,
}

impl TableCore {
    /// `[hit policy, aggregate or null, logic, strict]`, as JSON.
    pub(crate) fn config(&self) -> String {
        let config = self.table.config();
        to_string(&Json::Array(vec![
            Json::String(config.hit_policy.as_str().to_owned()),
            config
                .aggregate
                .map_or(Json::Null, |name| Json::String(name.as_str().to_owned())),
            Json::String(config.logic.map_or("boolean", LogicName::as_str).to_owned()),
            Json::Bool(config.strict),
        ]))
    }

    /// The input expressions, in column order.
    pub(crate) fn inputs(&self) -> Vec<Node> {
        self.table.inputs().to_vec()
    }

    /// The output columns as JSON: `[[name, [value, ...]], ...]`, the values in
    /// `out`'s encoding.
    pub(crate) fn outputs(&self) -> String {
        let outputs = self
            .table
            .outputs()
            .iter()
            .map(|output| {
                Json::Array(vec![
                    Json::String(output.name.clone()),
                    Json::Array(output.values.iter().map(out::value).collect()),
                ])
            })
            .collect();
        to_string(&Json::Array(outputs))
    }

    /// The rows as JSON: `[[name, priority, description, inputs, outputs],
    /// ...]`, with `null` for what a row lacks, the priority in `out`'s
    /// encoding, and each cell `true` when it holds a node (in `row_nodes`)
    /// and `false` when it is empty.
    pub(crate) fn rows(&self) -> String {
        let present = |cells: &[Option<Node>]| {
            Json::Array(
                cells
                    .iter()
                    .map(|cell| Json::Bool(cell.is_some()))
                    .collect(),
            )
        };
        let text =
            |text: Option<&String>| text.map_or(Json::Null, |text| Json::String(text.clone()));
        let rows = self
            .table
            .rows()
            .iter()
            .map(|row| {
                Json::Array(vec![
                    text(row.name.as_ref()),
                    row.priority.map_or(Json::Null, out::int),
                    text(row.description.as_ref()),
                    present(&row.inputs),
                    present(&row.outputs),
                ])
            })
            .collect();
        to_string(&Json::Array(rows))
    }

    /// The nodes of the rows' cells, row by row, inputs before outputs,
    /// without the empty ones.
    pub(crate) fn row_nodes(&self) -> Vec<Node> {
        self.table
            .rows()
            .iter()
            .flat_map(|row| row.inputs.iter().chain(&row.outputs))
            .flatten()
            .cloned()
            .collect()
    }

    pub(crate) fn row_count(&self) -> usize {
        self.table.rows().len()
    }

    /// The rule set the table compiles into.
    pub(crate) fn ruleset(&self) -> RulesCore {
        RulesCore::new(self.table.ruleset().clone())
    }

    /// The table document in `format`: JSON (the default), YAML or CSV.
    pub(crate) fn write(&self, format: &Raw) -> Outcome<String> {
        let format = super::format(format, DocumentFormat::Json)?;
        Ok(write_document_as(
            &Document::DecisionTable(self.table.clone()),
            format,
        )?)
    }
}

/// The settings of a grid as JavaScript gives them: the hit policy (`"first"`
/// when left out) and aggregate in any case, the logic (`"boolean"`) and
/// `strict` (`false`), with the errors of a rule set's settings.
fn config(hit_policy: &Raw, aggregate: &Raw, logic: &Raw, strict: &Raw) -> Outcome<TableConfig> {
    let policy = name_or(hit_policy, "first", "hitPolicy")?.unwrap_or("first");
    let hit_policy = TablePolicy::from_name(&policy.to_lowercase()).ok_or_else(|| {
        definition_plain(
            format!("unknown hit policy {policy:?}"),
            "use one of: first, priority, unique, any, rule_order, output_order, collect",
        )
    })?;
    let aggregate = aggregate_of(aggregate)?;
    // Boolean logic is the default, which documents leave out.
    let logic = match name_or(logic, "boolean", "logic")? {
        None | Some("boolean") => None,
        Some(name) => Some(LogicName::from_name(name)?),
    };
    let strict = match strict {
        Raw::Undefined | Raw::Null => false,
        Raw::Bool(strict) => *strict,
        other => {
            return Err(definition_plain(
                format!("strict must be a boolean, not {}", other.describe()),
                "",
            ));
        }
    };
    Ok(TableConfig {
        hit_policy,
        aggregate,
        logic,
        strict,
    })
}

/// Check that a value of a grid is a constant; `at` locates the error.
fn check_constant(value: &Value, at: impl FnOnce() -> String) -> Outcome<()> {
    match not_constant(value) {
        Some((message, hint)) => Err(Problem::Core(
            Error::new(ErrorCode::ExpressionError, message)
                .with_hint(hint)
                .at_pointer(at()),
        )),
        None => Ok(()),
    }
}

enum Stage {
    /// Reading the header's items.
    Header(Vec<HeaderItem>),
    /// Reading the rows.
    Rows(GridBuilder),
    /// Finished, or failed: the grid cannot be used again.
    Done,
}

/// A table being built from a grid.
pub(crate) struct GridCore {
    config: TableConfig,
    limits: Limits,
    stage: Stage,
    /// The cells of the row being read.
    row: Vec<Cell>,
    /// How many rows were pushed.
    rows: usize,
}

fn used() -> Problem {
    Problem::Internal("the grid was used out of order".to_owned())
}

impl GridCore {
    /// A grid with its settings, checked before anything else, and the limits
    /// of its texts and of the table.
    pub(crate) fn new(
        hit_policy: &Raw,
        aggregate: &Raw,
        logic: &Raw,
        strict: &Raw,
        raw_limits: &RawLimits,
    ) -> Outcome<Self> {
        Ok(Self {
            config: config(hit_policy, aggregate, logic, strict)?,
            limits: limits(raw_limits)?,
            stage: Stage::Header(Vec::new()),
            row: Vec::new(),
            rows: 0,
        })
    }

    fn items(&mut self) -> Outcome<&mut Vec<HeaderItem>> {
        match &mut self.stage {
            Stage::Header(items) => Ok(items),
            _ => Err(used()),
        }
    }

    /// The next header item: text or an input node.
    pub(crate) fn header(&mut self, item: HeaderItem) -> Outcome<()> {
        self.items()?.push(item);
        Ok(())
    }

    /// The next header item: an output, with its values as a list in the
    /// reader's encoding.
    pub(crate) fn header_output(&mut self, name: String, wire: &str) -> Outcome<()> {
        let items = self.items()?;
        let column = items.len();
        let Value::List(values) = data(wire)? else {
            return Err(Problem::Internal(
                "an output's values are a list".to_owned(),
            ));
        };
        for value in &values {
            check_constant(value, || format!("/header/{column}"))?;
        }
        items.push(HeaderItem::Output(TableOutput::new(name).values(values)));
        Ok(())
    }

    /// Read the header: its columns are checked before any row.
    pub(crate) fn start(&mut self) -> Outcome<()> {
        let Stage::Header(items) = std::mem::replace(&mut self.stage, Stage::Done) else {
            return Err(used());
        };
        self.stage = Stage::Rows(DecisionTable::grid(self.config, items, &self.limits)?);
        Ok(())
    }

    /// The next cell of the row being read: text or a node.
    pub(crate) fn cell(&mut self, cell: Cell) {
        self.row.push(cell);
    }

    /// The next cell of the row being read: a value, in the reader's
    /// encoding (`null` for an empty cell).
    pub(crate) fn cell_value(&mut self, wire: &str) -> Outcome<()> {
        let (row, column) = (self.rows, self.row.len());
        let value = data(wire)?;
        check_constant(&value, || format!("/rows/{row}/{column}"))?;
        self.row.push(Cell::Value(value));
        Ok(())
    }

    /// End the row being read and push it to the core.
    pub(crate) fn end_row(&mut self) -> Outcome<()> {
        let Stage::Rows(builder) = &mut self.stage else {
            return Err(used());
        };
        let cells = std::mem::take(&mut self.row);
        self.rows += 1;
        if let Err(error) = builder.push_row(cells) {
            // A failed row leaves the table unfinished.
            self.stage = Stage::Done;
            return Err(error.into());
        }
        Ok(())
    }

    /// The table, checked and compiled.
    pub(crate) fn finish(&mut self) -> Outcome<TableCore> {
        let Stage::Rows(builder) = std::mem::replace(&mut self.stage, Stage::Done) else {
            return Err(used());
        };
        Ok(TableCore {
            table: builder.finish()?,
        })
    }
}
