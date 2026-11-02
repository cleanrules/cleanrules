//! Core errors as Python exceptions (`spec/errors.md`, *In each language*).
//!
//! The classes live in `cleanrules.errors`, in Python; this module picks the
//! class for each code and passes the message, the hint and the context.

use cleanrules::{Error, ErrorCode, Segment};
use pyo3::exceptions::PyRuntimeError;
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList};

use crate::node::PyNode;

/// The Python class of an error code.
fn class_name(code: ErrorCode) -> &'static str {
    match code {
        ErrorCode::FormatError
        | ErrorCode::UnsupportedVersion
        | ErrorCode::LimitExceeded
        | ErrorCode::DecisionMismatch => "FormatError",
        ErrorCode::ExpressionError => "ExpressionError",
        ErrorCode::RuleDefinitionError => "RuleDefinitionError",
        ErrorCode::RuleSyntaxError => "RuleSyntaxError",
        ErrorCode::RegistryError => "RegistryError",
        ErrorCode::RuleNotFound => "RuleNotFoundError",
        ErrorCode::EvaluationError => "EvaluationError",
        ErrorCode::MissingField => "MissingFieldError",
        ErrorCode::TypeMismatch => "TypeMismatchError",
        ErrorCode::HitPolicyError => "HitPolicyError",
        ErrorCode::ActionError => "ActionError",
        ErrorCode::Unsupported => "UnsupportedError",
        // The core's codes are `#[non_exhaustive]`: a new one is still an error.
        _ => "CleanRulesError",
    }
}

/// A field path as a Python list of `str` and `int`.
pub(crate) fn path_to_py<'py>(py: Python<'py>, path: &[Segment]) -> PyResult<Bound<'py, PyList>> {
    let list = PyList::empty(py);
    for segment in path {
        match segment {
            Segment::Key(key) => list.append(key)?,
            Segment::Index(index) => list.append(index)?,
        }
    }
    Ok(list)
}

fn build(py: Python<'_>, error: &Error) -> PyResult<PyErr> {
    let module = py.import("cleanrules.errors")?;
    let class = module.getattr(class_name(error.code()))?;
    let kwargs = PyDict::new(py);
    kwargs.set_item("code", error.code().as_str())?;
    kwargs.set_item("hint", error.hint())?;
    let context = error.context();
    if let Some(pointer) = &context.pointer {
        kwargs.set_item("pointer", pointer)?;
    }
    if let Some(line) = context.line {
        kwargs.set_item("line", line)?;
    }
    if let Some(column) = context.column {
        kwargs.set_item("column", column)?;
    }
    if let Some(rule) = &context.rule {
        kwargs.set_item("rule", rule)?;
    }
    if !context.rules.is_empty() {
        kwargs.set_item("rules", &context.rules)?;
    }
    if let Some(path) = &context.path {
        kwargs.set_item("path", path_to_py(py, path)?)?;
    }
    if !context.types.is_empty() {
        let types: Vec<&str> = context.types.iter().map(|g| g.as_str()).collect();
        kwargs.set_item("types", types)?;
    }
    if let Some(row) = context.row {
        kwargs.set_item("row", row)?;
    }
    if let Some(name) = &context.name {
        kwargs.set_item("name", name)?;
    }
    if !context.available.is_empty() {
        kwargs.set_item("available", &context.available)?;
    }
    if !context.executed.is_empty() {
        kwargs.set_item("executed", &context.executed)?;
    }
    if let Some(node) = &context.node {
        let node = PyNode {
            node: node.as_ref().clone(),
        };
        kwargs.set_item("node", node)?;
    }
    if let Some(operation) = &context.operation {
        kwargs.set_item("operation", operation)?;
    }
    if let Some(reason) = &context.reason {
        kwargs.set_item("reason", reason)?;
    }
    let instance = class.call((error.message(),), Some(&kwargs))?;
    Ok(PyErr::from_value(instance))
}

/// The Python exception for a core error.
pub(crate) fn to_py(py: Python<'_>, error: &Error) -> PyErr {
    // If the error classes cannot be built (a broken install), say so plainly.
    build(py, error).unwrap_or_else(|failure| {
        PyRuntimeError::new_err(format!(
            "{} ({}); building its exception failed: {failure}",
            error.message(),
            error.code()
        ))
    })
}

/// An `expression_error` raised by the binding itself, while converting a
/// Python value.
pub(crate) fn expression(py: Python<'_>, message: impl Into<String>, hint: &str) -> PyErr {
    to_py(
        py,
        &Error::new(ErrorCode::ExpressionError, message).with_hint(hint),
    )
}

/// A `rule_definition_error` raised by the binding itself, naming the rule.
pub(crate) fn definition(
    py: Python<'_>,
    rule: &str,
    message: impl Into<String>,
    hint: &str,
) -> PyErr {
    let mut error = Error::new(ErrorCode::RuleDefinitionError, message).with_hint(hint);
    error.context_mut().rule = Some(rule.to_owned());
    to_py(py, &error)
}

/// Turn core results into Python ones.
pub(crate) trait IntoPyResult<T> {
    /// The value, or the core error as a Python exception.
    fn or_raise(self, py: Python<'_>) -> PyResult<T>;
}

impl<T> IntoPyResult<T> for cleanrules::Result<T> {
    fn or_raise(self, py: Python<'_>) -> PyResult<T> {
        self.map_err(|error| to_py(py, &error))
    }
}

/// `error`, located at `pointer` when it is a cleanrules error without a
/// location (one raised while converting a Python value).
pub(crate) fn with_pointer(py: Python<'_>, error: PyErr, pointer: &str) -> PyErr {
    let value = error.value(py);
    if value
        .getattr("pointer")
        .is_ok_and(|current| current.is_none())
    {
        // A failure here leaves the error as it was, which is still right.
        let _ = value.setattr("pointer", pointer);
    }
    error
}
