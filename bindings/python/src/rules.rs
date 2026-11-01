//! Rules, rule sets and decisions (`spec/rules.md`), built and evaluated by the
//! core.
//!
//! A core `Decision` borrows its rule set, so a Python decision keeps the rule
//! set and the converted input instead, with what Python reads at once (the
//! selected rules, the outputs). Explaining or writing it evaluates the input
//! again: evaluation is deterministic (`CLAUDE.md`, invariant 7), so it is the
//! same decision.

use std::sync::Arc;

use cleanrules::explain::{Cause, Gap, GroupKind, Requirement, explain, summary};
use cleanrules::format::{write_decision, write_explanation, write_summary};
use cleanrules::json::to_string;
use cleanrules::logic::{LogicName, Truth};
use cleanrules::rules::{Aggregate, Aggregated, Config, Decision, HitPolicy, Rule, RuleSet};
use cleanrules::{Error, Map, Value};
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::{PyDelta, PyDict, PyList, PyTuple};

use crate::convert;
use crate::errors::path_to_py;
use crate::errors::{self, IntoPyResult, to_py};
use crate::node::PyNode;

/// A rule of the core, immutable.
#[pyclass(frozen, name = "Rule", module = "cleanrules._cleanrules")]
pub(crate) struct PyRule {
    rule: Rule,
}

#[pymethods]
impl PyRule {
    /// The rule's tags, in order.
    #[getter]
    fn tags(&self) -> Vec<String> {
        self.rule.get_tags().to_vec()
    }

    /// The rule's metadata, as the core holds it.
    #[getter]
    fn metadata<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        convert::to_py(py, &Value::Map(self.rule.get_metadata().clone()))
    }
}

/// A rule: `then` holds the payload in order; `metadata` is converted as data.
#[pyfunction]
#[pyo3(signature = (name, when, then, priority, tags, description, metadata))]
#[allow(clippy::needless_pass_by_value, clippy::too_many_arguments)] // PyO3 extracts lists by value; the arguments are `Rule`'s.
pub(crate) fn rule(
    py: Python<'_>,
    name: &str,
    when: &Bound<'_, PyNode>,
    then: Vec<(String, Bound<'_, PyNode>)>,
    priority: i64,
    tags: Vec<String>,
    description: Option<&str>,
    metadata: &Bound<'_, PyDict>,
) -> PyResult<PyRule> {
    let payload = then
        .iter()
        .map(|(key, node)| (key.clone(), node.get().node.clone()))
        .collect();
    let mut rule = Rule::with_payload(name, when.get().node.clone(), payload)
        .or_raise(py)?
        .priority(priority)
        .tags(tags);
    if let Some(description) = description {
        rule = rule.description(description);
    }
    let metadata = match convert::metadata(metadata)? {
        Ok(Value::Map(map)) => map,
        Ok(_) => return Err(PyValueError::new_err("metadata must be a dict")),
        Err(problem) => {
            return Err(errors::definition(
                py,
                name,
                problem,
                "metadata holds data: None, bool, numbers, str, dates, lists and dicts",
            ));
        }
    };
    if !metadata.is_empty() {
        rule = rule.metadata(metadata);
    }
    Ok(PyRule { rule })
}

/// A rule set of the core, immutable and shared by its decisions.
#[pyclass(frozen, name = "RuleSet", module = "cleanrules._cleanrules")]
pub(crate) struct PyRuleSet {
    ruleset: Arc<RuleSet>,
}

/// A rule set from its rules and configuration, by the names of the format.
#[pyfunction]
#[pyo3(signature = (rules, hit_policy, aggregate, logic, strict))]
#[allow(clippy::needless_pass_by_value)] // PyO3 extracts lists by value.
pub(crate) fn ruleset(
    py: Python<'_>,
    rules: Vec<Bound<'_, PyRule>>,
    hit_policy: &str,
    aggregate: Option<&str>,
    logic: &str,
    strict: bool,
) -> PyResult<PyRuleSet> {
    let hit_policy = HitPolicy::from_name(hit_policy)
        .ok_or_else(|| PyValueError::new_err(format!("unknown hit policy {hit_policy:?}")))?;
    let aggregate = aggregate
        .map(|name| {
            Aggregate::from_name(name)
                .ok_or_else(|| PyValueError::new_err(format!("unknown aggregate {name:?}")))
        })
        .transpose()?;
    let config = Config {
        hit_policy,
        aggregate,
        logic: Some(LogicName::from_name(logic).or_raise(py)?),
        strict,
    };
    let rules = rules.iter().map(|r| r.get().rule.clone()).collect();
    let ruleset = RuleSet::new(rules, config).or_raise(py)?;
    Ok(PyRuleSet {
        ruleset: Arc::new(ruleset),
    })
}

#[pymethods]
impl PyRuleSet {
    /// Evaluate one input.
    fn evaluate(&self, py: Python<'_>, data: &Bound<'_, PyAny>) -> PyResult<PyDecision> {
        let input = convert::data(data)?;
        // The core does not touch Python objects: other threads may run meanwhile.
        let decision = py.detach(|| self.ruleset.evaluate(&input)).or_raise(py)?;
        Ok(PyDecision::new(&self.ruleset, &decision, input))
    }

    /// Evaluate a batch: errors name the input's position (`row`).
    #[allow(clippy::needless_pass_by_value)] // PyO3 extracts lists by value.
    fn evaluate_many(
        &self,
        py: Python<'_>,
        data: Vec<Bound<'_, PyAny>>,
    ) -> PyResult<Vec<PyDecision>> {
        let inputs = data
            .iter()
            .map(convert::data)
            .collect::<PyResult<Vec<_>>>()?;
        let decisions = py
            .detach(|| self.ruleset.evaluate_many(&inputs))
            .or_raise(py)?;
        Ok(decisions
            .iter()
            .zip(inputs)
            .map(|(decision, input)| PyDecision::new(&self.ruleset, decision, input))
            .collect())
    }

    /// The summary in the JSON format, as text.
    fn summary_json(&self) -> String {
        to_string(&write_summary(&summary(&self.ruleset)))
    }
}

/// What a rule's result says, read at once.
struct RuleOutcome {
    fired: bool,
    truth: &'static str,
    output: Option<Vec<(String, Value)>>,
}

/// A decision: what Python reads at once, and what is needed to rebuild it.
#[pyclass(frozen, name = "Decision", module = "cleanrules._cleanrules")]
pub(crate) struct PyDecision {
    ruleset: Arc<RuleSet>,
    input: Value,
    outcomes: Vec<RuleOutcome>,
    selected: Vec<usize>,
    output: Result<Option<Value>, Error>,
    /// The aggregate's values, when there is an aggregate and something was selected.
    aggregated: Option<Map>,
}

fn truth_name(truth: Option<Truth>) -> &'static str {
    match truth {
        Some(Truth::True) => "true",
        Some(Truth::False) => "false",
        _ => "unknown",
    }
}

