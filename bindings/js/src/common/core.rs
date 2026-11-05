//! Documents, rule sets, decisions and nodes over the core, for both front
//! ends. The core decides everything; this code reads and writes texts,
//! keeps what a decision says, and lists the actions to run.
//!
//! A core `Decision` borrows its rule set, so a JavaScript decision keeps the
//! rule set and what it was made from (the input, or a recorded decision),
//! with what is read at once. Writing or explaining it makes it again:
//! evaluation is deterministic (`CLAUDE.md`, invariant 7), so it is the same
//! decision. Values come out in the JSON format (`spec/json-format.md`,
//! *Values*); native JavaScript values for outputs wait for the specification
//! of that direction (phase F.2).

use std::sync::Arc;

use cleanrules::ast::Node;
use cleanrules::eval::{Options, evaluate_tree, evaluate_value_tree};
use cleanrules::explain::{explain, summary};
use cleanrules::format::{
    Document, DocumentFormat, ValueContext, parse_as, read_document_as, read_node,
    read_parsed_decision_document, read_value, write_decision_as, write_document_as,
    write_explanation_as, write_node, write_result, write_summary_as, write_value,
};
use cleanrules::json::{Json, Limits, parse_bytes, to_string, utf8_text};
use cleanrules::logic::{Kleene, LogicName, Truth};
use cleanrules::rules::{Decision, RuleSet};
use cleanrules::{Error, ErrorCode, Map, Segment, Value};

use super::{Outcome, Problem, Raw, RawOptions, TreeOptions, position, read_options};

/// A segment of a field path, for JavaScript: a key or an index.
pub(crate) enum PathSegment<'a> {
    Key(&'a str),
    Index(f64),
}

/// A field path as JavaScript gets it.
pub(crate) fn path_segments(path: &[Segment]) -> Vec<PathSegment<'_>> {
    path.iter()
        .map(|segment| match segment {
            Segment::Key(key) => PathSegment::Key(key),
            // Indices beyond 2^53 cannot be told apart in JavaScript anyway.
            #[allow(clippy::cast_precision_loss)]
            Segment::Index(index) => PathSegment::Index(*index as f64),
        })
        .collect()
}

// --- Documents ------------------------------------------------------------

/// A document read from a text.
pub(crate) struct DocumentCore {
    pub(crate) document: Document,
}

impl DocumentCore {
    pub(crate) fn read(text: &[u8], options: Option<&RawOptions>) -> Outcome<Self> {
        let (format, limits) = read_options(options)?;
        Ok(Self {
            document: read_document_as(text, format, &limits)?,
        })
    }

    pub(crate) fn kind(&self) -> &'static str {
        match &self.document {
            Document::Condition(_) => "condition",
            Document::Rule(_) => "rule",
            Document::RuleSet(_) => "ruleset",
            Document::DecisionTable(_) => "decision_table",
            // The core's documents are `#[non_exhaustive]`.
            _ => "unknown",
        }
    }

    pub(crate) fn write(&self, format: &Raw) -> Outcome<String> {
        let format = super::format(format, DocumentFormat::Json)?;
        Ok(write_document_as(&self.document, format)?)
    }
}

// --- Rule sets ------------------------------------------------------------

/// A rule set, shared by its decisions.
pub(crate) struct RulesCore {
    pub(crate) ruleset: Arc<RuleSet>,
}

/// A batch error: inputs must come in an array (`spec/rules.md`, *Evaluation*).
pub(crate) fn not_a_batch() -> Problem {
    Problem::Core(
        Error::new(ErrorCode::EvaluationError, "a batch is a list of inputs")
            .with_hint("pass the inputs as the elements of an array"),
    )
}

/// An `undefined` input in a batch, with its row.
pub(crate) fn undefined_row(row: usize) -> Problem {
    let mut error = Error::new(
        ErrorCode::EvaluationError,
        format!("input {row} of the batch is undefined"),
    )
    .with_hint("pass the data to evaluate (null for none)");
    error.context_mut().row = Some(row as u64);
    Problem::Core(error)
}

