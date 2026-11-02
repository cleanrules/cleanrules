//! Rules, rule sets and decisions (`spec/rules.md`).

use crate::ast::{Kind, Node};
use crate::error::{Error, ErrorCode, Result};
use crate::eval::{Evaluated, Options, evaluate_tree, evaluate_value_tree};
use crate::logic::{Kleene, Logic, LogicName, Truth};
use crate::value::{Map, NullCause, Number, Value};

/// A rule: a condition, a payload and descriptive data (`spec/rules.md`,
/// *Rules*). Actions are code of the host language and live in the bindings;
/// a rule holds only the name of its action, if it names one.
#[derive(Clone, Debug)]
pub struct Rule {
    name: String,
    when: Node,
    then: Vec<(String, Node)>,
    priority: i64,
    tags: Vec<String>,
    description: Option<String>,
    metadata: Map,
    action: Option<String>,
}

fn rule_error(rule: &str, message: impl Into<String>) -> Error {
    let mut error = Error::new(ErrorCode::RuleDefinitionError, message);
    if !rule.is_empty() {
        error.context_mut().rule = Some(rule.to_owned());
    }
    error
}

impl Rule {
    /// A rule with a name and a condition, and no payload.
    ///
    /// # Errors
    ///
    /// `expression_error` when `when` is the null constant; `rule_definition_error`
    /// when the name is empty or `when` is a value.
    pub fn new(name: impl Into<String>, when: Node) -> Result<Self> {
        Self::with_payload(name, when, Vec::new())
    }

    /// A rule with a payload, keys in order (a repeated key keeps its first place
    /// and its last value).
    ///
    /// # Errors
    ///
    /// As [`Rule::new`], and `rule_definition_error` when a payload value is a
    /// condition, naming the first such key.
    pub fn with_payload(
        name: impl Into<String>,
        when: Node,
        then: Vec<(String, Node)>,
    ) -> Result<Self> {
        let name = name.into();
        if when.is_null_constant() {
            return Err(Error::new(
                ErrorCode::ExpressionError,
                "a null constant cannot be a rule's condition",
            )
            .with_hint("test for missing values with is_null"));
        }
        if name.is_empty() {
            return Err(rule_error("", "a rule name cannot be empty"));
        }
        if when.kind() == Kind::Value {
            return Err(rule_error(
                &name,
                format!(
                    "the condition of {name:?} is a value ('{}'), not a condition",
                    when.tag()
                ),
            )
            .with_hint("compare it, for example `(a + b) > 0`"));
        }
        let mut payload: Vec<(String, Node)> = Vec::with_capacity(then.len());
        for (key, value) in then {
            if value.kind() == Kind::Condition {
                return Err(rule_error(
                    &name,
                    format!(
                        "the payload value {key:?} is a condition ('{}')",
                        value.tag()
                    ),
                )
                .with_hint("payload values are constants or value expressions"));
            }
            match payload.iter_mut().find(|(k, _)| *k == key) {
                Some(entry) => entry.1 = value,
                None => payload.push((key, value)),
            }
        }
        Ok(Self {
            name,
            when,
            then: payload,
            priority: 0,
            tags: Vec::new(),
            description: None,
            metadata: Map::new(),
            action: None,
        })
    }

    /// Set the priority, used by the `priority` policy.
    #[must_use]
    pub fn priority(mut self, priority: i64) -> Self {
        self.priority = priority;
        self
    }

    /// Set the tags, a set: duplicates are dropped and they are kept sorted.
    #[must_use]
    pub fn tags<S: Into<String>>(mut self, tags: impl IntoIterator<Item = S>) -> Self {
        let mut tags: Vec<String> = tags.into_iter().map(Into::into).collect();
        tags.sort();
        tags.dedup();
        self.tags = tags;
        self
    }

    /// Set the description.
    #[must_use]
    pub fn description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Set the metadata.
    #[must_use]
    pub fn metadata(mut self, metadata: Map) -> Self {
        self.metadata = metadata;
        self
    }