impl PyDecision {
    fn new(ruleset: &Arc<RuleSet>, decision: &Decision<'_>, input: Value) -> Self {
        let outcomes = decision
            .results()
            .iter()
            .map(|result| RuleOutcome {
                fired: result.fired,
                truth: truth_name(result.when.truth()),
                output: result.then.as_ref().map(|_| result.output()),
            })
            .collect();
        let selected = decision
            .selected()
            .filter_map(|chosen| {
                decision
                    .results()
                    .iter()
                    .position(|r| r.rule.name() == chosen.rule.name())
            })
            .collect();
        let aggregated = match decision.aggregated() {
            Some(Aggregated::Values(map)) => Some(map.clone()),
            _ => None,
        };
        Self {
            ruleset: Arc::clone(ruleset),
            input,
            outcomes,
            selected,
            output: decision.output(),
            aggregated,
        }
    }

    /// Evaluate the input again; it succeeded before, so it succeeds now.
    fn rebuild(&self, py: Python<'_>) -> PyResult<Decision<'_>> {
        self.ruleset.evaluate(&self.input).or_raise(py)
    }
}

fn pairs_to_dict<'py>(py: Python<'py>, pairs: &[(String, Value)]) -> PyResult<Bound<'py, PyDict>> {
    let dict = PyDict::new(py);
    for (key, value) in pairs {
        dict.set_item(key, convert::to_py(py, value)?)?;
    }
    Ok(dict)
}

#[pymethods]
impl PyDecision {
    /// Every rule's result, in rule order: `(fired, truth, output or None)`.
    fn results<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyList>> {
        let list = PyList::empty(py);
        for outcome in &self.outcomes {
            let output = match &outcome.output {
                Some(pairs) => pairs_to_dict(py, pairs)?.into_any(),
                None => py.None().into_bound(py),
            };
            list.append((outcome.fired, outcome.truth, output))?;
        }
        Ok(list)
    }

    /// The positions of the selected rules, in rule order.
    fn selected(&self) -> Vec<usize> {
        self.selected.clone()
    }

