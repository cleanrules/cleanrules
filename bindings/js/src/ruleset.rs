//! Rule sets (`spec/rules.md`): read, written, summarised and evaluated on
//! JSON or native data, one input or a batch, with actions by name as
//! JavaScript callbacks.

use std::sync::Arc;

use cleanrules::explain::summary;
use cleanrules::format::{
    Document as CoreDocument, DocumentFormat, ValueContext, parse_as, read_document_as,
    read_parsed_decision_document, read_value, write_document_as, write_summary_as,
};
use cleanrules::json::{Json, parse_bytes};
use cleanrules::rules::RuleSet as CoreRuleSet;
use cleanrules::{Error, ErrorCode, Segment, Value};
use napi::bindgen_prelude::{
    Array, ClassInstance, Either, FnArgs, JavaScriptClassExt, JsValue, This, Uint8Array, Unknown,
};
use napi::{Env, Result, ValueType};
use napi_derive::napi;

use crate::decision::{Decision, Source};
use crate::document::Document;
use crate::hooks::{OrThrow, hook, throw, type_error};
use crate::limits::{self, Limits, ReadOptions, bytes};
use crate::read::{chained, read};

/// A rule set; immutable, and shared by its decisions.
#[napi]
pub struct RuleSet {
    ruleset: Arc<CoreRuleSet>,
}

impl RuleSet {
    fn new(ruleset: CoreRuleSet) -> Self {
        Self {
            ruleset: Arc::new(ruleset),
        }
    }

    fn decision(&self, env: &Env, input: Value) -> Result<Decision> {
        let decision = self.ruleset.evaluate(&input).or_throw(env)?;
        Ok(Decision::new(
            &self.ruleset,
            &decision,
            Source::Input(input),
        ))
    }

    /// Evaluate native data, read along the rule set's paths.
    fn evaluate_native<'env>(
        &self,
        env: &'env Env,
        this: Unknown<'env>,
        data: Unknown<'env>,
    ) -> Result<Decision> {
        let native = read(env, data, this)?;
        let decision = self
            .ruleset
            .evaluate(&native.value)
            .map_err(|error| chained(env, &error, &native))?;
        // The decision is made: the values thrown while reading are not needed.
        Ok(Decision::new(
            &self.ruleset,
            &decision,
            Source::Input(native.value),
        ))
    }
}

fn path_to_js<'env>(env: &'env Env, path: &[Segment]) -> Result<Array<'env>> {
    let mut array = env.create_array(0)?;
    for segment in path {
        match segment {
            Segment::Key(key) => array.insert(key.as_str())?,
            // Indices beyond 2^53 cannot be told apart in JavaScript anyway.
            #[allow(clippy::cast_precision_loss)]
            Segment::Index(index) => array.insert(*index as f64)?,
        }
    }
    Ok(array)
}

/// The field paths read by `paths`, as JavaScript arrays of strings and
/// numbers.
pub(crate) fn paths_to_js<'env, 'p>(
    env: &'env Env,
    paths: impl IntoIterator<Item = &'p [Segment]>,
) -> Result<Array<'env>> {
    let mut array = env.create_array(0)?;
    for path in paths {
        array.insert(path_to_js(env, path)?)?;
    }
    Ok(array)
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

/// A JavaScript value as `unknown`, to hand to a hook.
fn unknown<'env>(value: &impl JsValue<'env>) -> Unknown<'env> {
    value.to_unknown()
}

/// A batch error: inputs must come in an array (`spec/rules.md`, *Evaluation*).
fn not_a_batch() -> Error {
    Error::new(ErrorCode::EvaluationError, "a batch is a list of inputs")
        .with_hint("pass the inputs as the elements of an array")
}

#[napi]
impl RuleSet {
    /// Read a rule set in `options.format` (`"json"`, the default, `"yaml"`,
    /// `"text"` or `"csv"`, whose decision table is compiled) under
    /// `options.limits`. A document
    /// that holds one rule is a rule set of that rule (`spec/json-format.md`,
    /// *Documents*).
    ///
    /// # Errors
    ///
    /// As `Document.read`, and `FormatError` for a condition document.
    #[napi(factory)]
    pub fn read(
        env: &Env,
        text: Either<String, Uint8Array>,
        options: Option<ReadOptions>,
    ) -> Result<Self> {
        let (format, limits) = limits::read_options(env, options)?;
        let ruleset = read_document_as(bytes(&text), format, &limits)
            .and_then(CoreDocument::into_ruleset)
            .or_throw(env)?;
        Ok(Self::new(ruleset))
    }

    /// The rule set of a document: its rule set, its one rule, or its compiled
    /// decision table.
    ///
    /// # Errors
    ///
    /// `FormatError` for a condition document.
    #[napi(factory)]
    pub fn from_document(env: &Env, document: &Document) -> Result<Self> {
        let ruleset = document.document.clone().into_ruleset().or_throw(env)?;
        Ok(Self::new(ruleset))
    }