    /// Name the rule's action, which a binding resolves in its action registry
    /// (`spec/rules.md`, *Actions*). The core never runs it.
    ///
    /// # Errors
    ///
    /// `rule_definition_error` when the name is empty.
    pub fn action_name(mut self, name: impl Into<String>) -> Result<Self> {
        let name = name.into();
        if name.is_empty() {
            return Err(rule_error(&self.name, "an action name cannot be empty")
                .with_hint("leave the action out, or name one the program registers"));
        }
        self.action = Some(name);
        Ok(self)
    }

    /// The name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The condition.
    #[must_use]
    pub fn when(&self) -> &Node {
        &self.when
    }

    /// The payload, keys in order.
    #[must_use]
    pub fn then(&self) -> &[(String, Node)] {
        &self.then
    }

    /// The priority.
    #[must_use]
    pub fn get_priority(&self) -> i64 {
        self.priority
    }

    /// The tags, sorted.
    #[must_use]
    pub fn get_tags(&self) -> &[String] {
        &self.tags
    }

    /// The description.
    #[must_use]
    pub fn get_description(&self) -> Option<&str> {
        self.description.as_deref()
    }

    /// The metadata.
    #[must_use]
    pub fn get_metadata(&self) -> &Map {
        &self.metadata
    }

    /// The name of the rule's action, if it names one.
    #[must_use]
    pub fn get_action_name(&self) -> Option<&str> {
        self.action.as_deref()
    }
}

/// How a rule set turns the rules that fired into a decision.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum HitPolicy {
    /// The first fired rule.
    #[default]
    First,
    /// The fired rule with the highest priority; ties go to rule order.
    Priority,
    /// The only fired rule; more than one is an error.
    Unique,
    /// The first fired rule; fired rules must agree on their outputs.
    Any,
    /// Every fired rule, in rule order.
    RuleOrder,
    /// Every fired rule, in rule order, optionally aggregated.
    Collect,
}

impl HitPolicy {
    /// The name in the format.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            HitPolicy::First => "first",
            HitPolicy::Priority => "priority",
            HitPolicy::Unique => "unique",
            HitPolicy::Any => "any",
            HitPolicy::RuleOrder => "rule_order",
            HitPolicy::Collect => "collect",
        }
    }

    /// The policy for a name (lowercase, exact).
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "first" => HitPolicy::First,
            "priority" => HitPolicy::Priority,
            "unique" => HitPolicy::Unique,
            "any" => HitPolicy::Any,
            "rule_order" => HitPolicy::RuleOrder,
            "collect" => HitPolicy::Collect,
            _ => return None,
        })
    }

    /// Whether at most one rule is selected.
    #[must_use]
    pub fn single_hit(self) -> bool {
        !matches!(self, HitPolicy::RuleOrder | HitPolicy::Collect)
    }
}

/// A `collect` aggregate, applied per payload key.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Aggregate {
    /// The sum of the non-null values.
    Sum,
    /// The smallest non-null value.
    Min,
    /// The largest non-null value.
    Max,
    /// The number of non-null values.
    Count,
}

impl Aggregate {
    /// The name in the format.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Aggregate::Sum => "sum",
            Aggregate::Min => "min",
            Aggregate::Max => "max",
            Aggregate::Count => "count",
        }
    }

    /// The aggregate for a name.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "sum" => Aggregate::Sum,
            "min" => Aggregate::Min,
            "max" => Aggregate::Max,
            "count" => Aggregate::Count,
            _ => return None,
        })
    }
}

/// How a rule set decides.
#[derive(Clone, Copy, Debug, Default)]
pub struct Config {
    /// The hit policy.
    pub hit_policy: HitPolicy,
    /// The aggregate, only with `collect`.
    pub aggregate: Option<Aggregate>,
    /// The logic.
    pub logic: Option<LogicName>,
    /// Raise instead of returning unknown.
    pub strict: bool,
}