/// Where a `read_error` met while reading an input came from: the index of the
/// thrown value whose path is `error`'s path or one of its prefixes, so that
/// strict mode chains it as the `cause` (`spec/ast.md`, *Missing values*).
pub(crate) fn cause_index(error: &Error, errors: &[(Vec<Segment>, u32)]) -> Option<u32> {
    if error.code() != ErrorCode::EvaluationError {
        return None;
    }
    let path = error.context().path.as_ref()?;
    errors
        .iter()
        .find(|(at, _)| path.starts_with(at))
        .map(|&(_, index)| index)
}

/// An action to call: the rule's position, its name and the action's name.
pub(crate) struct ActionStep {
    pub(crate) position: usize,
    pub(crate) rule: String,
    pub(crate) action: String,
    /// The truth of the rule's condition.
    pub(crate) truth: &'static str,
    /// The rule's evaluated payload in JSON (a selected rule's payload is
    /// always evaluated; `{}` for a rule without payload).
    pub(crate) output: String,
}

/// How an action failed.
pub(crate) enum ActionFailure {
    /// It threw; `detail` describes what.
    Threw(String),
    /// It returned a promise (any thenable): actions are synchronous.
    Promise,
}

/// The `action_error` of an action that failed (`spec/rules.md`, *Actions*),
/// after the actions of `executed` ran.
pub(crate) fn action_error(
    step: &ActionStep,
    failure: &ActionFailure,
    executed: Vec<String>,
) -> Error {
    let (message, detail) = match failure {
        ActionFailure::Threw(detail) => (
            format!(
                "the action {:?} of rule {:?} threw {detail}",
                step.action, step.rule
            ),
            detail.clone(),
        ),
        ActionFailure::Promise => (
            format!(
                "the action {:?} of rule {:?} returned a Promise; actions are synchronous",
                step.action, step.rule
            ),
            "a Promise".to_owned(),
        ),
    };
    let mut error = Error::new(ErrorCode::ActionError, message)
        .with_hint("later actions did not run; error.decision is the decision")
        .caused_by(Thrown(detail));
    error.context_mut().rule = Some(step.rule.clone());
    error.context_mut().executed = executed;
    error
}

/// What an action threw, described, as the core error's cause: its JSON form
/// gives the description as `cause`, and JavaScript gets the value itself.
#[derive(Debug)]
struct Thrown(String);

impl std::fmt::Display for Thrown {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Thrown {}

impl RulesCore {
    fn new(ruleset: RuleSet) -> Self {
        Self {
            ruleset: Arc::new(ruleset),
        }
    }

    /// A rule set read from a text; a rule document is a rule set of that
    /// rule, and a decision table is compiled.
    pub(crate) fn read(text: &[u8], options: Option<&RawOptions>) -> Outcome<Self> {
        let (format, limits) = read_options(options)?;
        let ruleset = read_document_as(text, format, &limits).and_then(Document::into_ruleset)?;
        Ok(Self::new(ruleset))
    }

    pub(crate) fn from_document(document: &DocumentCore) -> Outcome<Self> {
        Ok(Self::new(document.document.clone().into_ruleset()?))
    }

    pub(crate) fn write(&self, format: &Raw) -> Outcome<String> {
        let format = super::format(format, DocumentFormat::Json)?;
        let document = Document::RuleSet(RuleSet::clone(&self.ruleset));
        Ok(write_document_as(&document, format)?)
    }

    /// The summary: its text by default, as in C++.
    pub(crate) fn summary(&self, format: &Raw) -> Outcome<String> {
        let format = super::format(format, DocumentFormat::Text)?;
        Ok(write_summary_as(&summary(&self.ruleset), format)?)
    }

    pub(crate) fn len(&self) -> usize {
        self.ruleset.rules().len()
    }

