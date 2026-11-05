//! The native classes of the napi-rs build: documents, rule sets, decisions
//! and nodes, over the common code. Their TypeScript declarations
//! (`binding.d.ts`) are the package's for both builds (`native.d.ts`).

use cleanrules::Segment;
use napi::bindgen_prelude::{
    Array, ClassInstance, FnArgs, FromNapiValue, JavaScriptClassExt, JsValue, This, Unknown,
};
use napi::{Env, Result, ValueType};
use napi_derive::napi;

use super::hooks::{hook, throw};
use super::{OrFail, fail, raw, raw_limits, raw_list, raw_options};
use crate::common::core::{
    ActionFailure, DecisionCore, DocumentCore, NodeCore, PathSegment, RulesCore, TreeKind,
    action_error, cause_index, not_a_batch, path_segments, undefined_row,
};
use crate::common::wire::{Read, decode};
use crate::common::{Problem, Raw, UNDEFINED_INPUT, limits, strings, text, tree_options};

// --- Types for the declarations -------------------------------------------
//
// The values are read from JavaScript by the common code (`common::args`);
// these items only declare their TypeScript types.

/// Limits for reading documents and inputs; each one left out keeps the core's
/// default. `maxSize` sets the largest text of every format (JSON, YAML, the
/// rule syntax and CSV); `maxYamlSize` and `maxTextSize` then lower or raise
/// YAML's and the rule syntax's.
#[napi(object, object_from_js = false, object_to_js = false)]
pub struct Limits {
    /// The largest document, in bytes of UTF-8.
    pub max_size: Option<f64>,
    /// The deepest nesting of arrays and objects.
    pub max_nesting: Option<f64>,
    /// The deepest condition or payload value, in nodes.
    pub max_depth: Option<f64>,
    /// The most cells and nodes a decision table may build.
    pub max_table_size: Option<f64>,
    /// The largest YAML text, in bytes.
    pub max_yaml_size: Option<f64>,
    /// The largest text in the rule syntax or CSV, in bytes.
    pub max_text_size: Option<f64>,
}

/// How a text is read: its format (JSON when left out) and limits.
#[napi(object, object_from_js = false, object_to_js = false)]
pub struct ReadOptions {
    /// The format: `"json"` (the default), `"yaml"`, `"text"` or `"csv"`.
    #[napi(ts_type = "Format")]
    pub format: Option<String>,
    /// The limits; each one left out keeps the core's default.
    pub limits: Option<Limits>,
}

/// How one condition node is evaluated (`spec/ast.md`, *Evaluating one
/// node*).
#[napi(object, object_from_js = false, object_to_js = false)]
pub struct TreeOptions {
    /// The logic: only `"boolean"`, the default, for now.
    pub logic: Option<String>,
    /// Whether evaluation is strict; `false` by default.
    pub strict: Option<bool>,
    /// The limits for an input given as JSON; ignored for native data.
    pub limits: Option<Limits>,
}

/// How one value node is evaluated.
#[napi(object, object_from_js = false, object_to_js = false)]
pub struct ValueTreeOptions {
    /// Whether evaluation is strict; `false` by default.
    pub strict: Option<bool>,
    /// The limits for an input given as JSON; ignored for native data.
    pub limits: Option<Limits>,
}

/// The name of a format.
#[napi(string_enum)]
pub enum Format {
    /// JSON.
    #[napi(value = "json")]
    Json,
    /// YAML.
    #[napi(value = "yaml")]
    Yaml,
    /// The rule syntax.
    #[napi(value = "text")]
    Text,
    /// CSV, for decision tables.
    #[napi(value = "csv")]
    Csv,
}

/// The name of a hit policy (`spec/rules.md`).
#[napi(string_enum)]
pub enum HitPolicy {
    /// `first`.
    #[napi(value = "first")]
    First,
    /// `priority`.
    #[napi(value = "priority")]
    Priority,
    /// `unique`.
    #[napi(value = "unique")]
    Unique,
    /// `any`.
    #[napi(value = "any")]
    Any,
    /// `rule_order`.
    #[napi(value = "rule_order")]
    RuleOrder,
    /// `collect`.
    #[napi(value = "collect")]
    Collect,
}

