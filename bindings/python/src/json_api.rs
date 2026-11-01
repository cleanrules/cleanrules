//! JSON entry points, used by the conformance runner's JSON run
//! (`conformance/README.md`, *Runners*). They are private (`_` names) until
//! task D.1 gives the binding its public reading and writing API.

use cleanrules::Value;
use cleanrules::eval::{Options, evaluate_tree, evaluate_value_tree};
use cleanrules::explain::{explain, summary};
use cleanrules::format::{
    ValueContext, read_decision_document, read_document, read_node, read_ruleset, read_value,
    write_decision, write_document, write_explanation, write_result, write_summary,
};
use cleanrules::json::{Json, Limits, parse, to_string};
use cleanrules::logic::{Kleene, LogicName};
use cleanrules::rules::RuleSet;
use pyo3::prelude::*;

use crate::errors::IntoPyResult;

fn limits(max_size: Option<usize>, max_nesting: Option<usize>, max_depth: Option<usize>) -> Limits {
    let mut limits = Limits::default();
    if let Some(size) = max_size {
        limits.max_size = size;
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

/// Read a document; its canonical form.
#[pyfunction]
#[pyo3(signature = (text, max_size=None, max_nesting=None, max_depth=None))]
pub(crate) fn _read_document(
    py: Python<'_>,
    text: &str,
    max_size: Option<usize>,
    max_nesting: Option<usize>,
    max_depth: Option<usize>,
) -> PyResult<String> {
    let document = read_document(text, &limits(max_size, max_nesting, max_depth)).or_raise(py)?;
    Ok(to_string(&write_document(&document)))
}

/// Read a decision document with its rule set (the content of a rule set
/// document); the decision as it is written back.
#[pyfunction]
#[pyo3(signature = (ruleset_json, text, max_size=None, max_nesting=None, max_depth=None))]
pub(crate) fn _read_decision(
    py: Python<'_>,
    ruleset_json: &str,
    text: &str,
    max_size: Option<usize>,
    max_nesting: Option<usize>,
    max_depth: Option<usize>,
) -> PyResult<String> {
    let rules = ruleset(py, ruleset_json)?;
    let limits = limits(max_size, max_nesting, max_depth);
    let decision = read_decision_document(text, &rules, &limits).or_raise(py)?;
    Ok(to_string(&write_decision(&decision)))
}

/// Evaluate one input given as a JSON value; the decision.
#[pyfunction]
pub(crate) fn _evaluate(py: Python<'_>, ruleset_json: &str, input_json: &str) -> PyResult<String> {
    let rules = ruleset(py, ruleset_json)?;
    let decision = rules.evaluate(&input(py, input_json)?).or_raise(py)?;
    Ok(to_string(&write_decision(&decision)))
}

/// Evaluate a batch given as a JSON array; the decisions, as an array.
#[pyfunction]
pub(crate) fn _evaluate_many(
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
pub(crate) fn _explain(
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
pub(crate) fn _summary(py: Python<'_>, ruleset_json: &str) -> PyResult<String> {
    Ok(to_string(&write_summary(&summary(&ruleset(
        py,
        ruleset_json,
    )?))))
}

/// Evaluate one node given in JSON on an input given in JSON; its result tree.
#[pyfunction]
pub(crate) fn _evaluate_node(
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
