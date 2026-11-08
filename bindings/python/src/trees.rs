//! Evaluating one node (`spec/ast.md`, *Evaluating one node*): `evaluate_tree`
//! for a condition and `evaluate_value_tree` for a value, with their result
//! trees.

use cleanrules::eval::{Evaluated, Options, Outcome, evaluate_tree, evaluate_value_tree};
use cleanrules::format::write_result;
use cleanrules::json::to_string;
use cleanrules::logic::{Kleene, LogicName, Truth};
use cleanrules::{Error, ErrorCode};
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList, PyTuple};

use crate::convert;
use crate::errors::{self, IntoPyResult, path_to_py};
use crate::node::PyNode;
use crate::read::{Paths, read};

/// A result tree as Python data: `node`, `truth` or `value`, `reason`, and
/// `children`, each a tree.
pub(crate) fn tree_to_py<'py>(
    py: Python<'py>,
    tree: &Evaluated<'_, Truth>,
    objects: &[Py<PyAny>],
) -> PyResult<Bound<'py, PyDict>> {
    let dict = PyDict::new(py);
    let node: &cleanrules::ast::Node = &tree.node;
    dict.set_item("node", PyNode { node: node.clone() })?;
    match &tree.outcome {
        Outcome::Truth(truth) => {
            let name = match truth {
                Truth::True => "true",
                Truth::False => "false",
                Truth::Unknown => "unknown",
            };
            dict.set_item("truth", name)?;
        }
        Outcome::Value(value) => {
            dict.set_item("value", convert::to_py_with(py, value, objects)?)?;
        }
    }
    match &tree.reason {
        Some(reason) => {
            let paths = reason
                .paths
                .iter()
                .map(|path| path_to_py(py, path).map(|list| list.to_tuple()))
                .collect::<PyResult<Vec<_>>>()?;
            let types: Vec<&str> = reason.types.iter().map(|g| g.as_str()).collect();
            dict.set_item(
                "reason",
                (
                    reason.code.as_str(),
                    PyTuple::new(py, paths)?,
                    types,
                    &reason.detail,
                ),
            )?;
        }
        None => dict.set_item("reason", py.None())?,
    }
    let children = PyList::empty(py);
    for child in &tree.children {
        children.append(tree_to_py(py, child, objects)?)?;
    }
    dict.set_item("children", children)?;
    Ok(dict)
}

fn evaluate_node<'py>(
    py: Python<'py>,
    node: &Bound<'py, PyNode>,
    data: &Bound<'py, PyAny>,
    strict: bool,
    value: bool,
    max_values: Option<usize>,
) -> PyResult<(Bound<'py, PyDict>, String)> {
    let node = &node.get().node;
    let paths = Paths::new(node.field_paths());
    let native = read(
        data,
        &paths,
        crate::rules::max_values_or_default(max_values),
        None,
    )?;
    let options = Options { strict, rule: None };
    let tree = if value {
        evaluate_value_tree(node, &native.value, &Kleene, options)
    } else {
        evaluate_tree(node, &native.value, &Kleene, options)
    };
    let tree = tree.map_err(|error| crate::rules::chained(py, &error, &native))?;
    Ok((
        tree_to_py(py, &tree, &native.objects)?,
        to_string(&write_result(&tree)),
    ))
}

/// Evaluate a condition on `data`: its result tree as Python data, and in
/// the JSON format.
#[pyfunction]
#[pyo3(name = "evaluate_tree", signature = (node, data, logic, strict, max_values=None))]
pub(crate) fn evaluate_condition<'py>(
    py: Python<'py>,
    node: &Bound<'py, PyNode>,
    data: &Bound<'py, PyAny>,
    logic: &str,
    strict: bool,
    max_values: Option<usize>,
) -> PyResult<(Bound<'py, PyDict>, String)> {
    // Every logic of this build has its arm: a new one cannot be ignored silently.
    match LogicName::from_name(logic).or_raise(py)? {
        LogicName::Boolean => evaluate_node(py, node, data, strict, false, max_values),
        other => Err(errors::to_py(
            py,
            &Error::new(
                ErrorCode::Unsupported,
                format!("the logic {other:?} cannot evaluate one node yet"),
            )
            .unsupported(
                "evaluate_tree",
                "a logic added to the core without its binding",
            ),
        )),
    }
}

/// Evaluate a value on `data`: its result tree as Python data, and in the
/// JSON format.
#[pyfunction]
#[pyo3(name = "evaluate_value_tree", signature = (node, data, strict, max_values=None))]
pub(crate) fn evaluate_value<'py>(
    py: Python<'py>,
    node: &Bound<'py, PyNode>,
    data: &Bound<'py, PyAny>,
    strict: bool,
    max_values: Option<usize>,
) -> PyResult<(Bound<'py, PyDict>, String)> {
    evaluate_node(py, node, data, strict, true, max_values)
}