/// The name of an aggregate of `collect`.
#[napi(string_enum)]
pub enum Aggregate {
    /// `sum`.
    #[napi(value = "sum")]
    Sum,
    /// `min`.
    #[napi(value = "min")]
    Min,
    /// `max`.
    #[napi(value = "max")]
    Max,
    /// `count`.
    #[napi(value = "count")]
    Count,
}

/// What a document holds.
#[napi(string_enum)]
pub enum DocumentKind {
    /// A condition.
    #[napi(value = "condition")]
    Condition,
    /// A rule.
    #[napi(value = "rule")]
    Rule,
    /// A rule set.
    #[napi(value = "ruleset")]
    Ruleset,
    /// A decision table.
    #[napi(value = "decision_table")]
    DecisionTable,
}

/// A crisp truth value.
#[napi(string_enum)]
pub enum Truth {
    /// `TRUE`.
    #[napi(value = "true")]
    True,
    /// `FALSE`.
    #[napi(value = "false")]
    False,
    /// `UNKNOWN`.
    #[napi(value = "unknown")]
    Unknown,
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

// --- Helpers --------------------------------------------------------------

/// A JavaScript value as `unknown`, to hand to a hook.
fn unknown<'env>(value: &impl JsValue<'env>) -> Unknown<'env> {
    value.to_unknown()
}

/// The field paths `paths` as JavaScript arrays of strings and numbers.
fn paths_to_js<'env>(env: &'env Env, paths: &[Vec<Segment>]) -> Result<Array<'env>> {
    let mut array = env.create_array(0)?;
    for path in paths {
        let mut items = env.create_array(0)?;
        for segment in path_segments(path) {
            match segment {
                PathSegment::Key(key) => items.insert(key)?,
                PathSegment::Index(index) => items.insert(index)?,
            }
        }
        array.insert(items)?;
    }
    Ok(array)
}

/// One input read by the `read` hook, with the values its getters threw.
struct Native<'env> {
    read: Read,
    thrown: Option<Array<'env>>,
}

/// Read `data` along the paths of `owner` (`lib/read.js`).
fn read<'env>(env: &'env Env, data: Unknown<'env>, owner: Unknown<'env>) -> Result<Native<'env>> {
    let read = hook::<FnArgs<(Unknown<'env>, Unknown<'env>)>, Array<'env>>(env, "read")?;
    let result = read.call(FnArgs::from((data, owner)))?;
    let wire: String = result
        .get(0)?
        .ok_or_else(|| napi::Error::from_reason("the reader returned no encoding"))?;
    let thrown: Option<Array<'env>> = result.get(1)?;
    let read = decode(&wire).map_err(|problem| {
        fail(
            env,
            Problem::Internal(format!("the reader's encoding is wrong ({problem})")),
        )
    })?;
    Ok(Native { read, thrown })
}

