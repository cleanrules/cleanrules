//! Condition nodes: the core's `Node`, built through its constructors, which
//! apply every construction rule (`spec/ast.md`). Python's `Expr` wraps one.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use cleanrules::Segment;
use cleanrules::ast::{ArithOp, Closed, CompareOp, Expr, Kind, Node, StringOp};
use cleanrules::format::{read_node, write_node};
use cleanrules::json::{Limits, parse, to_string};
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList, PyTuple};

use crate::convert;
use crate::errors::{IntoPyResult, path_to_py};

/// A condition node of the core, immutable.
#[pyclass(frozen, name = "Node", module = "cleanrules._cleanrules")]
pub(crate) struct PyNode {
    pub(crate) node: Node,
}

fn wrap(node: Node) -> PyNode {
    PyNode { node }
}

#[pymethods]
impl PyNode {
    /// The node's tag (`spec/ast.md`, *Catalogue*).
    #[getter]
    fn tag(&self) -> &'static str {
        self.node.tag()
    }

    /// `"condition"`, `"value"` or `"any"`.
    #[getter]
    fn kind(&self) -> &'static str {
        match self.node.kind() {
            Kind::Condition => "condition",
            Kind::Value => "value",
            Kind::Any => "any",
        }
    }

    /// The direct children, in order.
    fn children(&self) -> Vec<PyNode> {
        self.node
            .children()
            .into_iter()
            .map(|child| wrap(child.clone()))
            .collect()
    }

    /// The paths of every field the node reads, in first-appearance order.
    fn field_paths<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyTuple>> {
        let paths = self
            .node
            .field_paths()
            .into_iter()
            .map(|path| path_to_py(py, path).map(|list| list.to_tuple()))
            .collect::<PyResult<Vec<_>>>()?;
        PyTuple::new(py, paths)
    }

    /// Whether `other` has the same structure, operators and constants.
    fn same_as(&self, other: &Self) -> bool {
        self.node == other.node
    }

    /// A hash consistent with `same_as`.
    fn structural_hash(&self) -> u64 {
        let mut hasher = DefaultHasher::new();
        self.node.hash(&mut hasher);
        hasher.finish()
    }

    /// The node in the JSON format (`spec/json-format.md`), as text.
    fn to_json(&self) -> String {
        to_string(&write_node(&self.node))
    }

    /// The node's own data, for rendering: the path of a field, the value of a
    /// constant, the operator of a comparison, and so on.
    fn details<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDict>> {
        let details = PyDict::new(py);
        match self.node.expr() {
            Expr::Field(path) => details.set_item("path", path_to_py(py, path)?.to_tuple())?,
            Expr::Const(value) => details.set_item("value", convert::to_py(py, value)?)?,
            Expr::In { values, .. } => {
                let list = PyList::empty(py);
                for value in values {
                    list.append(convert::to_py(py, value)?)?;
                }
                details.set_item("values", list)?;
            }
            Expr::Between { closed, .. } => details.set_item("closed", closed.as_str())?,
            Expr::Matches { pattern, .. } => details.set_item("pattern", pattern.source())?,
            Expr::StringTest { text, .. } => details.set_item("text", text)?,
            _ => {}
        }
        Ok(details)
    }

    /// The canonical text (`spec/text.md`, *Printing*).
    fn text(&self) -> String {
        self.node.to_string()
    }

    fn __repr__(&self) -> String {
        format!("<Node {}>", self.to_json())
    }
}

fn node(value: &Bound<'_, PyNode>) -> Node {
    value.get().node.clone()
}

fn unknown(what: &str, name: &str) -> PyErr {
    PyValueError::new_err(format!("unknown {what} {name:?}"))
}

/// A field, by its path of keys (`str`) and indices (`int`).
#[pyfunction]
pub(crate) fn field(py: Python<'_>, path: &Bound<'_, PyList>) -> PyResult<PyNode> {
    let mut segments = Vec::with_capacity(path.len());
    for item in path.iter() {
        segments.push(if let Ok(key) = item.extract::<String>() {
            Segment::Key(key)
        } else {
            Segment::Index(item.extract::<u64>()?)
        });
    }
    Node::field(segments).or_raise(py).map(wrap)
}

/// A constant, converted from a Python scalar.
#[pyfunction]
pub(crate) fn constant(py: Python<'_>, value: &Bound<'_, PyAny>) -> PyResult<PyNode> {
    Node::constant(convert::constant(value)?)
        .or_raise(py)
        .map(wrap)
}

/// `left op right`, with `op` one of `eq`, `ne`, `lt`, `le`, `gt`, `ge`.
#[pyfunction]
pub(crate) fn compare(
    py: Python<'_>,
    op: &str,
    left: &Bound<'_, PyNode>,
    right: &Bound<'_, PyNode>,
) -> PyResult<PyNode> {
    let op = CompareOp::from_tag(op).ok_or_else(|| unknown("comparison", op))?;
    Node::compare(op, node(left), node(right))
        .or_raise(py)
        .map(wrap)
}