/// Rules evaluated together under a hit policy.
#[derive(Clone, Debug)]
pub struct RuleSet {
    rules: Vec<Rule>,
    hit_policy: HitPolicy,
    aggregate: Option<Aggregate>,
    logic: LogicName,
    strict: bool,
}

impl RuleSet {
    /// A rule set.
    ///
    /// # Errors
    ///
    /// `rule_definition_error` for a repeated rule name (naming it), or an
    /// aggregate without the `collect` policy.
    pub fn new(rules: Vec<Rule>, config: Config) -> Result<Self> {
        let mut names = std::collections::BTreeSet::new();
        for rule in &rules {
            if !names.insert(rule.name.as_str()) {
                return Err(
                    rule_error(&rule.name, format!("two rules are named {:?}", rule.name))
                        .with_hint("rule names must be unique"),
                );
            }
        }
        if config.aggregate.is_some() && config.hit_policy != HitPolicy::Collect {
            return Err(rule_error("", "an aggregate needs the collect hit policy"));
        }
        Ok(Self {
            rules,
            hit_policy: config.hit_policy,
            aggregate: config.aggregate,
            logic: config.logic.unwrap_or(LogicName::Boolean),
            strict: config.strict,
        })
    }

    /// Check that every action the rules name is among `registered`, in rule
    /// order (`spec/rules.md`, *Actions*). Bindings call it when they bind
    /// their action registry, and before `run`; it only compares names.
    ///
    /// # Errors
    ///
    /// `registry_error` for the first rule whose action is not registered,
    /// with the rule, the name and the registered names, sorted.
    pub fn check_action_names<S: AsRef<str>>(&self, registered: &[S]) -> Result<()> {
        let Some(rule) = self.rules.iter().find(|rule| {
            rule.get_action_name()
                .is_some_and(|name| !registered.iter().any(|r| r.as_ref() == name))
        }) else {
            return Ok(());
        };
        let name = rule.get_action_name().unwrap_or_default();
        let mut available: Vec<String> = registered.iter().map(|r| r.as_ref().to_owned()).collect();
        available.sort();
        available.dedup();
        let mut error = Error::new(
            ErrorCode::RegistryError,
            format!(
                "the action {name:?} of rule {:?} is not registered",
                rule.name()
            ),
        )
        .with_hint("register an action by this name in the program, and bind the rules to it");
        let context = error.context_mut();
        context.rule = Some(rule.name().to_owned());
        context.name = Some(name.to_owned());
        context.available = available;
        Err(error)
    }

    /// The rules, in rule order.
    #[must_use]
    pub fn rules(&self) -> &[Rule] {
        &self.rules
    }

    /// The hit policy.
    #[must_use]
    pub fn hit_policy(&self) -> HitPolicy {
        self.hit_policy
    }

    /// The aggregate.
    #[must_use]
    pub fn aggregate(&self) -> Option<Aggregate> {
        self.aggregate
    }

    /// The logic.
    #[must_use]
    pub fn logic(&self) -> LogicName {
        self.logic
    }

    /// Whether evaluation is strict.
    #[must_use]
    pub fn strict(&self) -> bool {
        self.strict
    }

