//! Decision tables (`spec/tables.md`, task D.3a): the core reads, checks,
//! compiles and writes them; Python wraps the result.

use cleanrules::json::Limits;
use cleanrules::logic::LogicName;
use cleanrules::rules::Aggregate;
use cleanrules::table::{Cell, DecisionTable, HeaderItem, TableConfig, TableOutput, TablePolicy};
use cleanrules::{Error, ErrorCode};
use pyo3::prelude::*;
use pyo3::types::{PyList, PyString, PyTuple};

use crate::convert;
use crate::documents::ruleset_parts;
use crate::errors::IntoPyResult;
use crate::node::PyNode;

/// A decision table of the core, immutable, with its compiled rule set.
#[pyclass(frozen, name = "DecisionTable", module = "cleanrules._cleanrules")]
pub(crate) struct PyDecisionTable {
    pub(crate) table: DecisionTable,
}

#[pymethods]
impl PyDecisionTable {
    /// `(hit_policy, aggregate, logic, strict)`.
    fn config(
        &self,
    ) -> (
        &'static str,
        Option<&'static str>,
        Option<&'static str>,
        bool,
    ) {
        let config = self.table.config();
        (
            config.hit_policy.as_str(),
            config.aggregate.map(cleanrules::rules::Aggregate::as_str),
            config.logic.map(cleanrules::logic::LogicName::as_str),
            config.strict,
        )
    }

    /// The input expressions.
    fn inputs(&self) -> Vec<PyNode> {
        self.table
            .inputs()
            .iter()
            .map(|node| PyNode { node: node.clone() })
            .collect()
    }

    /// Each output as `(name, values)`.
    fn outputs<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyList>> {
        let list = PyList::empty(py);
        for output in self.table.outputs() {
            let values = output
                .values
                .iter()
                .map(|value| convert::to_py(py, value))
                .collect::<PyResult<Vec<_>>>()?;
            list.append((output.name.as_str(), PyTuple::new(py, values)?))?;
        }
        Ok(list)
    }

    /// Each row as `(name, priority, description, inputs, outputs)`, cells as
    /// nodes or `None`.
    #[allow(clippy::type_complexity)]
    fn rows(
        &self,
    ) -> Vec<(
        Option<String>,
        Option<i64>,
        Option<String>,
        Vec<Option<PyNode>>,
        Vec<Option<PyNode>>,
    )> {
        let cells = |cells: &[Option<cleanrules::ast::Node>]| {
            cells
                .iter()
                .map(|cell| cell.as_ref().map(|node| PyNode { node: node.clone() }))
                .collect()
        };
        self.table
            .rows()
            .iter()
            .map(|row| {
                (
                    row.name.clone(),
                    row.priority,
                    row.description.clone(),
                    cells(&row.inputs),
                    cells(&row.outputs),
                )
            })
            .collect()
    }

    /// The number of rows.
    fn row_count(&self) -> usize {
        self.table.rows().len()
    }

    /// The compiled rule set, as `load_document` gives rule sets.
    fn compiled<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyTuple>> {
        ruleset_parts(py, self.table.ruleset())
    }
}

/// A header item: text, an input node, or an output as `(name, values)`.
fn header_item(item: &Bound<'_, PyAny>) -> PyResult<HeaderItem> {
    if let Ok(text) = item.cast::<PyString>() {
        return Ok(HeaderItem::Text(text.to_str()?.to_owned()));
    }
    if let Ok(node) = item.cast::<PyNode>() {
        return Ok(HeaderItem::Input(node.get().node.clone()));
    }
    let (name, values): (String, Vec<Bound<'_, PyAny>>) = item.extract()?;
    let values = values
        .iter()
        .map(convert::constant)
        .collect::<PyResult<Vec<_>>>()?;
    Ok(HeaderItem::Output(TableOutput::new(name).values(values)))
}

/// A cell: text, a node, or a value (`None` for an empty cell).
fn cell(item: &Bound<'_, PyAny>) -> PyResult<Cell> {
    if let Ok(text) = item.cast::<PyString>() {
        return Ok(Cell::Text(text.to_str()?.to_owned()));
    }
    if let Ok(node) = item.cast::<PyNode>() {
        return Ok(Cell::Node(node.get().node.clone()));
    }
    convert::constant(item).map(Cell::Value)
}

fn unknown(what: &str, name: &str) -> Error {
    Error::new(
        ErrorCode::RuleDefinitionError,
        format!("unknown {what} {name:?}"),
    )
}

/// A decision table from a grid (`spec/tables.md`, *Grids*): header items
/// (`str`, a node, or `(name, values)` for an output) and an iterable of rows,
/// each a sequence of cells (`str`, a node, or a constant). The rows are
/// converted and pushed to the core one at a time.
#[pyfunction]
#[pyo3(signature = (header, rows, hit_policy, aggregate, logic, strict, max_size=None, max_depth=None, max_table_size=None))]
#[allow(clippy::too_many_arguments, clippy::needless_pass_by_value)]
pub(crate) fn table_from_grid(
    py: Python<'_>,
    header: Vec<Bound<'_, PyAny>>,
    rows: Bound<'_, PyAny>,
    hit_policy: &str,
    aggregate: Option<&str>,
    logic: Option<&str>,
    strict: bool,
    max_size: Option<usize>,
    max_depth: Option<usize>,
    max_table_size: Option<usize>,
) -> PyResult<PyDecisionTable> {
    let config = TableConfig {
        hit_policy: TablePolicy::from_name(hit_policy)
            .ok_or_else(|| unknown("hit policy", hit_policy))
            .or_raise(py)?,
        aggregate: aggregate
            .map(|name| Aggregate::from_name(name).ok_or_else(|| unknown("aggregate", name)))
            .transpose()
            .or_raise(py)?,
        logic: logic.map(LogicName::from_name).transpose().or_raise(py)?,
        strict,
    };
    let mut limits = Limits::default();
    if let Some(size) = max_size {
        limits.max_text_size = size;
    }
    if let Some(depth) = max_depth {
        limits.max_depth = depth;
    }
    if let Some(size) = max_table_size {
        limits.max_table_size = size;
    }
    let header = header
        .iter()
        .enumerate()
        .map(|(c, item)| {
            header_item(item)
                .map_err(|error| crate::errors::with_pointer(py, error, &format!("/header/{c}")))
        })
        .collect::<PyResult<Vec<_>>>()?;
    let mut grid = DecisionTable::grid(config, header, &limits).or_raise(py)?;
    for (r, row) in rows.try_iter()?.enumerate() {
        let cells = row?
            .try_iter()?
            .enumerate()
            .map(|(c, item)| {
                item.and_then(|item| cell(&item)).map_err(|error| {
                    crate::errors::with_pointer(py, error, &format!("/rows/{r}/{c}"))
                })
            })
            .collect::<PyResult<Vec<_>>>()?;
        grid.push_row(cells).or_raise(py)?;
    }
    Ok(PyDecisionTable {
        table: grid.finish().or_raise(py)?,
    })
}
