//! Rules, rule sets and decisions (`spec/rules.md`), built and evaluated by the
//! core.
//!
//! A core `Decision` borrows its rule set, so a Python decision keeps the rule
//! set and the converted input instead, with what Python reads at once (the
//! selected rules, the outputs). Explaining or writing it evaluates the input
//! again: evaluation is deterministic (`CLAUDE.md`, invariant 7), so it is the
//! same decision.

use std::sync::{Arc, Mutex, PoisonError};

use cleanrules::explain::{Cause, Gap, GroupKind, Requirement, Summary, explain, summary};
use cleanrules::format::{
    DocumentFormat, parse_as, read_parsed_decision_document, write_decision, write_explanation,
    write_summary,
};
use cleanrules::json::{Json, Limits, to_string};
use cleanrules::logic::{LogicName, Truth};
use cleanrules::rules::{Aggregate, Aggregated, Config, Decision, HitPolicy, Rule, RuleSet};
use cleanrules::{Error, ErrorCode, Map, Value};
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::{PyDelta, PyDict, PyList, PyTuple};
use pyo3::{PyTraverseError, PyVisit};

use crate::convert;
use crate::errors::path_to_py;
use crate::errors::{self, IntoPyResult, to_py};
use crate::node::PyNode;
use crate::read::{Native, Paths, Stop, read};

/// A rule of the core, immutable.
#[pyclass(frozen, name = "Rule", module = "cleanrules._cleanrules")]
pub(crate) struct PyRule {
    rule: Rule,
}

impl From<Rule> for PyRule {
    fn from(rule: Rule) -> Self {
        PyRule { rule }
    }
}

impl PyRule {
    pub(crate) fn rule(&self) -> &Rule {
        &self.rule
    }
}

impl PyRuleSet {
    pub(crate) fn ruleset(&self) -> &RuleSet {
        &self.ruleset
    }
}

#[pymethods]
impl PyRule {
    /// The rule's name.
    #[getter]
    fn name(&self) -> &str {
        self.rule.name()
    }

    /// The condition.
    #[getter]
    fn when(&self) -> PyNode {
        PyNode {
            node: self.rule.when().clone(),
        }
    }

    /// The payload, in order.
    #[getter]
    fn then(&self) -> Vec<(String, PyNode)> {
        self.rule
            .then()
            .iter()
            .map(|(key, node)| (key.clone(), PyNode { node: node.clone() }))
            .collect()
    }

    /// The priority.
    #[getter]
    fn priority(&self) -> i64 {
        self.rule.get_priority()
    }

    /// The description, if any.
    #[getter]
    fn description(&self) -> Option<&str> {
        self.rule.get_description()
    }

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

    /// The name of the rule's action, if it names one.
    #[getter]
    fn action_name(&self) -> Option<&str> {
        self.rule.get_action_name()
    }
}

/// A rule: `then` holds the payload in order; `metadata` is converted as data.
#[pyfunction]
#[pyo3(signature = (name, when, then, priority, tags, description, metadata, action=None))]
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
    action: Option<&str>,
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
    if let Some(action) = action {
        rule = rule.action_name(action).or_raise(py)?;
    }
    let converted = match convert::metadata(metadata) {
        Ok(converted) => converted,
        Err(Stop::Python(error)) => return Err(error),
        Err(Stop::Limit(mut error)) => {
            error.context_mut().rule = Some(name.to_owned());
            return Err(errors::to_py(py, &error));
        }
    };
    let metadata = match converted {
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
    /// The paths its conditions and payloads read: the only ones read in data.
    paths: Paths,
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
    let paths = Paths::new(summary(&ruleset).fields.iter().map(|f| f.path.as_slice()));
    Ok(PyRuleSet {
        ruleset: Arc::new(ruleset),
        paths,
    })
}

#[pymethods]
impl PyRuleSet {
    /// Evaluate one input, reading at most `max_values` values from it
    /// (`spec/data-model.md`, *Reading native data*, *Limits*).
    #[pyo3(signature = (data, max_values=None))]
    fn evaluate(
        &self,
        py: Python<'_>,
        data: &Bound<'_, PyAny>,
        max_values: Option<usize>,
    ) -> PyResult<PyDecision> {
        let native = read(data, &self.paths, max_values_or_default(max_values), None)?;
        // The core does not touch Python objects: other threads may run meanwhile.
        let decision = py
            .detach(|| self.ruleset.evaluate(&native.value))
            .map_err(|error| chained(py, &error, &native))?;
        Ok(PyDecision::new(&self.ruleset, &decision, native))
    }