    /// The rule set document in `format`: JSON (the default), YAML or the rule
    /// syntax.
    ///
    /// # Errors
    ///
    /// `UnsupportedError` for CSV.
    #[napi(ts_args_type = "format?: Format")]
    pub fn write(&self, env: &Env, format: Option<String>) -> Result<String> {
        let format = limits::format(env, format.as_deref(), DocumentFormat::Json)?;
        let document = CoreDocument::RuleSet(CoreRuleSet::clone(&self.ruleset));
        write_document_as(&document, format).or_throw(env)
    }

    /// The summary (`spec/explain.md`, *Summaries*) in `format`: its text (the
    /// default, as in C++), or a `"json"` or `"yaml"` document.
    ///
    /// # Errors
    ///
    /// `UnsupportedError` for CSV.
    #[napi(ts_args_type = "format?: Format")]
    pub fn summary(&self, env: &Env, format: Option<String>) -> Result<String> {
        let format = limits::format(env, format.as_deref(), DocumentFormat::Text)?;
        write_summary_as(&summary(&self.ruleset), format).or_throw(env)
    }

    /// How many rules the rule set has.
    #[napi(getter)]
    #[must_use]
    #[allow(clippy::cast_possible_truncation)] // A rule set never holds 2^32 rules.
    pub fn length(&self) -> u32 {
        self.ruleset.rules().len() as u32
    }

    /// The rules' names, in rule order.
    #[napi]
    #[must_use]
    pub fn rule_names(&self) -> Vec<String> {
        self.ruleset
            .rules()
            .iter()
            .map(|rule| rule.name().to_owned())
            .collect()
    }