/// The JavaScript error of a core error met evaluating `native`, thrown; a
/// `read_error` in strict mode gets what its getter threw as `cause`.
fn chained(env: &Env, error: &cleanrules::Error, native: &Native<'_>) -> napi::Error {
    let cause = cause_index(error, &native.read.errors)
        .and_then(|index| native.thrown.as_ref()?.get::<Unknown<'_>>(index).ok()?)
        .filter(|value| value.get_type().is_ok_and(|t| t != ValueType::Undefined));
    throw(env, error, cause, None)
}

fn is_undefined(value: &Unknown<'_>) -> Result<bool> {
    Ok(value.get_type()? == ValueType::Undefined)
}

// --- Document -------------------------------------------------------------

/// A document read from a text; immutable.
#[napi]
pub struct Document {
    core: DocumentCore,
}

#[napi]
impl Document {
    /// Not a constructor: use `Document.read`.
    ///
    /// # Errors
    ///
    /// Always a `TypeError`, the same as the WebAssembly build's.
    #[napi(constructor)]
    pub fn new(env: &Env) -> Result<Self> {
        Err(fail(env, crate::common::no_constructor("Document")))
    }

    /// Read a document in `options.format` (`"json"`, the default, `"yaml"`,
    /// `"text"` or `"csv"`) under `options.limits`.
    ///
    /// # Errors
    ///
    /// `FormatError`, `RuleSyntaxError`, `ExpressionError`,
    /// `RuleDefinitionError`, `RegistryError` or `UnsupportedError`, as the core
    /// reports them; a `TypeError` or `RangeError` for a bad argument.
    #[napi(
        factory,
        ts_args_type = "text: string | Uint8Array, options?: ReadOptions | null"
    )]
    pub fn read(env: &Env, text: Unknown<'_>, options: Option<Unknown<'_>>) -> Result<Self> {
        let text = crate::common::text(raw(&text)?, "the text").or_fail(env)?;
        let options = optional_options(env, options)?;
        let core = DocumentCore::read(&text, options.as_ref()).or_fail(env)?;
        Ok(Self { core })
    }

    /// What the document holds: `"condition"`, `"rule"`, `"ruleset"` or
    /// `"decision_table"`.
    #[napi(getter, ts_return_type = "DocumentKind")]
    #[must_use]
    pub fn kind(&self) -> &'static str {
        self.core.kind()
    }

    /// The document as text in `format`: JSON (the default) or YAML for every
    /// document, the rule syntax for conditions, rules and rule sets, CSV for
    /// decision tables.
    ///
    /// # Errors
    ///
    /// `UnsupportedError` for a format the document has no form in.
    #[napi(ts_args_type = "format?: Format | null")]
    pub fn write(&self, env: &Env, format: Option<Unknown<'_>>) -> Result<String> {
        self.core.write(&optional(format)?).or_fail(env)
    }
}

/// An optional argument as a [`Raw`] value.
fn optional(value: Option<Unknown<'_>>) -> Result<Raw> {
    value.map_or(Ok(Raw::Undefined), |value| raw(&value))
}

/// An optional options argument.
fn optional_options(
    env: &Env,
    value: Option<Unknown<'_>>,
) -> Result<Option<crate::common::RawOptions>> {
    value.map_or(Ok(None), |value| raw_options(env, &value))
}

// --- RuleSet --------------------------------------------------------------

/// A rule set; immutable, and shared by its decisions.
#[napi]
pub struct RuleSet {
    pub(crate) core: RulesCore,
}

impl RuleSet {
    /// Evaluate native data, read along the rule set's paths.
    fn evaluate_native<'env>(
        &self,
        env: &'env Env,
        this: Unknown<'env>,
        data: Unknown<'env>,
    ) -> Result<Decision> {
        let native = read(env, data, this)?;
        let core = self
            .core
            .evaluate(native.read.value.clone())
            .map_err(|error| chained(env, &error, &native))?;
        Ok(Decision { core })
    }
}

#[napi]
impl RuleSet {
    /// Not a constructor: use `RuleSet.read`.
    ///
    /// # Errors
    ///
    /// Always a `TypeError`, the same as the WebAssembly build's.
    #[napi(constructor)]
    pub fn new(env: &Env) -> Result<Self> {
        Err(fail(env, crate::common::no_constructor("RuleSet")))
    }