    /// Read a recorded decision, in `json` or `yaml`, with this rule set
    /// (`spec/json-format.md`, *Reading a decision*).
    #[pyo3(signature = (text, format, max_size=None, max_nesting=None, max_depth=None))]
    fn read_decision(
        &self,
        py: Python<'_>,
        text: &[u8],
        format: &str,
        max_size: Option<usize>,
        max_nesting: Option<usize>,
        max_depth: Option<usize>,
    ) -> PyResult<PyDecision> {
        let format = DocumentFormat::from_name(format)
            .ok_or_else(|| PyValueError::new_err(format!("unknown format {format:?}")))?;
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
        let json = parse_as(text, format, &limits).or_raise(py)?;
        let decision = read_parsed_decision_document(&json, &self.ruleset, &limits).or_raise(py)?;
        Ok(PyDecision::made(
            &self.ruleset,
            &decision,
            Source::Recorded(json, limits),
            Vec::new(),
        ))
    }

    /// Evaluate a batch: errors name the input's position (`row`).
    #[allow(clippy::needless_pass_by_value)] // PyO3 extracts lists by value.
    #[pyo3(signature = (data, max_values=None))]
    fn evaluate_many(
        &self,
        py: Python<'_>,
        data: Vec<Bound<'_, PyAny>>,
        max_values: Option<usize>,
    ) -> PyResult<Vec<PyDecision>> {
        let max_values = max_values_or_default(max_values);
        let natives = data
            .iter()
            .enumerate()
            .map(|(row, item)| read(item, &self.paths, max_values, Some(row)))
            .collect::<PyResult<Vec<_>>>()?;
        let inputs: Vec<Value> = natives.iter().map(|n| n.value.clone()).collect();
        let decisions = py
            .detach(|| self.ruleset.evaluate_many(&inputs))
            .map_err(|error| {
                let row = error.context().row.and_then(|r| usize::try_from(r).ok());
                match row.and_then(|r| natives.get(r)) {
                    Some(native) => chained(py, &error, native),
                    None => to_py(py, &error),
                }
            })?;
        Ok(decisions
            .iter()
            .zip(natives)
            .map(|(decision, native)| PyDecision::new(&self.ruleset, decision, native))
            .collect())
    }

    /// The summary in the JSON format, as text; `actions` says which rules
    /// have one, in rule order.
    #[allow(clippy::needless_pass_by_value)] // PyO3 extracts lists by value.
    /// Check that every named action is registered (`spec/rules.md`, *Actions*).
    #[allow(clippy::needless_pass_by_value)] // PyO3 extracts lists by value.
    fn check_action_names(&self, py: Python<'_>, registered: Vec<String>) -> PyResult<()> {
        self.ruleset.check_action_names(&registered).or_raise(py)
    }

    fn summary_json(&self, actions: Vec<bool>) -> String {
        to_string(&write_summary(&self.overview(actions)))
    }

    /// The summary's text (`spec/explain.md`, *Summaries*).
    #[allow(clippy::needless_pass_by_value)] // PyO3 extracts lists by value.
    fn summary_text(&self, actions: Vec<bool>) -> String {
        self.overview(actions).to_string()
    }
}

impl PyRuleSet {
    fn overview(&self, actions: Vec<bool>) -> Summary<'_> {
        summary(&self.ruleset).with_actions(actions)
    }
}

/// The limit on values read from one input: `max_values=`, or the default.
pub(crate) fn max_values_or_default(max_values: Option<usize>) -> usize {
    max_values.unwrap_or_else(|| Limits::default().max_values)
}

