//! Decision tables (`spec/tables.md`, task D.3a): the core reads, checks,
//! compiles and writes them; Python wraps the result.

use cleanrules::table::DecisionTable;
use pyo3::prelude::*;
use pyo3::types::{PyList, PyTuple};

use crate::convert;
use crate::documents::ruleset_parts;
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