    /// Read a rule set in `options.format` (`"json"`, the default, `"yaml"`,
    /// `"text"` or `"csv"`, whose decision table is compiled) under
    /// `options.limits`. A document that holds one rule is a rule set of that
    /// rule (`spec/json-format.md`, *Documents*).
    ///
    /// # Errors
    ///
    /// As `Document.read`, and `FormatError` for a condition document.
    #[napi(
        factory,
        ts_args_type = "text: string | Uint8Array, options?: ReadOptions | null"
    )]
    pub fn read(env: &Env, text: Unknown<'_>, options: Option<Unknown<'_>>) -> Result<Self> {
        let text = crate::common::text(raw(&text)?, "the text").or_fail(env)?;
        let options = optional_options(env, options)?;
        let core = RulesCore::read(&text, options.as_ref()).or_fail(env)?;
        Ok(Self { core })
    }

    /// The rule set of a document: its rule set, its one rule, or its compiled
    /// decision table.
    ///
    /// # Errors
    ///
    /// `FormatError` for a condition document.
    #[napi(factory, ts_args_type = "document: Document")]
    pub fn from_document(env: &Env, document: Unknown<'_>) -> Result<Self> {
        let Ok(document) = ClassInstance::<Document>::from_unknown(document) else {
            return Err(fail(env, crate::common::not_a_document(&raw(&document)?)));
        };
        let core = RulesCore::from_document(&document.core).or_fail(env)?;
        Ok(Self { core })
    }

    /// The rule set document in `format`: JSON (the default), YAML or the rule
    /// syntax.
    ///
    /// # Errors
    ///
    /// `UnsupportedError` for CSV.
    #[napi(ts_args_type = "format?: Format | null")]
    pub fn write(&self, env: &Env, format: Option<Unknown<'_>>) -> Result<String> {
        self.core.write(&optional(format)?).or_fail(env)
    }

    /// The summary (`spec/explain.md`, *Summaries*) in `format`: its text (the
    /// default, as in C++), or a `"json"` or `"yaml"` document.
    ///
    /// # Errors
    ///
    /// `UnsupportedError` for CSV.
    #[napi(ts_args_type = "format?: Format | null")]
    pub fn summary(&self, env: &Env, format: Option<Unknown<'_>>) -> Result<String> {
        self.core.summary(&optional(format)?).or_fail(env)
    }

    /// How many rules the rule set has.
    #[napi(getter)]
    #[must_use]
    #[allow(clippy::cast_possible_truncation)] // A rule set never holds 2^32 rules.
    pub fn length(&self) -> u32 {
        self.core.len() as u32
    }

    /// The rules' names, in rule order.
    #[napi]
    #[must_use]
    pub fn rule_names(&self) -> Vec<String> {
        self.core.rule_names()
    }

    /// The hit policy's name (`"first"`, `"priority"`, `"unique"`, `"any"`,
    /// `"rule_order"` or `"collect"`).
    #[napi(getter, ts_return_type = "HitPolicy")]
    #[must_use]
    pub fn hit_policy(&self) -> &'static str {
        self.core.hit_policy()
    }

    /// The aggregate's name (`"sum"`, `"min"`, `"max"` or `"count"`), or `null`.
    #[napi(getter, ts_return_type = "Aggregate | null")]
    #[must_use]
    pub fn aggregate(&self) -> Option<&'static str> {
        self.core.aggregate()
    }

    /// The logic's name, such as `"boolean"`.
    #[napi(getter)]
    #[must_use]
    pub fn logic(&self) -> &'static str {
        self.core.logic()
    }

    /// Whether evaluation is strict (`spec/rules.md`, *Strict mode and errors*).
    #[napi(getter)]
    #[must_use]
    pub fn strict(&self) -> bool {
        self.core.strict()
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
        paths_to_js(env, &self.core.field_paths())
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
        if is_undefined(&data)? {
            return Err(fail(env, Problem::Type(UNDEFINED_INPUT.to_owned())));
        }
        self.evaluate_native(env, unknown(&this), data)
    }

    /// Evaluate an input given as a JSON value (`spec/json-format.md`,
    /// *Values*) under `limits`.
    ///
    /// # Errors
    ///
    /// `FormatError` for an invalid text, and as `evaluate`.
    #[napi(ts_args_type = "input: string | Uint8Array, limits?: Limits | null")]
    pub fn evaluate_json(
        &self,
        env: &Env,
        input: Unknown<'_>,
        limits: Option<Unknown<'_>>,
    ) -> Result<Decision> {
        let input = text(raw(&input)?, "the input").or_fail(env)?;
        let limits = optional_limits(env, limits)?;
        let core = self.core.evaluate_json(&input, &limits).or_fail(env)?;
        Ok(Decision { core })
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
        rows: Unknown<'env>,
    ) -> Result<Vec<Decision>> {
        if !rows.is_array()? {
            return Err(fail(env, not_a_batch()));
        }
        let rows = Array::from_unknown(rows)?;
        let owner = unknown(&this);
        let mut natives = Vec::with_capacity(rows.len() as usize);
        for index in 0..rows.len() {
            let row: Unknown<'env> = rows
                .get(index)?
                .ok_or_else(|| napi::Error::from_reason(format!("row {index} cannot be read")))?;
            if is_undefined(&row)? {
                return Err(fail(env, undefined_row(index as usize)));
            }
            natives.push(read(env, row, owner)?);
        }
        let inputs = natives.iter().map(|n| n.read.value.clone()).collect();
        let decisions = self.core.evaluate_many(inputs).map_err(|error| {
            let row = error.context().row.and_then(|r| usize::try_from(r).ok());
            match row.and_then(|r| natives.get(r)) {
                Some(native) => chained(env, &error, native),
                None => throw(env, &error, None, None),
            }
        })?;
        Ok(decisions
            .into_iter()
            .map(|core| Decision { core })
            .collect())
    }

    /// Evaluate a batch given as a JSON array of values, under `limits`.
    ///
    /// # Errors
    ///
    /// `FormatError` for an invalid text, `EvaluationError` when it is not an
    /// array, and as `evaluateMany`.
    #[napi(ts_args_type = "inputs: string | Uint8Array, limits?: Limits | null")]
    pub fn evaluate_many_json(
        &self,
        env: &Env,
        inputs: Unknown<'_>,
        limits: Option<Unknown<'_>>,
    ) -> Result<Vec<Decision>> {
        let inputs = text(raw(&inputs)?, "the inputs").or_fail(env)?;
        let limits = optional_limits(env, limits)?;
        let decisions = self
            .core
            .evaluate_many_json(&inputs, &limits)
            .or_fail(env)?;
        Ok(decisions
            .into_iter()
            .map(|core| Decision { core })
            .collect())
    }

    /// Read a recorded decision in `options.format` (`"json"`, the default,
    /// or `"yaml"`) under `options.limits`, with this rule set
    /// (`spec/json-format.md`, *Reading a decision*).
    ///
    /// # Errors
    ///
    /// `FormatError`, with the code `decision_mismatch` for a decision of
    /// another rule set.
    #[napi(ts_args_type = "text: string | Uint8Array, options?: ReadOptions | null")]
    pub fn read_decision(
        &self,
        env: &Env,
        text: Unknown<'_>,
        options: Option<Unknown<'_>>,
    ) -> Result<Decision> {
        let text = crate::common::text(raw(&text)?, "the text").or_fail(env)?;
        let options = optional_options(env, options)?;
        let core = self
            .core
            .read_decision(&text, options.as_ref())
            .or_fail(env)?;
        Ok(Decision { core })
    }

    /// Check that every action the rules name is among `names`
    /// (`spec/rules.md`, *Actions*).
    ///
    /// # Errors
    ///
    /// `RegistryError` naming the first rule whose action is missing.
    #[napi(ts_args_type = "names: string[]")]
    pub fn check_actions(&self, env: &Env, names: Unknown<'_>) -> Result<()> {
        let names = strings(raw_list(env, &names, "names")?, "action names").or_fail(env)?;
        self.core.check_actions(&names).or_fail(env)
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
        self.core.check_actions(&names).or_fail(env)?;
        if is_undefined(&data)? {
            return Err(fail(env, Problem::Type(UNDEFINED_INPUT.to_owned())));
        }
        let decision = self.evaluate_native(env, unknown(&this), data)?;
        let steps = self.core.action_steps(&decision.core, &names);
        let decision = decision.into_instance(env)?;
        let call =
            hook::<FnArgs<(Unknown<'env>, ActionCall<'env>)>, Option<Array<'env>>>(env, "call")?;
        let mut executed: Vec<String> = Vec::new();
        for step in steps {
            let index = names.iter().position(|n| *n == step.action).unwrap_or(0);
            let function: Unknown<'env> = functions
                .get(u32::try_from(index).unwrap_or(u32::MAX))?
                .ok_or_else(|| napi::Error::from_reason("an action is missing"))?;
            #[allow(clippy::cast_possible_truncation)] // A rule set never holds 2^32 rules.
            let argument = ActionCall {
                rule_name: step.rule.clone(),
                position: step.position as u32,
                input: data,
                truth: step.truth.to_owned(),
                output_json: step.output.clone(),
                decision: unknown(&decision),
            };
            // `undefined`, or `[kind, thrown, description]`.
            let Some(failure) = call.call(FnArgs::from((function, argument)))? else {
                executed.push(step.rule);
                continue;
            };
            let kind: String = failure.get(0)?.unwrap_or_default();
            let cause: Option<Unknown<'env>> = failure.get(1)?;
            let detail: String = failure.get(2)?.unwrap_or_default();
            let failure = if kind == "promise" {
                ActionFailure::Promise
            } else {
                ActionFailure::Threw(detail)
            };
            let error = action_error(&step.rule, Some(&step.action), &failure, executed);
            let cause =
                cause.filter(|value| value.get_type().is_ok_and(|t| t != ValueType::Undefined));
            return Err(throw(env, &error, cause, Some(unknown(&decision))));
        }
        Ok(decision)
    }
}