/// The Python exception of a core error; a `read_error` in strict mode is
/// chained to the exception that reading the data raised (`spec/ast.md`).
pub(crate) fn chained(py: Python<'_>, error: &Error, native: &Native) -> PyErr {
    let raised = to_py(py, error);
    if error.code() == ErrorCode::EvaluationError
        && let Some(path) = &error.context().path
        // The exception was raised reading the path or one of its prefixes.
        && let Some((_, cause)) = native.errors.iter().find(|(at, _)| path.starts_with(at))
    {
        raised.set_cause(py, Some(cause.clone_ref(py)));
    }
    raised
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
    source: Source,
    /// The native objects of the input's opaque values, given back in outputs.
    /// Behind a lock so that the garbage collector can clear them (`__clear__`)
    /// in a frozen class: an object may hold its own decision.
    objects: Mutex<Vec<Py<PyAny>>>,
    outcomes: Vec<RuleOutcome>,
    selected: Vec<usize>,
    output: Result<Option<Value>, Error>,
    /// The aggregate's values, when there is an aggregate and something was selected.
    aggregated: Option<Map>,
}

/// Where a decision comes from: it is made again from this when it is written
/// or explained.
enum Source {
    /// An input the rule set evaluated.
    Input(Value),
    /// A recorded decision the rule set accepted, with the limits it was read
    /// under.
    Recorded(Json, Limits),
}

fn truth_name(truth: Option<Truth>) -> &'static str {
    match truth {
        Some(Truth::True) => "true",
        Some(Truth::False) => "false",
        _ => "unknown",
    }
}

impl PyDecision {
    fn new(ruleset: &Arc<RuleSet>, decision: &Decision<'_>, native: Native) -> Self {
        Self::made(
            ruleset,
            decision,
            Source::Input(native.value),
            native.objects,
        )
    }

    fn made(
        ruleset: &Arc<RuleSet>,
        decision: &Decision<'_>,
        source: Source,
        objects: Vec<Py<PyAny>>,
    ) -> Self {
        let outcomes = decision
            .results()
            .iter()
            .map(|result| RuleOutcome {
                fired: result.fired,
                truth: truth_name(result.when.truth()),
                output: result.then.as_ref().map(|_| result.output()),
            })
            .collect();
        let selected = decision.selected_positions().to_vec();
        let aggregated = match decision.aggregated() {
            Some(Aggregated::Values(map)) => Some(map.clone()),
            _ => None,
        };
        Self {
            ruleset: Arc::clone(ruleset),
            source,
            objects: Mutex::new(objects),
            outcomes,
            selected,
            output: decision.output(),
            aggregated,
        }
    }

    /// The native objects, copied out of the lock: no Python code runs while
    /// it is held, so the garbage collector never waits on it.
    fn objects(&self, py: Python<'_>) -> Vec<Py<PyAny>> {
        let objects = self.objects.lock().unwrap_or_else(PoisonError::into_inner);
        objects.iter().map(|object| object.clone_ref(py)).collect()
    }

    /// Make the decision again; it succeeded before, so it succeeds now.
    fn rebuild(&self, py: Python<'_>) -> PyResult<Decision<'_>> {
        match &self.source {
            Source::Input(input) => self.ruleset.evaluate(input),
            Source::Recorded(json, limits) => {
                read_parsed_decision_document(json, &self.ruleset, limits)
            }
        }
        .or_raise(py)
    }
}

fn pairs_to_dict<'py>(
    py: Python<'py>,
    pairs: &[(String, Value)],
    objects: &[Py<PyAny>],
) -> PyResult<Bound<'py, PyDict>> {
    let dict = PyDict::new(py);
    for (key, value) in pairs {
        dict.set_item(key, convert::to_py_with(py, value, objects)?)?;
    }
    Ok(dict)
}

#[pymethods]
impl PyDecision {
    fn __traverse__(&self, visit: PyVisit<'_>) -> Result<(), PyTraverseError> {
        // Never wait inside the collector: a held lock is released at once anyway.
        if let Ok(objects) = self.objects.try_lock() {
            for object in objects.iter() {
                visit.call(object)?;
            }
        }
        Ok(())
    }

    fn __clear__(&self) {
        let taken =
            std::mem::take(&mut *self.objects.lock().unwrap_or_else(PoisonError::into_inner));
        drop(taken);
    }