    /// The single output, or `None`; raises for multi-hit decisions.
    fn output<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        match &self.output {
            Ok(Some(value)) => convert::to_py(py, value),
            Ok(None) => Ok(py.None().into_bound(py)),
            Err(error) => Err(to_py(py, error)),
        }
    }

    /// The aggregate's values: `None` without an aggregate or when nothing was
    /// selected.
    fn aggregated<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        match &self.aggregated {
            Some(map) => convert::to_py(py, &Value::Map(map.clone())),
            None => Ok(py.None().into_bound(py)),
        }
    }

    /// The decision in the JSON format, as text.
    fn to_json(&self, py: Python<'_>) -> PyResult<String> {
        Ok(to_string(&write_decision(&self.rebuild(py)?)))
    }

    /// The explanation as Python data: per rule, a dict with `rule`, `status`,
    /// `note`, `achievable` and `requirement` (a cause or a group, as dicts).
    fn explanation<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyList>> {
        let decision = self.rebuild(py)?;
        let list = PyList::empty(py);
        for entry in explain(&decision).rules {
            let dict = PyDict::new(py);
            dict.set_item("rule", entry.rule)?;
            dict.set_item("status", entry.status.as_str())?;
            match &entry.note {
                Some(note) => {
                    let (priority, winner_priority) = note.priorities.unzip();
                    dict.set_item(
                        "note",
                        (note.kind.as_str(), &note.winner, priority, winner_priority),
                    )?;
                }
                None => dict.set_item("note", py.None())?,
            }
            dict.set_item("achievable", entry.achievable)?;
            dict.set_item("requirement", requirement_to_py(py, &entry.requirement)?)?;
            list.append(dict)?;
        }
        Ok(list)
    }

    /// The explanation in the JSON format, as text.
    fn explanation_json(&self, py: Python<'_>) -> PyResult<String> {
        let decision = self.rebuild(py)?;
        Ok(to_string(&write_explanation(&explain(&decision))))
    }
}

fn requirement_to_py<'py>(
    py: Python<'py>,
    requirement: &Requirement<'_, '_>,
) -> PyResult<Bound<'py, PyDict>> {
    match requirement {
        Requirement::Cause(cause) => cause_to_py(py, cause),
        Requirement::Group(kind, items) => {
            let dict = PyDict::new(py);
            let name = if *kind == GroupKind::All {
                "all"
            } else {
                "any"
            };
            dict.set_item("kind", name)?;
            let list = PyList::empty(py);
            for item in items {
                list.append(requirement_to_py(py, item)?)?;
            }
            dict.set_item("items", list)?;
            Ok(dict)
        }
    }
}

fn cause_to_py<'py>(py: Python<'py>, cause: &Cause<'_, '_>) -> PyResult<Bound<'py, PyDict>> {
    let dict = PyDict::new(py);
    dict.set_item("kind", "cause")?;
    let node: &cleanrules::ast::Node = &cause.result.node;
    dict.set_item("node", PyNode { node: node.clone() })?;
    dict.set_item("truth", truth_name(Some(cause.truth())))?;
    dict.set_item("negated", cause.negated)?;
    dict.set_item("holds", cause.holds())?;
    let values = PyList::empty(py);
    for (path, value) in &cause.values {
        values.append((path_to_py(py, path)?.to_tuple(), convert::to_py(py, value)?))?;
    }
    dict.set_item("values", values)?;
    match cause.reason() {
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
    match &cause.counterfactual {
        Some(counterfactual) => {
            let gap = match &counterfactual.gap {
                Some(Gap::Number(value)) => convert::to_py(py, value)?,
                Some(Gap::Days(days)) => {
                    PyDelta::new(py, i32::try_from(*days)?, 0, 0, true)?.into_any()
                }
                Some(Gap::Microseconds(micros)) => {
                    let days = micros.div_euclid(86_400_000_000);
                    let rest = micros.rem_euclid(86_400_000_000);
                    PyDelta::new(
                        py,
                        i32::try_from(days)?,
                        i32::try_from(rest / 1_000_000)?,
                        i32::try_from(rest % 1_000_000)?,
                        true,
                    )?
                    .into_any()
                }
                None => py.None().into_bound(py),
            };
            dict.set_item(
                "counterfactual",
                (
                    path_to_py(py, &counterfactual.path)?.to_tuple(),
                    counterfactual.op.tag(),
                    convert::to_py(py, &counterfactual.target)?,
                    gap,
                ),
            )?;
        }
        None => dict.set_item("counterfactual", py.None())?,
    }
    Ok(dict)
}