/// An optional limits argument.
fn optional_limits(env: &Env, value: Option<Unknown<'_>>) -> Result<cleanrules::json::Limits> {
    let raw = match value {
        Some(value) => raw_limits(&value)?,
        None => crate::common::RawLimits::Absent,
    };
    limits(&raw).or_fail(env)
}

// --- Decision -------------------------------------------------------------

/// The result of evaluating a rule set on one input, or a recorded decision
/// read with its rule set; immutable.
#[napi]
pub struct Decision {
    pub(crate) core: DecisionCore,
}

#[napi]
impl Decision {
    /// Not a constructor: decisions come from `RuleSet.evaluate`.
    ///
    /// # Errors
    ///
    /// Always a `TypeError`, the same as the WebAssembly build's.
    #[napi(constructor)]
    pub fn new(env: &Env) -> Result<Self> {
        Err(fail(env, crate::common::no_constructor("Decision")))
    }

    /// How many rules the rule set has: one result each.
    #[napi(getter)]
    #[must_use]
    #[allow(clippy::cast_possible_truncation)] // A rule set never holds 2^32 rules.
    pub fn length(&self) -> u32 {
        self.core.len() as u32
    }

    /// The name of the rule at position `rule`.
    ///
    /// # Errors
    ///
    /// A `RangeError` for a position out of the rule set.
    #[napi(ts_args_type = "rule: number")]
    pub fn rule_name(&self, env: &Env, rule: Unknown<'_>) -> Result<String> {
        self.core.rule_name(&raw(&rule)?).or_fail(env)
    }

