//! Loading and dumping documents (`spec/json-format.md`), in JSON or YAML
//! (task D.1): the core reads and writes; Python builds its objects.

use cleanrules::format::{Document, read_document, read_yaml_document, write_document};
use cleanrules::json::{Limits, to_string};
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::{PyList, PyTuple};

use crate::errors::IntoPyResult;
use crate::node::PyNode;
use crate::rules::{PyRule, PyRuleSet};

fn limits(max_size: Option<usize>, max_nesting: Option<usize>, max_depth: Option<usize>) -> Limits {
    let mut limits = Limits::default();
    // One size limit for the caller: it applies to JSON and YAML texts alike.
    if let Some(size) = max_size {
        limits.max_size = size;
        limits.max_yaml_size = size;
    }
    if let Some(nesting) = max_nesting {
        limits.max_nesting = nesting;
    }
    if let Some(depth) = max_depth {
        limits.max_depth = depth;
    }
    limits
}

/// Read a document: `("condition", Node)`, `("rule", Rule)` or
/// `("ruleset", (rules, hit_policy, aggregate, logic, strict))`.
#[pyfunction]
#[pyo3(signature = (text, format, max_size=None, max_nesting=None, max_depth=None))]
pub(crate) fn load_document<'py>(
    py: Python<'py>,
    text: &str,
    format: &str,
    max_size: Option<usize>,
    max_nesting: Option<usize>,
    max_depth: Option<usize>,
) -> PyResult<(&'static str, Bound<'py, PyAny>)> {
    let limits = limits(max_size, max_nesting, max_depth);
    let document = match format {
        "json" => read_document(text, &limits),
        "yaml" => read_yaml_document(text, &limits),
        other => return Err(PyValueError::new_err(format!("unknown format {other:?}"))),
    }
    .or_raise(py)?;
    Ok(match document {
        Document::Condition(node) => ("condition", PyNode { node }.into_pyobject(py)?.into_any()),
        Document::Rule(rule) => ("rule", PyRule::from(rule).into_pyobject(py)?.into_any()),
        Document::RuleSet(ruleset) => {
            let rules = PyList::empty(py);
            for rule in ruleset.rules() {
                rules.append(PyRule::from(rule.clone()))?;
            }
            let parts = PyTuple::new(
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
            )?;
            ("ruleset", parts.into_any())
        }
        // The core's documents are `#[non_exhaustive]`.
        _ => {
            return Err(PyValueError::new_err(
                "this kind of document cannot be loaded yet",
            ));
        }
    })
}

fn write(document: &Document, format: &str) -> PyResult<String> {
    let json = write_document(document);
    match format {
        "json" => Ok(to_string(&json)),
        "yaml" => Ok(cleanrules::yaml::to_string(&json)),
        other => Err(PyValueError::new_err(format!("unknown format {other:?}"))),
    }
}

/// The document of a condition, in `format` (`json` or `yaml`).
#[pyfunction]
pub(crate) fn dump_condition(node: &Bound<'_, PyNode>, format: &str) -> PyResult<String> {
    write(&Document::Condition(node.get().node.clone()), format)
}

/// The document of a rule, in `format`.
#[pyfunction]
pub(crate) fn dump_rule(rule: &Bound<'_, PyRule>, format: &str) -> PyResult<String> {
    write(&Document::Rule(rule.get().rule().clone()), format)
}

/// The document of a rule set, in `format`.
#[pyfunction]
pub(crate) fn dump_ruleset(ruleset: &Bound<'_, PyRuleSet>, format: &str) -> PyResult<String> {
    write(&Document::RuleSet(ruleset.get().ruleset().clone()), format)
}