    /// Evaluate on one input (`spec/rules.md`, *Evaluation*). Pure: no action runs.
    ///
    /// # Errors
    ///
    /// In strict mode, the evaluation errors of conditions then payloads; and
    /// `hit_policy_error` when the policy cannot decide.
    pub fn evaluate<'a>(&'a self, input: &Value) -> Result<Decision<'a>> {
        match self.logic {
            LogicName::Boolean => self.decide(input, Kleene),
        }
    }

    /// Evaluate on a batch, one decision per input.
    ///
    /// # Errors
    ///
    /// As [`RuleSet::evaluate`], for the first input that fails, with its `row`.
    pub fn evaluate_many<'a>(&'a self, inputs: &[Value]) -> Result<Vec<Decision<'a>>> {
        inputs
            .iter()
            .enumerate()
            .map(|(row, input)| {
                self.evaluate(input).map_err(|mut error| {
                    error.context_mut().row = Some(row as u64);
                    error
                })
            })
            .collect()
    }

    fn decide<'a>(&'a self, input: &Value, logic: Kleene) -> Result<Decision<'a>> {
        let mut results = Vec::with_capacity(self.rules.len());
        for rule in &self.rules {
            let options = Options {
                strict: self.strict,
                rule: Some(&rule.name),
            };
            let when = evaluate_tree(&rule.when, input, &logic, options)?;
            let fired = when.truth().is_some_and(|t| logic.fires(t));
            results.push(RuleResult {
                rule,
                fired,
                when,
                then: None,
            });
        }
        let fired: Vec<usize> = (0..results.len()).filter(|&i| results[i].fired).collect();
        if self.hit_policy == HitPolicy::Any {
            for &i in &fired {
                results[i].then = Some(self.payload(&self.rules[i], input, logic)?);
            }
        }
        let selected = self.select(&fired)?;
        for &i in &selected {
            if results[i].then.is_none() {
                results[i].then = Some(self.payload(&self.rules[i], input, logic)?);
            }
        }
        if self.hit_policy == HitPolicy::Any && !Self::outputs_agree(&results, &fired) {
            return Err(self.policy_error(
                "rules fired with different outputs under the any hit policy".into(),
                &fired,
                "give them the same payload, or use first, priority or collect",
            ));
        }
        let aggregated = match self.aggregate {
            Some(aggregate) => Some(self.aggregated(aggregate, &results, &selected)?),
            None => None,
        };
        Ok(Decision {
            ruleset: self,
            results,
            selected,
            aggregated,
        })
    }

    /// The rules the hit policy selects among those that fired (`spec/rules.md`).
    pub(crate) fn select(&self, fired: &[usize]) -> Result<Vec<usize>> {
        Ok(match self.hit_policy {
            HitPolicy::First | HitPolicy::Any => fired.iter().copied().take(1).collect(),
            HitPolicy::Priority => {
                let top = fired.iter().map(|&i| self.rules[i].priority).max();
                fired
                    .iter()
                    .copied()
                    .find(|&i| Some(self.rules[i].priority) == top)
                    .into_iter()
                    .collect()
            }
            HitPolicy::Unique => {
                if fired.len() > 1 {
                    return Err(self.policy_error(
                        format!("{} rules fired under the unique hit policy", fired.len()),
                        fired,
                        "make the conditions mutually exclusive, or use first or priority",
                    ));
                }
                fired.to_vec()
            }
            HitPolicy::RuleOrder | HitPolicy::Collect => fired.to_vec(),
        })
    }

    fn payload<'a>(
        &self,
        rule: &'a Rule,
        input: &Value,
        logic: Kleene,
    ) -> Result<Vec<(String, Evaluated<'a, Truth>)>> {
        let options = Options {
            strict: self.strict,
            rule: Some(&rule.name),
        };
        rule.then
            .iter()
            .map(|(key, node)| {
                Ok((
                    key.clone(),
                    evaluate_value_tree(node, input, &logic, options)?,
                ))
            })
            .collect()
    }

    fn policy_error(&self, message: String, indices: &[usize], hint: &str) -> Error {
        let mut error = Error::new(ErrorCode::HitPolicyError, message).with_hint(hint);
        error.context_mut().rules = indices
            .iter()
            .map(|&i| self.rules[i].name.clone())
            .collect();
        error
    }

    pub(crate) fn outputs_agree(results: &[RuleResult<'_>], fired: &[usize]) -> bool {
        let Some((&first, rest)) = fired.split_first() else {
            return true;
        };
        let reference = results[first].output();
        rest.iter().all(|&i| {
            let other = results[i].output();
            reference.len() == other.len()
                && reference
                    .iter()
                    .zip(&other)
                    .all(|((k, a), (l, b))| k == l && a.any_equal(b))
        })
    }

    pub(crate) fn aggregated(
        &self,
        aggregate: Aggregate,
        results: &[RuleResult<'_>],
        selected: &[usize],
    ) -> Result<Aggregated> {
        if selected.is_empty() {
            return Ok(Aggregated::NothingSelected);
        }
        let mut keys: Vec<(String, Vec<Value>)> = Vec::new();
        for &i in selected {
            for (key, value) in results[i].output() {
                let slot = if let Some(at) = keys.iter().position(|(k, _)| *k == key) {
                    at
                } else {
                    keys.push((key.clone(), Vec::new()));
                    keys.len() - 1
                };
                if value.is_null() {
                    continue;
                }
                if aggregate != Aggregate::Count && value.as_number().is_none() {
                    return Err(self.policy_error(
                        format!(
                            "cannot {} the non-numeric output {key:?}",
                            aggregate.as_str()
                        ),
                        &[i],
                        "aggregate only numeric payloads, or use collect without aggregate",
                    ));
                }
                keys[slot].1.push(value);
            }
        }
        Ok(Aggregated::Values(
            keys.into_iter()
                .map(|(key, values)| (key, reduce(aggregate, &values)))
                .collect(),
        ))
    }
}

/// One key's aggregate over its non-null values, as the Python v1 computed it.
fn reduce(aggregate: Aggregate, values: &[Value]) -> Value {
    if aggregate == Aggregate::Count {
        return Value::Int(i64::try_from(values.len()).unwrap_or(i64::MAX));
    }
    if values.is_empty() {
        return Value::Null(NullCause::Null);
    }
    let any_decimal = values.iter().any(|v| matches!(v, Value::Decimal(_)));
    // Decimals turn every value into a float, as arithmetic does with decimal operands.
    let numbers: Vec<Number> = values
        .iter()
        .filter_map(Value::as_number)
        .map(|n| {
            if any_decimal {
                Number::Float(n.to_f64())
            } else {
                n
            }
        })
        .collect();
    let to_value = |n: Number| match n {
        Number::Int(i) => Value::Int(i),
        Number::Float(f) => Value::float(f),
        Number::Decimal(d) => Value::Decimal(d),
    };
    match aggregate {
        Aggregate::Min | Aggregate::Max => {
            // The first of equal values wins, as Python's min and max.
            let mut best = numbers[0];
            for &n in &numbers[1..] {
                let order = n.compare(best);
                let better = if aggregate == Aggregate::Min {
                    order.is_lt()
                } else {
                    order.is_gt()
                };
                if better {
                    best = n;
                }
            }
            to_value(best)
        }
        _ => sum(&numbers),
    }
}

/// Python's `sum`: an exact integer until a float appears, then a float; an int
/// outside 64 bits or a NaN result is null.
fn sum(numbers: &[Number]) -> Value {
    enum Total {
        Int(i128),
        Float(f64),
    }
    let mut total = Total::Int(0);
    for n in numbers {
        total = match (total, n) {
            (Total::Int(acc), Number::Int(i)) => Total::Int(acc + i128::from(*i)),
            // Rounds to nearest, as Python's int-to-float conversion.
            #[allow(clippy::cast_precision_loss)]
            (Total::Int(acc), other) => Total::Float(acc as f64 + other.to_f64()),
            (Total::Float(acc), other) => Total::Float(acc + other.to_f64()),
        };
    }
    match total {
        Total::Int(i) => i64::try_from(i).map_or(Value::Null(NullCause::Null), Value::Int),
        Total::Float(f) => Value::float(f),
    }
}

/// The aggregate of a decision whose rule set has one.
#[derive(Clone, Debug)]
pub enum Aggregated {
    /// No rule was selected: the aggregate is null.
    NothingSelected,
    /// Each payload key with its aggregate, keys in order of first appearance.
    Values(Map),
}

/// One rule's result in a decision.
#[derive(Clone, Debug)]
pub struct RuleResult<'a> {
    /// The rule.
    pub rule: &'a Rule,
    /// Whether its truth value passed the logic's activation test.
    pub fired: bool,
    /// The result tree of its condition.
    pub when: Evaluated<'a, Truth>,
    /// The result trees of its payload values, when they were evaluated.
    pub then: Option<Vec<(String, Evaluated<'a, Truth>)>>,
}

impl RuleResult<'_> {
    /// The output: each payload key with the value at the root of its tree; empty
    /// when the payload was not evaluated.
    #[must_use]
    pub fn output(&self) -> Vec<(String, Value)> {
        self.then
            .iter()
            .flatten()
            .map(|(key, tree)| {
                (
                    key.clone(),
                    tree.value()
                        .cloned()
                        .unwrap_or(Value::Null(NullCause::Null)),
                )
            })
            .collect()
    }
}