    /// Whether the rule at position `rule` fired.
    ///
    /// # Errors
    ///
    /// A `RangeError` for a position out of the rule set.
    #[napi(ts_args_type = "rule: number")]
    pub fn fired_at(&self, env: &Env, rule: Unknown<'_>) -> Result<bool> {
        self.core.fired_at(&raw(&rule)?).or_fail(env)
    }

    /// The truth of the rule's condition: `"true"`, `"false"` or `"unknown"`.
    /// Under a graded logic (phase J) it is the crisp truth of its degree.
    ///
    /// # Errors
    ///
    /// A `RangeError` for a position out of the rule set.
    #[napi(ts_args_type = "rule: number", ts_return_type = "Truth")]
    pub fn truth(&self, env: &Env, rule: Unknown<'_>) -> Result<&'static str> {
        self.core.truth(&raw(&rule)?).or_fail(env)
    }

    /// The positions of the selected rules, in rule order.
    #[napi]
    #[must_use]
    #[allow(clippy::cast_possible_truncation)] // A rule set never holds 2^32 rules.
    pub fn selected(&self) -> Vec<u32> {
        self.core.selected().iter().map(|&p| p as u32).collect()
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
        self.core.output_json().or_fail(env)
    }

    /// The outputs of the selected rules, in rule order, as a JSON array.
    #[napi]
    #[must_use]
    pub fn outputs_json(&self) -> String {
        self.core.outputs_json()
    }

    /// The evaluated payload of the rule at position `rule` as a JSON object,
    /// or `null` when it was not evaluated (`spec/rules.md`, *When payloads
    /// are evaluated*).
    ///
    /// # Errors
    ///
    /// A `RangeError` for a position out of the rule set.
    #[napi(ts_args_type = "rule: number")]
    pub fn rule_output_json(&self, env: &Env, rule: Unknown<'_>) -> Result<Option<String>> {
        self.core.rule_output_json(&raw(&rule)?).or_fail(env)
    }

    /// The decision document in `format`: JSON (the default) or YAML.
    ///
    /// # Errors
    ///
    /// `UnsupportedError` for the rule syntax and CSV.
    #[napi(ts_args_type = "format?: Format | null")]
    pub fn write(&self, env: &Env, format: Option<Unknown<'_>>) -> Result<String> {
        self.core.write(&optional(format)?).or_fail(env)
    }

    /// The explanation (`spec/explain.md`) in `format`: its text (the
    /// default, as in C++), or a `"json"` or `"yaml"` document.
    ///
    /// # Errors
    ///
    /// `UnsupportedError` for CSV.
    #[napi(ts_args_type = "format?: Format | null")]
    pub fn explain(&self, env: &Env, format: Option<Unknown<'_>>) -> Result<String> {
        self.core.explain(&optional(format)?).or_fail(env)
    }
}

// --- Node -----------------------------------------------------------------

/// A condition or value node; immutable.
#[napi]
pub struct Node {
    pub(crate) core: NodeCore,
}

impl Node {
    fn native<'env>(
        &self,
        env: &'env Env,
        this: &This<'env>,
        data: Unknown<'env>,
        kind: &TreeKind,
        options: Option<Unknown<'env>>,
    ) -> Result<String> {
        let options = optional_options(env, options)?;
        let options = tree_options(options.as_ref()).or_fail(env)?;
        NodeCore::check(kind, &options).or_fail(env)?;
        if is_undefined(&data)? {
            return Err(fail(env, Problem::Type(UNDEFINED_INPUT.to_owned())));
        }
        let native = read(env, data, unknown(this))?;
        self.core
            .tree(kind, &native.read.value, &options)
            .map_err(|error| chained(env, &error, &native))
    }

