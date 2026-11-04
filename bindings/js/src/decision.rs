//! Decisions (`spec/rules.md`, *Evaluation*), with their explanations
//! (`spec/explain.md`).
//!
//! A core `Decision` borrows its rule set, so a JavaScript decision keeps the
//! rule set and what it was made from (the input, or a recorded decision),
//! with what is read at once. Writing or explaining it makes it again:
//! evaluation is deterministic (`CLAUDE.md`, invariant 7), so it is the same
//! decision. Values come out in the JSON format (`spec/json-format.md`,
//! *Values*); native JavaScript values for outputs wait for the specification
//! of that direction (phase F.2).

use std::sync::Arc;

use cleanrules::explain::explain;
use cleanrules::format::{
    DocumentFormat, read_parsed_decision_document, write_decision_as, write_explanation_as,
    write_value,
};
use cleanrules::json::{Json, Limits, to_string};
use cleanrules::logic::Truth;
use cleanrules::rules::{Decision as CoreDecision, RuleSet};
use cleanrules::{Map, Value};
use napi::{Env, Result};
use napi_derive::napi;

use crate::hooks::{OrThrow, range_error, throw};
use crate::limits;

/// What a decision was made from.
pub(crate) enum Source {
    /// An input the rule set evaluated.
    Input(Value),
    /// A recorded decision the rule set accepted, with the limits it was read
    /// under.
    Recorded(Json, Limits),
}

/// What a decision says, read once.
struct Facts {
    fired: Vec<bool>,
    truths: Vec<Truth>,
    selected: Vec<usize>,
    rule_outputs: Vec<Option<Vec<(String, Value)>>>,
    outputs: Vec<Vec<(String, Value)>>,
    /// The single output; a multi-hit decision has none (`hit_policy_error`).
    output: cleanrules::Result<Option<Value>>,
}

impl Facts {
    fn of(decision: &CoreDecision<'_>) -> Self {
        let results = decision.results();
        Self {
            fired: results.iter().map(|r| r.fired).collect(),
            truths: results
                .iter()
                .map(|r| r.when.truth().unwrap_or(Truth::Unknown))
                .collect(),
            selected: decision.selected_positions().to_vec(),
            rule_outputs: results
                .iter()
                .map(|r| r.then.as_ref().map(|_| r.output()))
                .collect(),
            outputs: decision.outputs(),
            output: decision.output(),
        }
    }
}

fn map_of(pairs: &[(String, Value)]) -> Value {
    let mut map = Map::new();
    for (key, value) in pairs {
        map.insert(key.clone(), value.clone());
    }
    Value::Map(map)
}

/// The result of evaluating a rule set on one input, or a recorded decision
/// read with its rule set; immutable.
#[napi]
pub struct Decision {
    ruleset: Arc<RuleSet>,
    source: Source,
    facts: Facts,
}

impl Decision {
    /// The decision `decision` of `ruleset`, made from `source`.
    pub(crate) fn new(ruleset: &Arc<RuleSet>, decision: &CoreDecision<'_>, source: Source) -> Self {
        Self {
            ruleset: Arc::clone(ruleset),
            source,
            facts: Facts::of(decision),
        }
    }

    /// The positions of the selected rules, in rule order.
    pub(crate) fn selected_positions(&self) -> &[usize] {
        &self.facts.selected
    }

    /// The truth name of the rule at `position`, a valid position.
    pub(crate) fn truth_name(&self, position: usize) -> &'static str {
        truth_name(self.facts.truths[position])
    }

    /// The payload of the rule at `position`, a valid position, in JSON.
    pub(crate) fn rule_output_text(&self, position: usize) -> Option<String> {
        self.facts.rule_outputs[position]
            .as_ref()
            .map(|pairs| to_string(&write_value(&map_of(pairs))))
    }

    /// The rule set.
    pub(crate) fn ruleset(&self) -> &RuleSet {
        &self.ruleset
    }

    /// Make the decision again and read it; it succeeded before, so it
    /// succeeds now.
    fn with<T>(&self, env: &Env, read: impl FnOnce(&CoreDecision<'_>) -> T) -> Result<T> {
        let decision = match &self.source {
            Source::Input(input) => self.ruleset.evaluate(input),
            Source::Recorded(json, limits) => {
                read_parsed_decision_document(json, &self.ruleset, limits)
            }
        }
        .or_throw(env)?;
        Ok(read(&decision))
    }

    fn position(&self, env: &Env, rule: u32) -> Result<usize> {
        let count = self.facts.fired.len();
        usize::try_from(rule)
            .ok()
            .filter(|&r| r < count)
            .ok_or_else(|| {
                range_error(
                    env,
                    &format!("rule {rule} is out of the decision's {count} rules"),
                )
            })
    }
}