/// What evaluating a rule set on one input produced (`spec/rules.md`).
#[derive(Clone, Debug)]
pub struct Decision<'a> {
    ruleset: &'a RuleSet,
    results: Vec<RuleResult<'a>>,
    selected: Vec<usize>,
    aggregated: Option<Aggregated>,
}

impl<'a> Decision<'a> {
    /// A decision from its parts, for readers that rebuild one.
    pub(crate) fn from_parts(
        ruleset: &'a RuleSet,
        results: Vec<RuleResult<'a>>,
        selected: Vec<usize>,
        aggregated: Option<Aggregated>,
    ) -> Self {
        Self {
            ruleset,
            results,
            selected,
            aggregated,
        }
    }

    /// The rule set that produced it.
    #[must_use]
    pub fn ruleset(&self) -> &'a RuleSet {
        self.ruleset
    }

    /// Every rule's result, in rule order.
    #[must_use]
    pub fn results(&self) -> &[RuleResult<'a>] {
        &self.results
    }

    /// The selected rules' results, in rule order.
    pub fn selected(&self) -> impl Iterator<Item = &RuleResult<'a>> {
        self.selected.iter().map(|&i| &self.results[i])
    }

    /// The outputs of the selected rules, in rule order.
    #[must_use]
    pub fn outputs(&self) -> Vec<Vec<(String, Value)>> {
        self.selected().map(RuleResult::output).collect()
    }

    /// The aggregate, when the rule set has one.
    #[must_use]
    pub fn aggregated(&self) -> Option<&Aggregated> {
        self.aggregated.as_ref()
    }

    /// The single output: the winner's for single-hit policies (`None` when
    /// nothing was selected), or the aggregate.
    ///
    /// # Errors
    ///
    /// `hit_policy_error` for `rule_order`, and `collect` without an aggregate:
    /// they have several outputs.
    pub fn output(&self) -> Result<Option<Value>> {
        match &self.aggregated {
            Some(Aggregated::Values(map)) => return Ok(Some(Value::Map(map.clone()))),
            Some(Aggregated::NothingSelected) => return Ok(None),
            None => {}
        }
        if !self.ruleset.hit_policy.single_hit() {
            return Err(Error::new(
                ErrorCode::HitPolicyError,
                format!(
                    "{} decisions have several outputs",
                    self.ruleset.hit_policy.as_str()
                ),
            )
            .with_hint("read the outputs, or use collect with an aggregate"));
        }
        Ok(self
            .selected()
            .next()
            .map(|r| Value::Map(r.output().into_iter().collect())))
    }
}

