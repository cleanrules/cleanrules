//! Loading and dumping documents (`spec/json-format.md`), in JSON or YAML
//! (task D.1) and in the text syntax (`spec/text.md`, task D.2a): the core
//! reads and writes; Python builds its objects.

use cleanrules::format::{Document, DocumentFormat, read_document_as, write_document_as};
use cleanrules::json::Limits;
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::{PyList, PyTuple};

use crate::errors::IntoPyResult;
use crate::node::PyNode;
use crate::rules::{PyRule, PyRuleSet};
use crate::tables::PyDecisionTable;
use cleanrules::rules::RuleSet;

fn limits(max_size: Option<usize>, max_nesting: Option<usize>, max_depth: Option<usize>) -> Limits {
    let mut limits = Limits::default();
    // One size limit for the caller: it applies to JSON, YAML and rule texts alike.
    if let Some(size) = max_size {
        limits = limits.with_max_size(size);
    }
    if let Some(nesting) = max_nesting {
        limits.max_nesting = nesting;
    }
    if let Some(depth) = max_depth {
        limits.max_depth = depth;
    }
    limits
}

/// A rule set as `(rules, hit_policy, aggregate, logic, strict)`.
pub(crate) fn ruleset_parts<'py>(
    py: Python<'py>,
    ruleset: &RuleSet,
) -> PyResult<Bound<'py, PyTuple>> {
    let rules = PyList::empty(py);
    for rule in ruleset.rules() {
        rules.append(PyRule::from(rule.clone()))?;
    }
    PyTuple::new(
        py,
        [
            rules.into_any(),
            ruleset.hit_policy().as_str().into_pyobject(py)?.into_any(),
            ruleset
                .aggregate()
                .map(cleanrules::rules::Aggregate::as_str)
                .into_pyobject(py)?
                .into_any(),
            ruleset.logic().as_str().into_pyobject(py)?.into_any(),
            pyo3::types::PyBool::new(py, ruleset.strict())
                .to_owned()
                .into_any(),
        ],
    )
}

/// Read a document: `("condition", Node)`, `("rule", Rule)`,
/// `("ruleset", (rules, hit_policy, aggregate, logic, strict))` or
/// `("decision_table", DecisionTable)`; CSV holds a decision table.
#[pyfunction]
#[pyo3(signature = (text, format, max_size=None, max_nesting=None, max_depth=None, max_table_size=None))]
pub(crate) fn load_document<'py>(
    py: Python<'py>,
    text: &str,
    format: &str,
    max_size: Option<usize>,
    max_nesting: Option<usize>,
    max_depth: Option<usize>,
    max_table_size: Option<usize>,
) -> PyResult<(&'static str, Bound<'py, PyAny>)> {
    let mut limits = limits(max_size, max_nesting, max_depth);
    if let Some(size) = max_table_size {
        limits.max_table_size = size;
    }
    let document =
        read_document_as(text.as_bytes(), document_format(format)?, &limits).or_raise(py)?;
    Ok(match document {
        Document::Condition(node) => ("condition", PyNode { node }.into_pyobject(py)?.into_any()),
        Document::Rule(rule) => ("rule", PyRule::from(rule).into_pyobject(py)?.into_any()),
        Document::RuleSet(ruleset) => ("ruleset", ruleset_parts(py, &ruleset)?.into_any()),
        Document::DecisionTable(table) => (
            "decision_table",
            PyDecisionTable { table }.into_pyobject(py)?.into_any(),
        ),
        // The core's documents are `#[non_exhaustive]`.
        _ => {
            return Err(PyValueError::new_err(
                "this kind of document cannot be loaded yet",
            ));
        }
    })
}

/// Read a text that holds one rule (`cleanrules::text::parse_rule`).
#[pyfunction]
#[pyo3(signature = (text, max_size=None, max_depth=None))]
pub(crate) fn load_rule(
    py: Python<'_>,
    text: &str,
    max_size: Option<usize>,
    max_depth: Option<usize>,
) -> PyResult<PyRule> {
    let limits = limits(max_size, None, max_depth);
    Ok(PyRule::from(
        cleanrules::text::parse_rule(text, &limits).or_raise(py)?,
    ))
}

fn document_format(name: &str) -> PyResult<DocumentFormat> {
    DocumentFormat::from_name(name)
        .ok_or_else(|| PyValueError::new_err(format!("unknown format {name:?}")))
}

fn write(py: Python<'_>, document: &Document, format: &str) -> PyResult<String> {
    write_document_as(document, document_format(format)?).or_raise(py)
}

/// The document of a condition, in `format` (`json`, `yaml` or `text`).
#[pyfunction]
pub(crate) fn dump_condition(node: &Bound<'_, PyNode>, format: &str) -> PyResult<String> {
    write(
        node.py(),
        &Document::Condition(node.get().node.clone()),
        format,
    )
}

/// The document of a rule, in `format`.
#[pyfunction]
pub(crate) fn dump_rule(rule: &Bound<'_, PyRule>, format: &str) -> PyResult<String> {
    write(
        rule.py(),
        &Document::Rule(rule.get().rule().clone()),
        format,
    )
}

/// The document of a rule set, in `format`.
#[pyfunction]
pub(crate) fn dump_ruleset(ruleset: &Bound<'_, PyRuleSet>, format: &str) -> PyResult<String> {
    write(
        ruleset.py(),
        &Document::RuleSet(ruleset.get().ruleset().clone()),
        format,
    )
}

/// The document of a decision table, in `format` (`json`, `yaml` or `csv`).
#[pyfunction]
pub(crate) fn dump_table(table: &Bound<'_, PyDecisionTable>, format: &str) -> PyResult<String> {
    write(
        table.py(),
        &Document::DecisionTable(table.get().table.clone()),
        format,
    )
}