    pub(crate) fn rule_names(&self) -> Vec<String> {
        self.ruleset
            .rules()
            .iter()
            .map(|rule| rule.name().to_owned())
            .collect()
    }

    pub(crate) fn hit_policy(&self) -> &'static str {
        self.ruleset.hit_policy().as_str()
    }

    pub(crate) fn aggregate(&self) -> Option<&'static str> {
        self.ruleset
            .aggregate()
            .map(cleanrules::rules::Aggregate::as_str)
    }

    pub(crate) fn logic(&self) -> &'static str {
        self.ruleset.logic().as_str()
    }

    pub(crate) fn strict(&self) -> bool {
        self.ruleset.strict()
    }

    /// The field paths the rule set reads, in order of first appearance.
    pub(crate) fn field_paths(&self) -> Vec<Vec<Segment>> {
        summary(&self.ruleset)
            .fields
            .into_iter()
            .map(|field| field.path)
            .collect()
    }

    /// The decision on an input already converted.
    pub(crate) fn evaluate(&self, input: Value) -> cleanrules::Result<DecisionCore> {
        let decision = self.ruleset.evaluate(&input)?;
        Ok(DecisionCore::new(
            &self.ruleset,
            &decision,
            Source::Input(input),
        ))
    }

    /// The decision on an input given as a JSON value.
    pub(crate) fn evaluate_json(&self, text: &[u8], limits: &Limits) -> Outcome<DecisionCore> {
        let input =
            parse_bytes(text, limits).and_then(|json| read_value(&json, "", ValueContext::Data))?;
        Ok(self.evaluate(input)?)
    }

    /// The decisions on inputs already converted, in order.
    pub(crate) fn evaluate_many(
        &self,
        inputs: Vec<Value>,
    ) -> cleanrules::Result<Vec<DecisionCore>> {
        let decisions = self.ruleset.evaluate_many(&inputs)?;
        Ok(decisions
            .iter()
            .zip(inputs)
            .map(|(decision, input)| {
                DecisionCore::new(&self.ruleset, decision, Source::Input(input))
            })
            .collect())
    }

    /// The decisions on a batch given as a JSON array.
    pub(crate) fn evaluate_many_json(
        &self,
        text: &[u8],
        limits: &Limits,
    ) -> Outcome<Vec<DecisionCore>> {
        let json = parse_bytes(text, limits)?;
        let Json::Array(items) = &json else {
            return Err(not_a_batch());
        };
        let inputs = items
            .iter()
            .enumerate()
            .map(|(row, item)| read_value(item, &format!("/{row}"), ValueContext::Data))
            .collect::<cleanrules::Result<Vec<_>>>()?;
        Ok(self.evaluate_many(inputs)?)
    }

    /// A recorded decision, checked against the rule set.
    pub(crate) fn read_decision(
        &self,
        text: &[u8],
        options: Option<&RawOptions>,
    ) -> Outcome<DecisionCore> {
        let (format, limits) = read_options(options)?;
        let json = parse_as(text, format, &limits)?;
        let decision = read_parsed_decision_document(&json, &self.ruleset, &limits)?;
        Ok(DecisionCore::new(
            &self.ruleset,
            &decision,
            Source::Recorded(json.clone(), limits),
        ))
    }

    pub(crate) fn check_actions(&self, names: &[String]) -> Outcome<()> {
        Ok(self.ruleset.check_action_names(names)?)
    }

    /// The actions to call for `decision`, in rule order: the selected rules
    /// that name an action in `names` (checked before, with
    /// [`RulesCore::check_actions`]).
    pub(crate) fn action_steps(
        &self,
        decision: &DecisionCore,
        names: &[String],
    ) -> Vec<ActionStep> {
        decision
            .facts
            .selected
            .iter()
            .filter_map(|&position| {
                let rule = &self.ruleset.rules()[position];
                let action = rule.get_action_name()?;
                names.iter().any(|n| n == action).then(|| ActionStep {
                    position,
                    rule: rule.name().to_owned(),
                    action: action.to_owned(),
                    truth: truth_name(decision.facts.truths[position]),
                    output: decision
                        .rule_output_text(position)
                        .unwrap_or_else(|| "{}".to_owned()),
                })
            })
            .collect()
    }
}