#[cfg(test)]
mod tests {
    use super::{Aggregate, Config, HitPolicy, Rule, RuleSet, sum};
    use crate::build::{constant, field};
    use crate::error::ErrorCode;
    use crate::value::{Map, Number, Value};

    fn input(x: i64) -> Value {
        let mut map = Map::new();
        map.insert("x", Value::Int(x));
        Value::Map(map)
    }

    #[test]
    fn rule_construction() {
        let gt = || field("x").gt(1).build().unwrap();
        assert_eq!(
            Rule::new("", gt()).unwrap_err().code(),
            ErrorCode::RuleDefinitionError
        );
        let value = (field("x") + 1).build().unwrap();
        assert_eq!(
            Rule::new("r", value).unwrap_err().code(),
            ErrorCode::RuleDefinitionError
        );
        let payload = vec![("ok".to_owned(), gt())];
        assert_eq!(
            Rule::with_payload("r", gt(), payload).unwrap_err().code(),
            ErrorCode::RuleDefinitionError
        );
        let rule = Rule::new("r", gt()).unwrap().tags(["b", "a", "b"]);
        assert_eq!(rule.get_tags(), ["a", "b"]);
        assert_eq!(rule.get_action_name(), None);
        let error = rule.clone().action_name("").unwrap_err();
        assert_eq!(
            (error.code(), error.context().rule.as_deref()),
            (ErrorCode::RuleDefinitionError, Some("r"))
        );
        let named = rule.action_name("notify").unwrap();
        assert_eq!(named.get_action_name(), Some("notify"));
        let ruleset = RuleSet::new(
            vec![Rule::new("q", gt()).unwrap(), named],
            Config::default(),
        )
        .unwrap();
        assert!(ruleset.check_action_names(&["notify"]).is_ok());
        let error = ruleset
            .check_action_names(&["zz", "log", "log"])
            .unwrap_err();
        let context = error.context();
        assert_eq!(error.code(), ErrorCode::RegistryError);
        assert_eq!(
            (context.rule.as_deref(), context.name.as_deref()),
            (Some("r"), Some("notify"))
        );
        assert_eq!(context.available, ["log", "zz"]);
        let twice = vec![Rule::new("r", gt()).unwrap(), Rule::new("r", gt()).unwrap()];
        assert_eq!(
            RuleSet::new(twice, Config::default()).unwrap_err().code(),
            ErrorCode::RuleDefinitionError
        );
        let config = Config {
            aggregate: Some(Aggregate::Sum),
            ..Config::default()
        };
        assert_eq!(
            RuleSet::new(vec![], config).unwrap_err().code(),
            ErrorCode::RuleDefinitionError
        );
    }