/// The conjunction of two or more conditions.
#[allow(clippy::needless_pass_by_value)] // PyO3 extracts lists by value.
#[pyfunction]
pub(crate) fn and_(py: Python<'_>, operands: Vec<Bound<'_, PyNode>>) -> PyResult<PyNode> {
    Node::and(operands.iter().map(node).collect())
        .or_raise(py)
        .map(wrap)
}

/// The disjunction of two or more conditions.
#[allow(clippy::needless_pass_by_value)] // PyO3 extracts lists by value.
#[pyfunction]
pub(crate) fn or_(py: Python<'_>, operands: Vec<Bound<'_, PyNode>>) -> PyResult<PyNode> {
    Node::or(operands.iter().map(node).collect())
        .or_raise(py)
        .map(wrap)
}

/// The negation of a condition.
#[pyfunction]
pub(crate) fn not_(py: Python<'_>, operand: &Bound<'_, PyNode>) -> PyResult<PyNode> {
    Node::not(node(operand)).or_raise(py).map(wrap)
}

/// Membership in a set of constants.
#[allow(clippy::needless_pass_by_value)] // PyO3 extracts lists by value.
#[pyfunction]
pub(crate) fn is_in(
    py: Python<'_>,
    operand: &Bound<'_, PyNode>,
    values: Vec<Bound<'_, PyAny>>,
) -> PyResult<PyNode> {
    let values = values
        .iter()
        .map(convert::constant)
        .collect::<PyResult<Vec<_>>>()?;
    Node::is_in(node(operand), values).or_raise(py).map(wrap)
}

/// A range test; `closed` is `both`, `left`, `right` or `none`.
#[pyfunction]
pub(crate) fn between(
    py: Python<'_>,
    operand: &Bound<'_, PyNode>,
    low: &Bound<'_, PyNode>,
    high: &Bound<'_, PyNode>,
    closed: &str,
) -> PyResult<PyNode> {
    let closed = Closed::from_name(closed).ok_or_else(|| unknown("closed", closed))?;
    Node::between(node(operand), node(low), node(high), closed)
        .or_raise(py)
        .map(wrap)
}

/// Whether a value is null.
#[pyfunction]
pub(crate) fn is_null(py: Python<'_>, operand: &Bound<'_, PyNode>) -> PyResult<PyNode> {
    Node::is_null(node(operand)).or_raise(py).map(wrap)
}

/// A regular expression search.
#[pyfunction]
pub(crate) fn matches(
    py: Python<'_>,
    operand: &Bound<'_, PyNode>,
    pattern: &str,
) -> PyResult<PyNode> {
    Node::matches(node(operand), pattern).or_raise(py).map(wrap)
}

/// A literal string test: `starts_with`, `ends_with` or `contains`.
#[pyfunction]
pub(crate) fn string_test(
    py: Python<'_>,
    op: &str,
    operand: &Bound<'_, PyNode>,
    text: &str,
) -> PyResult<PyNode> {
    let op = StringOp::from_tag(op).ok_or_else(|| unknown("string test", op))?;
    Node::string_test(op, node(operand), text)
        .or_raise(py)
        .map(wrap)
}

/// Arithmetic: `add`, `sub`, `mul` or `div`.
#[pyfunction]
pub(crate) fn arith(
    py: Python<'_>,
    op: &str,
    left: &Bound<'_, PyNode>,
    right: &Bound<'_, PyNode>,
) -> PyResult<PyNode> {
    let op = ArithOp::from_tag(op).ok_or_else(|| unknown("operator", op))?;
    Node::arith(op, node(left), node(right))
        .or_raise(py)
        .map(wrap)
}

/// The absolute value of a number.
#[pyfunction]
pub(crate) fn abs_(py: Python<'_>, operand: &Bound<'_, PyNode>) -> PyResult<PyNode> {
    Node::abs(node(operand)).or_raise(py).map(wrap)
}

/// The negation of a number.
#[pyfunction]
pub(crate) fn neg(py: Python<'_>, operand: &Bound<'_, PyNode>) -> PyResult<PyNode> {
    Node::neg(node(operand)).or_raise(py).map(wrap)
}

/// A node read back from the JSON text written by `Node.to_json` (for pickling).
#[pyfunction]
pub(crate) fn node_from_json(py: Python<'_>, text: &str) -> PyResult<PyNode> {
    let limits = Limits::default();
    let json = parse(text, &limits).or_raise(py)?;
    read_node(&json, "", &limits).or_raise(py).map(wrap)
}
