//! The tools for agents (`spec/agents.md`): the core's catalog answers every
//! call; this module only hands it the rule sets and the arguments' JSON text,
//! and writes native Python scalars in the JSON format's encoding.

use cleanrules::agents::{Catalog, Tool};
use cleanrules::format::write_value;
use cleanrules::json::{Json, JsonNumber, to_string};
use pyo3::exceptions::{PyTypeError, PyValueError};
use pyo3::prelude::*;

use crate::convert::{Problem, scalar};
use crate::rules::PyRuleSet;

/// The tools' names, sorted (`Tool::ALL`).
pub(crate) fn tool_names() -> Vec<&'static str> {
    Tool::ALL.iter().map(|t| t.as_str()).collect()
}

/// Named rule sets that answer tool calls: the core's catalog.
#[pyclass(frozen, name = "Catalog", module = "cleanrules._cleanrules")]
pub(crate) struct PyCatalog {
    catalog: Catalog,
}

#[pymethods]
impl PyCatalog {
    /// A catalog of `rulesets` by `names`, with which rules have an action
    /// (`actions`, per rule set in rule order), offering `tools` or every tool.
    #[new]
    #[pyo3(signature = (names, rulesets, actions, tools=None))]
    #[allow(clippy::needless_pass_by_value)] // PyO3 extracts lists by value.
    fn new(
        names: Vec<String>,
        rulesets: Vec<Bound<'_, PyRuleSet>>,
        actions: Vec<Vec<bool>>,
        tools: Option<Vec<String>>,
    ) -> PyResult<Self> {
        let mut catalog = Catalog::new();
        for ((name, ruleset), actions) in names.into_iter().zip(&rulesets).zip(actions) {
            catalog.insert_with_actions(name, ruleset.get().shared(), actions);
        }
        if let Some(tools) = tools {
            let tools = tools
                .iter()
                .map(|name| {
                    Tool::from_name(name)
                        .ok_or_else(|| PyValueError::new_err(format!("there is no tool {name:?}")))
                })
                .collect::<PyResult<Vec<_>>>()?;
            catalog = catalog.with_tools(tools);
        }
        Ok(Self { catalog })
    }

    /// The names of its rule sets, sorted.
    fn names(&self) -> Vec<&str> {
        self.catalog.names().collect()
    }

    /// The names of the tools it offers, sorted.
    fn tools(&self) -> Vec<&'static str> {
        self.catalog.tools().iter().map(|t| t.as_str()).collect()
    }

    /// Answer a call whose arguments are JSON text: whether it is a tool
    /// error, and the result (or `{"error": ...}`) as JSON text.
    fn call(&self, py: Python<'_>, tool: &str, arguments: &str) -> (bool, String) {
        // The core does not touch Python objects: other threads may run meanwhile.
        let result = py.detach(|| self.catalog.call_text(tool, arguments));
        (result.is_error, to_string(&result.content))
    }
}

/// A Python scalar that is not JSON data (a `Decimal`, a date, a datetime, a
/// numpy scalar...) in the JSON format's encoding, converted as the data of
/// `RuleSet.evaluate` is.
#[pyfunction]
pub(crate) fn scalar_json(value: &Bound<'_, PyAny>) -> PyResult<String> {
    let json = match scalar(value)? {
        Ok(converted) => write_value(&converted),
        // Read as data, these are nulls with their reason, as from JSON.
        Err(Problem::IntOverflow) => Json::Number(JsonNumber::from_literal(
            value.call_method0("__index__")?.str()?.to_string(),
        )),
        Err(Problem::DecimalOverflow) => Json::Object(vec![(
            "$decimal".into(),
            Json::String(value.str()?.to_string()),
        )]),
        Err(Problem::NotANumber) => {
            return Err(PyValueError::new_err(format!(
                "{} is NaN, which JSON cannot hold: send None for a missing value",
                value.repr()?
            )));
        }
        Err(Problem::TooPrecise) => {
            return Err(PyValueError::new_err(format!(
                "{} is finer than a microsecond: round it first",
                value.repr()?
            )));
        }
        Err(Problem::Opaque(name)) => {
            return Err(PyTypeError::new_err(format!(
                "a {name} is not data that a tool call can send"
            )));
        }
    };
    Ok(to_string(&json))
}