    #[test]
    fn outputs_of_a_decision() {
        let rule = |name: &str, n: i64| {
            Rule::with_payload(
                name,
                field("x").gt(0).build().unwrap(),
                vec![("n".into(), constant(n).build().unwrap())],
            )
            .unwrap()
        };
        let all = Config {
            hit_policy: HitPolicy::RuleOrder,
            ..Config::default()
        };
        let ruleset = RuleSet::new(vec![rule("a", 1), rule("b", 2)], all).unwrap();
        let decision = ruleset.evaluate(&input(5)).unwrap();
        assert_eq!(decision.outputs().len(), 2);
        assert_eq!(
            decision.output().unwrap_err().code(),
            ErrorCode::HitPolicyError
        );
        let first = RuleSet::new(vec![rule("a", 1), rule("b", 2)], Config::default()).unwrap();
        let output = first
            .evaluate(&input(5))
            .unwrap()
            .output()
            .unwrap()
            .unwrap();
        assert!(matches!(output, Value::Map(m) if matches!(m.get("n"), Some(Value::Int(1)))));
        assert!(
            first
                .evaluate(&input(-1))
                .unwrap()
                .output()
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn sum_is_exact_until_a_float() {
        let big = 1_i64 << 62;
        assert!(matches!(
            sum(&[Number::Int(big), Number::Int(big)]),
            Value::Null(_)
        ));
        assert!(
            matches!(sum(&[Number::Int(big), Number::Int(big), Number::Int(-big)]), Value::Int(i) if i == big)
        );
        assert!(matches!(
            sum(&[Number::Int(big), Number::Int(big), Number::Float(1.5)]),
            Value::Float(_)
        ));
        assert!(matches!(
            sum(&[
                Number::Float(f64::INFINITY),
                Number::Float(f64::NEG_INFINITY)
            ]),
            Value::Null(_)
        ));
    }
}
