//! JSON entry points, used by the conformance runner's JSON run
//! (`conformance/README.md`, *Runners*). They are private (`_` names) until
//! task D.1 gives the binding its public reading and writing API.

use cleanrules::Value;
use cleanrules::bundle::{BundleRules, CaseFile};
use cleanrules::eval::{Options, evaluate_tree, evaluate_value_tree};
use cleanrules::explain::{explain, summary};
use cleanrules::format::{
    Document, DocumentFormat, ValueContext, read_decision_document_as, read_document,
    read_document_as, read_node, read_ruleset, read_value, write_decision, write_document,
    write_explanation, write_result, write_summary,
};
use cleanrules::json::{Json, Limits, parse, to_string};
use cleanrules::logic::{Kleene, LogicName};
use cleanrules::rules::RuleSet;
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;

use crate::bundles::recorded_limits;
use crate::errors::IntoPyResult;

fn limits(max_size: Option<usize>, max_nesting: Option<usize>, max_depth: Option<usize>) -> Limits {
    let mut limits = Limits::default();
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

fn json(py: Python<'_>, text: &str) -> PyResult<Json> {
    parse(text, &Limits::default()).or_raise(py)
}

fn ruleset(py: Python<'_>, text: &str) -> PyResult<RuleSet> {
    read_ruleset(&json(py, text)?, "/ruleset", &Limits::default()).or_raise(py)
}

fn input(py: Python<'_>, text: &str) -> PyResult<Value> {
    read_value(&json(py, text)?, "/input", ValueContext::Data).or_raise(py)
}

/// Read a document, in `format` (`json` or `yaml`); its canonical form.
#[pyfunction]
#[pyo3(signature = (text, max_size=None, max_nesting=None, max_depth=None, max_path_segments=None, max_table_size=None, format="json"))]
#[pyo3(name = "_read_document")]
#[allow(clippy::too_many_arguments)] // The limits are keyword arguments in Python.
pub(crate) fn py_read_document(
    py: Python<'_>,
    text: &str,
    max_size: Option<usize>,
    max_nesting: Option<usize>,
    max_depth: Option<usize>,
    max_path_segments: Option<usize>,
    max_table_size: Option<usize>,
    format: &str,
) -> PyResult<String> {
    let mut limits = limits(max_size, max_nesting, max_depth);
    if let Some(segments) = max_path_segments {
        limits.max_path_segments = segments;
    }
    if let Some(size) = max_table_size {
        limits.max_table_size = size;
    }
    let document =
        read_document_as(text.as_bytes(), document_format(format)?, &limits).or_raise(py)?;
    Ok(to_string(&write_document(&document)))
}

fn document_format(name: &str) -> PyResult<DocumentFormat> {
    DocumentFormat::from_name(name)
        .ok_or_else(|| PyValueError::new_err(format!("unknown format {name:?}")))
}

/// Make a bundle of the rules of a rule set or decision table document, with
/// the case files given as `(path, text)`; the bundle's document.
#[pyfunction]
#[pyo3(signature = (rules_json, id, revision, cases, max_size=None, max_nesting=None, max_depth=None, max_path_segments=None, max_table_size=None, max_values=None))]
#[pyo3(name = "_make_bundle")]
#[allow(clippy::too_many_arguments, clippy::needless_pass_by_value)] // The limits are keyword arguments in Python; PyO3 extracts lists by value.
pub(crate) fn py_make_bundle(
    py: Python<'_>,
    rules_json: &str,
    id: &str,
    revision: &str,
    cases: Vec<(String, String)>,
    max_size: Option<usize>,
    max_nesting: Option<usize>,
    max_depth: Option<usize>,
    max_path_segments: Option<usize>,
    max_table_size: Option<usize>,
    max_values: Option<usize>,
) -> PyResult<String> {
    let rules: BundleRules = match read_document(rules_json, &Limits::default()).or_raise(py)? {
        Document::RuleSet(ruleset) => ruleset.into(),
        Document::DecisionTable(table) => table.into(),
        _ => {
            return Err(PyValueError::new_err(
                "the rules of a bundle are a rule set or a decision table document",
            ));
        }
    };
    let cases = cases
        .iter()
        .map(|(path, text)| CaseFile::new(path, text.as_bytes()))
        .collect::<cleanrules::Result<Vec<_>>>()
        .or_raise(py)?;
    let limits = recorded_limits(
        max_size,
        max_nesting,
        max_depth,
        max_path_segments,
        max_table_size,
        max_values,
    );
    let bundle = crate::bundles::make(rules, id, revision, cases, &limits).or_raise(py)?;
    Ok(to_string(&write_document(&bundle.into_document())))
}

/// Read two bundle documents and compare the first with the second; the
/// comparison's JSON form.
#[pyfunction]
#[pyo3(name = "_compare_bundles")]
pub(crate) fn py_compare_bundles(py: Python<'_>, a_json: &str, b_json: &str) -> PyResult<String> {
    let bundle = |text: &str| match read_document(text, &Limits::default()).or_raise(py)? {
        Document::Bundle(bundle) => Ok(bundle),
        _ => Err(PyValueError::new_err("not a bundle document")),
    };
    let (a, b) = (bundle(a_json)?, bundle(b_json)?);
    Ok(to_string(&a.compare(&b).to_json()))
}

/// Read a decision document with its rule set (the content of a rule set
/// document); the decision as it is written back.
#[pyfunction]
#[pyo3(signature = (ruleset_json, text, format="json", max_size=None, max_nesting=None, max_depth=None))]
#[allow(clippy::too_many_arguments)] // The limits are keyword arguments in Python.
#[pyo3(name = "_read_decision")]
pub(crate) fn py_read_decision(
    py: Python<'_>,
    ruleset_json: &str,
    text: &str,
    format: &str,
    max_size: Option<usize>,
    max_nesting: Option<usize>,
    max_depth: Option<usize>,
) -> PyResult<String> {
    let rules = ruleset(py, ruleset_json)?;
    let limits = limits(max_size, max_nesting, max_depth);
    let format = document_format(format)?;
    let decision =
        read_decision_document_as(text.as_bytes(), format, &rules, &limits).or_raise(py)?;
    Ok(to_string(&write_decision(&decision)))
}

/// Evaluate one input given as a JSON value; the decision.
#[pyfunction]
#[pyo3(name = "_evaluate")]
pub(crate) fn py_evaluate(
    py: Python<'_>,
    ruleset_json: &str,
    input_json: &str,
) -> PyResult<String> {
    let rules = ruleset(py, ruleset_json)?;
    let decision = rules.evaluate(&input(py, input_json)?).or_raise(py)?;
    Ok(to_string(&write_decision(&decision)))
}

/// Evaluate a batch given as a JSON array; the decisions, as an array.
#[pyfunction]
#[pyo3(name = "_evaluate_many")]
pub(crate) fn py_evaluate_many(
    py: Python<'_>,
    ruleset_json: &str,
    inputs_json: &str,
) -> PyResult<String> {
    let rules = ruleset(py, ruleset_json)?;
    let parsed = json(py, inputs_json)?;
    let Json::Array(items) = &parsed else {
        return Err(pyo3::exceptions::PyValueError::new_err(
            "inputs must be an array",
        ));
    };
    let inputs = items
        .iter()
        .map(|item| read_value(item, "/inputs", ValueContext::Data))
        .collect::<cleanrules::Result<Vec<_>>>()
        .or_raise(py)?;
    let decisions = rules.evaluate_many(&inputs).or_raise(py)?;
    Ok(to_string(&Json::Array(
        decisions.iter().map(write_decision).collect(),
    )))
}

/// Evaluate and explain; the decision and the explanation.
#[pyfunction]
#[pyo3(name = "_explain")]
pub(crate) fn py_explain(
    py: Python<'_>,
    ruleset_json: &str,
    input_json: &str,
) -> PyResult<(String, String)> {
    let rules = ruleset(py, ruleset_json)?;
    let decision = rules.evaluate(&input(py, input_json)?).or_raise(py)?;
    Ok((
        to_string(&write_decision(&decision)),
        to_string(&write_explanation(&explain(&decision))),
    ))
}

/// The summary of a rule set.
#[pyfunction]
#[pyo3(name = "_summary")]
pub(crate) fn py_summary(py: Python<'_>, ruleset_json: &str) -> PyResult<String> {
    Ok(to_string(&write_summary(&summary(&ruleset(
        py,
        ruleset_json,
    )?))))
}

/// Evaluate one node given in JSON on an input given in JSON; its result tree.
#[pyfunction]
#[pyo3(name = "_evaluate_node")]
pub(crate) fn py_evaluate_node(
    py: Python<'_>,
    node_json: &str,
    input_json: &str,
    logic: &str,
    strict: bool,
    value: bool,
) -> PyResult<String> {
    let LogicName::Boolean = LogicName::from_name(logic).or_raise(py)? else {
        return Err(pyo3::exceptions::PyValueError::new_err(format!(
            "the logic {logic:?} cannot evaluate one node yet"
        )));
    };
    let node = read_node(&json(py, node_json)?, "", &Limits::default()).or_raise(py)?;
    let data = input(py, input_json)?;
    let options = Options { strict, rule: None };
    let tree = if value {
        evaluate_value_tree(&node, &data, &Kleene, options)
    } else {
        evaluate_tree(&node, &data, &Kleene, options)
    }
    .or_raise(py)?;
    Ok(to_string(&write_result(&tree)))
}