    /// The hit policy's name (`"first"`, `"priority"`, `"unique"`, `"any"`,
    /// `"rule_order"` or `"collect"`).
    #[napi(getter, ts_return_type = "HitPolicy")]
    #[must_use]
    pub fn hit_policy(&self) -> &'static str {
        self.ruleset.hit_policy().as_str()
    }

    /// The aggregate's name (`"sum"`, `"min"`, `"max"` or `"count"`), or `null`.
    #[napi(getter, ts_return_type = "Aggregate | null")]
    #[must_use]
    pub fn aggregate(&self) -> Option<&'static str> {
        self.ruleset
            .aggregate()
            .map(cleanrules::rules::Aggregate::as_str)
    }

    /// The logic's name, such as `"boolean"`.
    #[napi(getter)]
    #[must_use]
    pub fn logic(&self) -> &'static str {
        self.ruleset.logic().as_str()
    }

    /// Whether evaluation is strict (`spec/rules.md`, *Strict mode and errors*).
    #[napi(getter)]
    #[must_use]
    pub fn strict(&self) -> bool {
        self.ruleset.strict()
    }

    /// The field paths the rule set reads, in order of first appearance; each
    /// an array of strings (keys) and numbers (indices). Native data is read
    /// along these paths only.
    ///
    /// # Errors
    ///
    /// If the arrays cannot be made.
    #[napi(ts_return_type = "Array<Array<string | number>>")]
    pub fn field_paths<'env>(&self, env: &'env Env) -> Result<Array<'env>> {
        let summary = summary(&self.ruleset);
        paths_to_js(env, summary.fields.iter().map(|f| f.path.as_slice()))
    }

    /// Evaluate native JavaScript data (`spec/data-model.md`, *JavaScript and
    /// TypeScript*), read along the rule set's paths only.
    ///
    /// # Errors
    ///
    /// In strict mode, `EvaluationError` and its subclasses; a `read_error`
    /// has the value its getter threw as `cause`. A `TypeError` for an
    /// `undefined` input.
    #[napi(ts_args_type = "data: unknown")]
    pub fn evaluate<'env>(
        &self,
        env: &'env Env,
        this: This<'env>,
        data: Unknown<'env>,
    ) -> Result<Decision> {
        if data.get_type()? == ValueType::Undefined {
            return Err(type_error(
                env,
                "the input is undefined; pass the data to evaluate (null for none)",
            ));
        }
        self.evaluate_native(env, unknown(&this), data)
    }

    /// Evaluate an input given as a JSON value (`spec/json-format.md`,
    /// *Values*) under `limits`.
    ///
    /// # Errors
    ///
    /// `FormatError` for an invalid text, and as `evaluate`.
    #[napi]
    pub fn evaluate_json(
        &self,
        env: &Env,
        input: Either<String, Uint8Array>,
        limits: Option<Limits>,
    ) -> Result<Decision> {
        let limits = limits::of(env, limits)?;
        let input = parse_bytes(bytes(&input), &limits)
            .and_then(|json| read_value(&json, "", ValueContext::Data))
            .or_throw(env)?;
        self.decision(env, input)
    }

    /// Evaluate each input of an array of native data, in order.
    ///
    /// # Errors
    ///
    /// As `evaluate`, for the first input that fails, whose position is the
    /// error's `row`; `EvaluationError` when `rows` is not an array or an
    /// input is `undefined` (with its `row`).
    #[napi(ts_args_type = "rows: unknown[]")]
    pub fn evaluate_many<'env>(
        &self,
        env: &'env Env,
        this: This<'env>,
        rows: Either<Array<'env>, Unknown<'env>>,
    ) -> Result<Vec<Decision>> {
        let Either::A(rows) = rows else {
            return Err(throw(env, &not_a_batch(), None, None));
        };
        let owner = unknown(&this);
        let mut natives = Vec::with_capacity(rows.len() as usize);
        for index in 0..rows.len() {
            let row: Unknown<'env> = rows
                .get(index)?
                .ok_or_else(|| napi::Error::from_reason(format!("row {index} cannot be read")))?;
            if row.get_type()? == ValueType::Undefined {
                let mut error = Error::new(
                    ErrorCode::EvaluationError,
                    format!("input {index} of the batch is undefined"),
                )
                .with_hint("pass the data to evaluate (null for none)");
                error.context_mut().row = Some(u64::from(index));
                return Err(throw(env, &error, None, None));
            }
            natives.push(read(env, row, owner)?);
        }
        let inputs: Vec<Value> = natives.iter().map(|n| n.value.clone()).collect();
        let decisions = self.ruleset.evaluate_many(&inputs).map_err(|error| {
            let row = error.context().row.and_then(|r| usize::try_from(r).ok());
            match row.and_then(|r| natives.get(r)) {
                Some(native) => chained(env, &error, native),
                None => throw(env, &error, None, None),
            }
        })?;
        Ok(decisions
            .iter()
            .zip(inputs)
            .map(|(decision, input)| Decision::new(&self.ruleset, decision, Source::Input(input)))
            .collect())
    }

    /// Evaluate a batch given as a JSON array of values, under `limits`.
    ///
    /// # Errors
    ///
    /// `FormatError` for an invalid text, `EvaluationError` when it is not an
    /// array, and as `evaluateMany`.
    #[napi]
    pub fn evaluate_many_json(
        &self,
        env: &Env,
        inputs: Either<String, Uint8Array>,
        limits: Option<Limits>,
    ) -> Result<Vec<Decision>> {
        let limits = limits::of(env, limits)?;
        let json = parse_bytes(bytes(&inputs), &limits).or_throw(env)?;
        let Json::Array(items) = &json else {
            return Err(throw(env, &not_a_batch(), None, None));
        };
        let inputs = items
            .iter()
            .enumerate()
            .map(|(row, item)| read_value(item, &format!("/{row}"), ValueContext::Data))
            .collect::<cleanrules::Result<Vec<_>>>()
            .or_throw(env)?;
        let decisions = self.ruleset.evaluate_many(&inputs).or_throw(env)?;
        Ok(decisions
            .iter()
            .zip(inputs)
            .map(|(decision, input)| Decision::new(&self.ruleset, decision, Source::Input(input)))
            .collect())
    }

    /// Read a recorded decision in `options.format` (`"json"`, the default,
    /// or `"yaml"`) under `options.limits`, with this rule set (`spec/json-format.md`, *Reading a
    /// decision*).
    ///
    /// # Errors
    ///
    /// `FormatError`, with the code `decision_mismatch` for a decision of
    /// another rule set.
    #[napi]
    pub fn read_decision(
        &self,
        env: &Env,
        text: Either<String, Uint8Array>,
        options: Option<ReadOptions>,
    ) -> Result<Decision> {
        let (format, limits) = limits::read_options(env, options)?;
        let json = parse_as(bytes(&text), format, &limits).or_throw(env)?;
        let decision =
            read_parsed_decision_document(&json, &self.ruleset, &limits).or_throw(env)?;
        let decision = Decision::new(
            &self.ruleset,
            &decision,
            Source::Recorded(json.clone(), limits),
        );
        Ok(decision)
    }

    /// Check that every action the rules name is among `names`
    /// (`spec/rules.md`, *Actions*).
    ///
    /// # Errors
    ///
    /// `RegistryError` naming the first rule whose action is missing.
    #[napi]
    pub fn check_actions(&self, env: &Env, names: Vec<String>) -> Result<()> {
        self.ruleset.check_action_names(&names).or_throw(env)
    }

    /// Evaluate native data like `evaluate`, then run the actions of the
    /// selected rules in rule order (`spec/rules.md`, *Actions*). `actions`
    /// maps each action name to a function (an object or a `Map`), called
    /// with an `ActionCall`. Actions are synchronous: one that returns a
    /// promise (any thenable) fails.
    ///
    /// # Errors
    ///
    /// `RegistryError` before anything is read if a rule names an action that
    /// `actions` lacks; `ActionError` if an action throws (with the thrown
    /// value as `cause`) or returns a promise, with the rules whose actions
    /// had run as `executed` and the decision as `decision` (later actions do
    /// not run); a `TypeError` if an action is not a function; and as
    /// `evaluate`.
    #[napi(
        ts_args_type = "data: unknown, actions: Record<string, (call: ActionCall) => void> | Map<string, (call: ActionCall) => void>"
    )]
    pub fn run<'env>(
        &self,
        env: &'env Env,
        this: This<'env>,
        data: Unknown<'env>,
        actions: Unknown<'env>,
    ) -> Result<ClassInstance<'env, Decision>> {
        // `[names, functions]`, checked by the hook.
        let registry = hook::<FnArgs<(Unknown<'env>,)>, Array<'env>>(env, "actions")?
            .call(FnArgs::from((actions,)))?;
        let names: Array<'env> = registry
            .get(0)?
            .ok_or_else(|| napi::Error::from_reason("the actions hook returned no names"))?;
        let functions: Array<'env> = registry
            .get(1)?
            .ok_or_else(|| napi::Error::from_reason("the actions hook returned no functions"))?;
        let names: Vec<String> = (0..names.len())
            .map(|i| {
                names
                    .get::<String>(i)?
                    .ok_or_else(|| napi::Error::from_reason("no name"))
            })
            .collect::<Result<_>>()?;
        self.ruleset.check_action_names(&names).or_throw(env)?;
        let decision = self.evaluate_native(env, unknown(&this), data)?;
        let decision = decision.into_instance(env)?;
        let call =
            hook::<FnArgs<(Unknown<'env>, ActionCall<'env>)>, Option<Array<'env>>>(env, "call")?;
        let mut executed: Vec<String> = Vec::new();
        for &position in decision.selected_positions() {
            let rule = &decision.ruleset().rules()[position];
            let Some(name) = rule.get_action_name() else {
                continue;
            };
            // Checked above: every named action is registered.
            let Some(index) = names.iter().position(|n| n == name) else {
                continue;
            };
            let function: Unknown<'env> = functions
                .get(u32::try_from(index).unwrap_or(u32::MAX))?
                .ok_or_else(|| napi::Error::from_reason("an action is missing"))?;
            #[allow(clippy::cast_possible_truncation)] // A rule set never holds 2^32 rules.
            let argument = ActionCall {
                rule_name: rule.name().to_owned(),
                position: position as u32,
                input: data,
                truth: decision.truth_name(position).to_owned(),
                // A selected rule's payload is always evaluated.
                output_json: decision
                    .rule_output_text(position)
                    .unwrap_or_else(|| "{}".to_owned()),
                decision: unknown(&decision),
            };
            // `undefined`, or `[kind, thrown, description]` with kind `"threw"`
            // or `"promise"`.
            let Some(failure) = call.call(FnArgs::from((function, argument)))? else {
                executed.push(rule.name().to_owned());
                continue;
            };
            let kind: String = failure.get(0)?.unwrap_or_default();
            let cause: Option<Unknown<'env>> = failure.get(1)?;
            let detail: String = failure.get(2)?.unwrap_or_default();
            let message = if kind == "promise" {
                format!(
                    "the action {name:?} of rule {:?} returned a Promise; actions are synchronous",
                    rule.name()
                )
            } else {
                format!(
                    "the action {name:?} of rule {:?} threw {detail}",
                    rule.name()
                )
            };
            let mut error = Error::new(ErrorCode::ActionError, message)
                .with_hint("later actions did not run; error.decision is the decision")
                .caused_by(Thrown(detail));
            error.context_mut().rule = Some(rule.name().to_owned());
            error.context_mut().executed = executed;
            return Err(throw(env, &error, cause, Some(unknown(&decision))));
        }
        Ok(decision)
    }
}

/// The argument of an action (`spec/rules.md`, *Actions*): the rule, the
/// input as given, the truth of the rule's condition and its evaluated
/// payload, with the decision whose actions are running.
#[napi(object, object_from_js = false)]
pub struct ActionCall<'env> {
    /// The rule's name.
    pub rule_name: String,
    /// The rule's position in the rule set.
    pub position: u32,
    /// The input, as given to `run`.
    #[napi(ts_type = "unknown")]
    pub input: Unknown<'env>,
    /// The truth of the rule's condition.
    #[napi(ts_type = "Truth")]
    pub truth: String,
    /// The rule's evaluated payload as a JSON object (`{}` for a rule
    /// without payload).
    pub output_json: String,
    /// The decision whose actions are running: the one `run` returns.
    #[napi(ts_type = "Decision")]
    pub decision: Unknown<'env>,
}