    /// Every rule's result, in rule order: `(fired, truth, output or None)`.
    fn results<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyList>> {
        let objects = self.objects(py);
        let list = PyList::empty(py);
        for outcome in &self.outcomes {
            let output = match &outcome.output {
                Some(pairs) => pairs_to_dict(py, pairs, &objects)?.into_any(),
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
            Ok(Some(value)) => convert::to_py_with(py, value, &self.objects(py)),
            Ok(None) => Ok(py.None().into_bound(py)),
            Err(error) => Err(to_py(py, error)),
        }
    }

    /// The aggregate's values: `None` without an aggregate or when nothing was
    /// selected.
    fn aggregated<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        match &self.aggregated {
            Some(map) => convert::to_py_with(py, &Value::Map(map.clone()), &self.objects(py)),
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
        let objects = self.objects(py);
        let list = PyList::empty(py);
        for entry in explain(&decision).rules {
            let dict = PyDict::new(py);
            dict.set_item("rule", entry.rule.name())?;
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
            dict.set_item(
                "requirement",
                requirement_to_py(py, &entry.requirement, &objects)?,
            )?;
            list.append(dict)?;
        }
        Ok(list)
    }

    /// The result tree of rule `index`'s condition, as Python data and in the
    /// JSON format, from the input already read (never read again).
    fn rule_tree<'py>(
        &self,
        py: Python<'py>,
        index: usize,
    ) -> PyResult<(Bound<'py, PyDict>, String)> {
        let decision = self.rebuild(py)?;
        let Some(result) = decision.results().get(index) else {
            return Err(pyo3::exceptions::PyIndexError::new_err(
                "no rule at that position",
            ));
        };
        Ok((
            crate::trees::tree_to_py(py, &result.when, &self.objects(py))?,
            to_string(&cleanrules::format::write_result(&result.when)),
        ))
    }

    /// The explanation in the JSON format, as text.
    fn explanation_json(&self, py: Python<'_>) -> PyResult<String> {
        let decision = self.rebuild(py)?;
        Ok(to_string(&write_explanation(&explain(&decision))))
    }

    /// The explanation's text (`spec/explain.md`, *Text*), and per rule its
    /// text and its causes' texts in pre-order, as `explanation` lists them.
    #[allow(clippy::type_complexity)]
    fn explanation_texts(&self, py: Python<'_>) -> PyResult<(String, Vec<(String, Vec<String>)>)> {
        fn causes(requirement: &Requirement<'_, '_>, out: &mut Vec<String>) {
            match requirement {
                Requirement::Cause(cause) => out.push(cause.to_string()),
                Requirement::Group(_, items) => {
                    for item in items {
                        causes(item, out);
                    }
                }
            }
        }
        let decision = self.rebuild(py)?;
        let explanation = explain(&decision);
        let rules = explanation
            .rules
            .iter()
            .map(|entry| {
                let mut found = Vec::new();
                causes(&entry.requirement, &mut found);
                (entry.to_string(), found)
            })
            .collect();
        Ok((explanation.to_string(), rules))
    }
}

fn requirement_to_py<'py>(
    py: Python<'py>,
    requirement: &Requirement<'_, '_>,
    objects: &[Py<PyAny>],
) -> PyResult<Bound<'py, PyDict>> {
    match requirement {
        Requirement::Cause(cause) => cause_to_py(py, cause, objects),
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
                list.append(requirement_to_py(py, item, objects)?)?;
            }
            dict.set_item("items", list)?;
            Ok(dict)
        }
    }
}

fn cause_to_py<'py>(
    py: Python<'py>,
    cause: &Cause<'_, '_>,
    objects: &[Py<PyAny>],
) -> PyResult<Bound<'py, PyDict>> {
    let dict = PyDict::new(py);
    dict.set_item("kind", "cause")?;
    let node: &cleanrules::ast::Node = &cause.result.node;
    dict.set_item("node", PyNode { node: node.clone() })?;
    dict.set_item("truth", truth_name(Some(cause.truth())))?;
    dict.set_item("negated", cause.negated)?;
    dict.set_item("holds", cause.holds())?;
    let values = PyList::empty(py);
    for (path, value) in &cause.values {
        values.append((
            path_to_py(py, path)?.to_tuple(),
            convert::to_py_with(py, value, objects)?,
        ))?;
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
                Some(Gap::Number(value)) => convert::to_py_with(py, value, objects)?,
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
                    convert::to_py_with(py, &counterfactual.target, objects)?,
                    gap,
                ),
            )?;
        }
        None => dict.set_item("counterfactual", py.None())?,
    }
    Ok(dict)
}