// --- Decisions ------------------------------------------------------------

/// What a decision was made from.
enum Source {
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
    fn of(decision: &Decision<'_>) -> Self {
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

fn truth_name(truth: Truth) -> &'static str {
    match truth {
        Truth::True => "true",
        Truth::False => "false",
        Truth::Unknown => "unknown",
    }
}

/// A decision: an evaluated input, or a recorded decision read with its rule
/// set.
pub(crate) struct DecisionCore {
    ruleset: Arc<RuleSet>,
    source: Source,
    facts: Facts,
}

impl DecisionCore {
    fn new(ruleset: &Arc<RuleSet>, decision: &Decision<'_>, source: Source) -> Self {
        Self {
            ruleset: Arc::clone(ruleset),
            source,
            facts: Facts::of(decision),
        }
    }

    /// Make the decision again and read it; it succeeded before, so it
    /// succeeds now.
    fn with<T>(&self, read: impl FnOnce(&Decision<'_>) -> cleanrules::Result<T>) -> Outcome<T> {
        let decision = match &self.source {
            Source::Input(input) => self.ruleset.evaluate(input),
            Source::Recorded(json, limits) => {
                read_parsed_decision_document(json, &self.ruleset, limits)
            }
        }?;
        Ok(read(&decision)?)
    }

    fn position(&self, rule: &Raw) -> Outcome<usize> {
        position(rule, self.facts.fired.len(), "decision")
    }

    pub(crate) fn len(&self) -> usize {
        self.facts.fired.len()
    }

    pub(crate) fn rule_name(&self, rule: &Raw) -> Outcome<String> {
        Ok(self.ruleset.rules()[self.position(rule)?].name().to_owned())
    }

    pub(crate) fn fired_at(&self, rule: &Raw) -> Outcome<bool> {
        Ok(self.facts.fired[self.position(rule)?])
    }

    pub(crate) fn truth(&self, rule: &Raw) -> Outcome<&'static str> {
        Ok(truth_name(self.facts.truths[self.position(rule)?]))
    }

    pub(crate) fn selected(&self) -> &[usize] {
        &self.facts.selected
    }

    /// The single output in JSON, `None` when nothing was selected.
    pub(crate) fn output_json(&self) -> Outcome<Option<String>> {
        match &self.facts.output {
            Ok(output) => Ok(output.as_ref().map(|value| to_string(&write_value(value)))),
            // The same error again, made by the core (errors are not `Clone`).
            Err(_) => match self.with(|d| d.output().map(|_| ())) {
                Err(problem) => Err(problem),
                Ok(()) => Err(Problem::Internal(
                    "an output that failed once succeeds; please report it".to_owned(),
                )),
            },
        }
    }

    pub(crate) fn outputs_json(&self) -> String {
        let outputs = self.facts.outputs.iter().map(|pairs| map_of(pairs));
        to_string(&write_value(&Value::List(outputs.collect())))
    }

    fn rule_output_text(&self, position: usize) -> Option<String> {
        self.facts.rule_outputs[position]
            .as_ref()
            .map(|pairs| to_string(&write_value(&map_of(pairs))))
    }

    pub(crate) fn rule_output_json(&self, rule: &Raw) -> Outcome<Option<String>> {
        Ok(self.rule_output_text(self.position(rule)?))
    }

    /// The decision document: JSON by default.
    pub(crate) fn write(&self, format: &Raw) -> Outcome<String> {
        let format = super::format(format, DocumentFormat::Json)?;
        self.with(|d| write_decision_as(d, format))
    }