    fn json(
        &self,
        env: &Env,
        input: &Unknown<'_>,
        kind: &TreeKind,
        options: Option<Unknown<'_>>,
    ) -> Result<String> {
        let input = text(raw(input)?, "the input").or_fail(env)?;
        let options = optional_options(env, options)?;
        let options = tree_options(options.as_ref()).or_fail(env)?;
        self.core.tree_json(kind, &input, &options).or_fail(env)
    }
}

#[napi]
impl Node {
    /// Not a constructor: use `Node.read`.
    ///
    /// # Errors
    ///
    /// Always a `TypeError`, the same as the WebAssembly build's.
    #[napi(constructor)]
    pub fn new(env: &Env) -> Result<Self> {
        Err(fail(env, crate::common::no_constructor("Node")))
    }

    /// Read a node (`spec/json-format.md`, *Conditions*, not a document) in
    /// `options.format` (`"json"`, the default, `"yaml"` or `"text"`, an
    /// expression of any kind) under `options.limits`.
    ///
    /// # Errors
    ///
    /// `FormatError`, `RuleSyntaxError` or `ExpressionError`;
    /// `UnsupportedError` for CSV.
    #[napi(
        factory,
        ts_args_type = "text: string | Uint8Array, options?: ReadOptions | null"
    )]
    pub fn read(env: &Env, text: Unknown<'_>, options: Option<Unknown<'_>>) -> Result<Self> {
        let text = crate::common::text(raw(&text)?, "the text").or_fail(env)?;
        let options = optional_options(env, options)?;
        let core = NodeCore::read(&text, options.as_ref()).or_fail(env)?;
        Ok(Self { core })
    }

    /// The node in `format`: JSON (the default), YAML or the rule syntax.
    ///
    /// # Errors
    ///
    /// `UnsupportedError` for CSV.
    #[napi(ts_args_type = "format?: Format | null")]
    pub fn write(&self, env: &Env, format: Option<Unknown<'_>>) -> Result<String> {
        self.core.write(&optional(format)?).or_fail(env)
    }

    /// The field paths the node reads, in order of first appearance.
    ///
    /// # Errors
    ///
    /// If the arrays cannot be made.
    #[napi(ts_return_type = "Array<Array<string | number>>")]
    pub fn field_paths<'env>(&self, env: &'env Env) -> Result<Array<'env>> {
        paths_to_js(env, &self.core.field_paths())
    }

    /// Evaluate the node as a condition on native data: its result tree in
    /// JSON (`spec/json-format.md`, *Result trees*).
    ///
    /// # Errors
    ///
    /// `RegistryError` for an unknown logic, `UnsupportedError` for a graded
    /// one; in strict mode, `EvaluationError` and its subclasses.
    #[napi(ts_args_type = "data: unknown, options?: TreeOptions | null")]
    pub fn evaluate_tree<'env>(
        &self,
        env: &'env Env,
        this: This<'env>,
        data: Unknown<'env>,
        options: Option<Unknown<'env>>,
    ) -> Result<String> {
        self.native(env, &this, data, &TreeKind::Condition, options)
    }

    /// Evaluate the node as a value on native data: its result tree in JSON.
    ///
    /// # Errors
    ///
    /// In strict mode, `EvaluationError` and its subclasses.
    #[napi(ts_args_type = "data: unknown, options?: ValueTreeOptions | null")]
    pub fn evaluate_value_tree<'env>(
        &self,
        env: &'env Env,
        this: This<'env>,
        data: Unknown<'env>,
        options: Option<Unknown<'env>>,
    ) -> Result<String> {
        self.native(env, &this, data, &TreeKind::Value, options)
    }

    /// `evaluateTree` on an input given as a JSON value, under
    /// `options.limits`.
    ///
    /// # Errors
    ///
    /// `FormatError` for an invalid text, and as `evaluateTree`.
    #[napi(ts_args_type = "input: string | Uint8Array, options?: TreeOptions | null")]
    pub fn evaluate_tree_json(
        &self,
        env: &Env,
        input: Unknown<'_>,
        options: Option<Unknown<'_>>,
    ) -> Result<String> {
        self.json(env, &input, &TreeKind::Condition, options)
    }

    /// `evaluateValueTree` on an input given as a JSON value, under
    /// `options.limits`.
    ///
    /// # Errors
    ///
    /// `FormatError` for an invalid text, and as `evaluateValueTree`.
    #[napi(ts_args_type = "input: string | Uint8Array, options?: ValueTreeOptions | null")]
    pub fn evaluate_value_tree_json(
        &self,
        env: &Env,
        input: Unknown<'_>,
        options: Option<Unknown<'_>>,
    ) -> Result<String> {
        self.json(env, &input, &TreeKind::Value, options)
    }
}