fn truth_name(truth: Truth) -> &'static str {
    match truth {
        Truth::True => "true",
        Truth::False => "false",
        Truth::Unknown => "unknown",
    }
}

#[napi]
impl Decision {
    /// How many rules the rule set has: one result each.
    #[napi(getter)]
    #[must_use]
    #[allow(clippy::cast_possible_truncation)] // A rule set never holds 2^32 rules.
    pub fn length(&self) -> u32 {
        self.facts.fired.len() as u32
    }

    /// The name of the rule at position `rule`.
    ///
    /// # Errors
    ///
    /// A `RangeError` for a position out of the rule set.
    #[napi]
    pub fn rule_name(&self, env: &Env, rule: u32) -> Result<String> {
        let position = self.position(env, rule)?;
        Ok(self.ruleset.rules()[position].name().to_owned())
    }

    /// Whether the rule at position `rule` fired.
    ///
    /// # Errors
    ///
    /// A `RangeError` for a position out of the rule set.
    #[napi]
    pub fn fired_at(&self, env: &Env, rule: u32) -> Result<bool> {
        Ok(self.facts.fired[self.position(env, rule)?])
    }

    /// The truth of the rule's condition: `"true"`, `"false"` or `"unknown"`.
    /// Under a graded logic (phase J) it is the crisp truth of its degree.
    ///
    /// # Errors
    ///
    /// A `RangeError` for a position out of the rule set.
    #[napi(ts_return_type = "Truth")]
    pub fn truth(&self, env: &Env, rule: u32) -> Result<&'static str> {
        Ok(self.truth_name(self.position(env, rule)?))
    }

    /// The positions of the selected rules, in rule order.
    #[napi]
    #[must_use]
    #[allow(clippy::cast_possible_truncation)] // A rule set never holds 2^32 rules.
    pub fn selected(&self) -> Vec<u32> {
        self.facts.selected.iter().map(|&p| p as u32).collect()
    }

    /// The output of a single-hit decision, as a JSON object, or `null` when no
    /// rule was selected; with an aggregate, the aggregated values.
    ///
    /// # Errors
    ///
    /// `HitPolicyError` for a multi-hit decision, which has `outputsJson()`
    /// instead (`spec/rules.md`, *Outputs*).
    #[napi]
    pub fn output_json(&self, env: &Env) -> Result<Option<String>> {
        match &self.facts.output {
            Ok(output) => Ok(output.as_ref().map(|value| to_string(&write_value(value)))),
            Err(error) => Err(throw(env, error, None, None)),
        }
    }

    /// The outputs of the selected rules, in rule order, as a JSON array.
    #[napi]
    #[must_use]
    pub fn outputs_json(&self) -> String {
        let outputs = self.facts.outputs.iter().map(|pairs| map_of(pairs));
        to_string(&write_value(&Value::List(outputs.collect())))
    }

    /// The evaluated payload of the rule at position `rule` as a JSON object,
    /// or `null` when it was not evaluated (`spec/rules.md`, *When payloads
    /// are evaluated*).
    ///
    /// # Errors
    ///
    /// A `RangeError` for a position out of the rule set.
    #[napi]
    pub fn rule_output_json(&self, env: &Env, rule: u32) -> Result<Option<String>> {
        Ok(self.rule_output_text(self.position(env, rule)?))
    }

    /// The decision document in `format`: JSON (the default) or YAML.
    ///
    /// # Errors
    ///
    /// `UnsupportedError` for the rule syntax and CSV.
    #[napi(ts_args_type = "format?: Format")]
    pub fn write(&self, env: &Env, format: Option<String>) -> Result<String> {
        let format = limits::format(env, format.as_deref(), DocumentFormat::Json)?;
        self.with(env, |d| write_decision_as(d, format))?
            .or_throw(env)
    }

    /// The explanation (`spec/explain.md`) in `format`: its text (the
    /// default, as in C++), or a `"json"` or `"yaml"` document.
    ///
    /// # Errors
    ///
    /// `UnsupportedError` for CSV.
    #[napi(ts_args_type = "format?: Format")]
    pub fn explain(&self, env: &Env, format: Option<String>) -> Result<String> {
        let format = limits::format(env, format.as_deref(), DocumentFormat::Text)?;
        self.with(env, |d| write_explanation_as(&explain(d), format))?
            .or_throw(env)
    }
}