    /// The explanation: its text by default, as in C++.
    pub(crate) fn explain(&self, format: &Raw) -> Outcome<String> {
        let format = super::format(format, DocumentFormat::Text)?;
        self.with(|d| write_explanation_as(&explain(d), format))
    }
}

// --- Nodes ----------------------------------------------------------------

/// One condition or value node.
pub(crate) struct NodeCore {
    node: Node,
}

fn not_csv(format: DocumentFormat) -> Error {
    Error::new(
        ErrorCode::Unsupported,
        format!("a node has no {} form", format.as_str()),
    )
    .with_hint("use JSON, YAML or text")
    .unsupported(format.as_str(), "CSV holds decision tables")
}

/// The logic that evaluates one node: only `"boolean"` for now
/// (`spec/ast.md`, *Evaluating one node*).
fn logic(name: Option<&str>) -> cleanrules::Result<Kleene> {
    let name = name.unwrap_or("boolean");
    match LogicName::from_name(name)? {
        LogicName::Boolean => Ok(Kleene),
        #[allow(unreachable_patterns)] // Graded logics arrive in phase J.
        _ => Err(Error::new(
            ErrorCode::Unsupported,
            format!("the logic {name:?} cannot evaluate one node yet"),
        )
        .unsupported(
            "evaluating one node",
            "only boolean logic evaluates one node",
        )),
    }
}

/// How a node is evaluated: as a condition under a logic, or as a value.
pub(crate) enum TreeKind {
    Condition,
    Value,
}

impl NodeCore {
    pub(crate) fn read(text: &[u8], options: Option<&RawOptions>) -> Outcome<Self> {
        let (format, limits) = read_options(options)?;
        let node = match format {
            DocumentFormat::Text => {
                utf8_text(text).and_then(|text| cleanrules::text::parse_node(text, &limits))
            }
            DocumentFormat::Csv => Err(not_csv(format)),
            other => parse_as(text, other, &limits).and_then(|json| read_node(&json, "", &limits)),
        }?;
        Ok(Self { node })
    }

    pub(crate) fn write(&self, format: &Raw) -> Outcome<String> {
        Ok(match super::format(format, DocumentFormat::Json)? {
            DocumentFormat::Json => to_string(&write_node(&self.node)),
            DocumentFormat::Yaml => cleanrules::yaml::to_string(&write_node(&self.node)),
            DocumentFormat::Text => self.node.to_string(),
            other => return Err(Problem::Core(not_csv(other))),
        })
    }

    pub(crate) fn field_paths(&self) -> Vec<Vec<Segment>> {
        self.node
            .field_paths()
            .into_iter()
            .map(<[Segment]>::to_vec)
            .collect()
    }

    /// Check the options before reading anything: an unknown logic fails first.
    pub(crate) fn check(kind: &TreeKind, options: &TreeOptions) -> Outcome<()> {
        if let TreeKind::Condition = kind {
            logic(options.logic.as_deref())?;
        }
        Ok(())
    }

    /// The result tree in JSON (`spec/json-format.md`, *Result trees*).
    pub(crate) fn tree(
        &self,
        kind: &TreeKind,
        input: &Value,
        options: &TreeOptions,
    ) -> cleanrules::Result<String> {
        let evaluation = Options {
            strict: options.strict,
            rule: None,
        };
        let tree = match kind {
            TreeKind::Condition => evaluate_tree(
                &self.node,
                input,
                &logic(options.logic.as_deref())?,
                evaluation,
            )?,
            TreeKind::Value => evaluate_value_tree(&self.node, input, &Kleene, evaluation)?,
        };
        Ok(to_string(&write_result(&tree)))
    }

    /// The result tree on an input given as a JSON value.
    pub(crate) fn tree_json(
        &self,
        kind: &TreeKind,
        text: &[u8],
        options: &TreeOptions,
    ) -> Outcome<String> {
        Self::check(kind, options)?;
        let input = parse_bytes(text, &options.limits)
            .and_then(|json| read_value(&json, "", ValueContext::Data))?;
        Ok(self.tree(kind, &input, options)?)
    }
}
