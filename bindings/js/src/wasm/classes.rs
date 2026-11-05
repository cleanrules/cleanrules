//! The classes of the WebAssembly build, with the names, arguments and
//! results of the napi-rs build (`src/napi/classes.rs`, whose declarations
//! are the package's types for both builds).

use std::cell::OnceCell;

use cleanrules::Segment;
use js_sys::Array;
use wasm_bindgen::prelude::*;

use super::{
    OrFail, call_action, call_actions, call_read, core_error, fail, nullable, object, raw,
    raw_limits, raw_list, raw_options,
};
use crate::common::core::{
    ActionFailure, DecisionCore, DocumentCore, NodeCore, PathSegment, RulesCore, TreeKind,
    action_error, cause_index, not_a_batch, path_segments, undefined_row,
};
use crate::common::wire::{Read, decode};
use crate::common::{Problem, RawLimits, UNDEFINED_INPUT, limits, strings, text, tree_options};

/// The field paths `paths` as JavaScript arrays of strings and numbers.
fn paths_to_js(paths: &[Vec<Segment>]) -> Array {
    paths
        .iter()
        .map(|path| {
            path_segments(path)
                .into_iter()
                .map(|segment| match segment {
                    PathSegment::Key(key) => JsValue::from_str(key),
                    PathSegment::Index(index) => JsValue::from_f64(index),
                })
                .collect::<Array>()
        })
        .collect()
}

/// One input read by the `read` hook, with the values its getters threw.
struct Native {
    read: Read,
    thrown: Array,
}

/// Read `data` along the paths `owner` (`lib/read.js`).
fn read(data: &JsValue, owner: &JsValue) -> Result<Native, JsValue> {
    let result = call_read(data, owner)?;
    let Some(wire) = result.get(0).as_string() else {
        return Err(fail(Problem::Internal(
            "the reader returned no encoding".to_owned(),
        )));
    };
    let read = decode(&wire).map_err(|problem| {
        fail(Problem::Internal(format!(
            "the reader's encoding is wrong ({problem})"
        )))
    })?;
    Ok(Native {
        read,
        thrown: Array::from(&result.get(1)),
    })
}

/// The JavaScript error of a core error met evaluating `native`; a
/// `read_error` in strict mode gets what its getter threw as `cause`.
fn chained(error: &cleanrules::Error, native: &Native) -> JsValue {
    let cause = cause_index(error, &native.read.errors)
        .map_or(JsValue::NULL, |index| native.thrown.get(index));
    let cause = if cause.is_undefined() {
        JsValue::NULL
    } else {
        cause
    };
    core_error(error, &cause, &JsValue::NULL)
}

fn undefined_input() -> JsValue {
    fail(Problem::Type(UNDEFINED_INPUT.to_owned()))
}

fn no_constructor(class: &str) -> JsValue {
    fail(crate::common::no_constructor(class))
}

fn optional_limits(value: &JsValue) -> Result<cleanrules::json::Limits, JsValue> {
    let raw = if value.is_undefined() {
        RawLimits::Absent
    } else {
        raw_limits(value)?
    };
    limits(&raw).or_fail()
}

// --- Document -------------------------------------------------------------

/// A document read from a text; immutable.
#[wasm_bindgen]
pub struct Document {
    core: DocumentCore,
}

#[wasm_bindgen]
impl Document {
    /// Not a constructor: use `Document.read`.
    ///
    /// # Errors
    ///
    /// Always a `TypeError`.
    #[wasm_bindgen(constructor)]
    pub fn new() -> Result<Document, JsValue> {
        Err(no_constructor("Document"))
    }

    /// Read a document.
    ///
    /// # Errors
    ///
    /// As in the napi-rs build.
    pub fn read(text: &JsValue, options: &JsValue) -> Result<Document, JsValue> {
        let text = crate::common::text(raw(text), "the text").or_fail()?;
        let options = raw_options(options)?;
        let core = DocumentCore::read(&text, options.as_ref()).or_fail()?;
        Ok(Self { core })
    }

    /// What the document holds.
    #[wasm_bindgen(getter)]
    #[must_use]
    pub fn kind(&self) -> String {
        self.core.kind().to_owned()
    }

    /// The document as text in `format`.
    ///
    /// # Errors
    ///
    /// As in the napi-rs build.
    pub fn write(&self, format: &JsValue) -> Result<String, JsValue> {
        self.core.write(&raw(format)).or_fail()
    }
}

// --- RuleSet --------------------------------------------------------------

/// A rule set; immutable, and shared by its decisions.
#[wasm_bindgen]
pub struct RuleSet {
    pub(crate) core: RulesCore,
    /// The field paths, made once: the reader keeps its path tree by them.
    paths: Array,
}

impl RuleSet {
    pub(crate) fn from_core(core: RulesCore) -> Self {
        let paths = paths_to_js(&core.field_paths());
        Self { core, paths }
    }

    fn evaluate_native(&self, data: &JsValue) -> Result<DecisionCore, JsValue> {
        let native = read(data, &self.paths)?;
        self.core
            .evaluate(native.read.value.clone())
            .map_err(|error| chained(&error, &native))
    }
}

#[wasm_bindgen]
impl RuleSet {
    /// Not a constructor: use `RuleSet.read`.
    ///
    /// # Errors
    ///
    /// Always a `TypeError`.
    #[wasm_bindgen(constructor)]
    pub fn new() -> Result<RuleSet, JsValue> {
        Err(no_constructor("RuleSet"))
    }

    /// Read a rule set.
    ///
    /// # Errors
    ///
    /// As in the napi-rs build.
    pub fn read(text: &JsValue, options: &JsValue) -> Result<RuleSet, JsValue> {
        let text = crate::common::text(raw(text), "the text").or_fail()?;
        let options = raw_options(options)?;
        Ok(Self::from_core(
            RulesCore::read(&text, options.as_ref()).or_fail()?,
        ))
    }

    /// The rule set of a document.
    ///
    /// # Errors
    ///
    /// As in the napi-rs build.
    #[wasm_bindgen(js_name = fromDocument)]
    pub fn from_document(document: &Document) -> Result<RuleSet, JsValue> {
        Ok(Self::from_core(
            RulesCore::from_document(&document.core).or_fail()?,
        ))
    }

    /// The rule set document in `format`.
    ///
    /// # Errors
    ///
    /// As in the napi-rs build.
    pub fn write(&self, format: &JsValue) -> Result<String, JsValue> {
        self.core.write(&raw(format)).or_fail()
    }

    /// The summary in `format`: its text by default.
    ///
    /// # Errors
    ///
    /// As in the napi-rs build.
    pub fn summary(&self, format: &JsValue) -> Result<String, JsValue> {
        self.core.summary(&raw(format)).or_fail()
    }

    /// How many rules the rule set has.
    #[wasm_bindgen(getter)]
    #[must_use]
    #[allow(clippy::cast_possible_truncation)] // A rule set never holds 2^32 rules.
    pub fn length(&self) -> u32 {
        self.core.len() as u32
    }

    /// The rules' names, in rule order.
    #[wasm_bindgen(js_name = ruleNames)]
    #[must_use]
    pub fn rule_names(&self) -> Vec<String> {
        self.core.rule_names()
    }

    /// The hit policy's name.
    #[wasm_bindgen(getter, js_name = hitPolicy)]
    #[must_use]
    pub fn hit_policy(&self) -> String {
        self.core.hit_policy().to_owned()
    }

    /// The aggregate's name, or `null`.
    #[wasm_bindgen(getter)]
    #[must_use]
    pub fn aggregate(&self) -> JsValue {
        nullable(self.core.aggregate().map(str::to_owned))
    }

    /// The logic's name.
    #[wasm_bindgen(getter)]
    #[must_use]
    pub fn logic(&self) -> String {
        self.core.logic().to_owned()
    }

    /// Whether evaluation is strict.
    #[wasm_bindgen(getter)]
    #[must_use]
    pub fn strict(&self) -> bool {
        self.core.strict()
    }

    /// The field paths the rule set reads, in order of first appearance.
    #[wasm_bindgen(js_name = fieldPaths)]
    #[must_use]
    pub fn field_paths(&self) -> Array {
        paths_to_js(&self.core.field_paths())
    }

    /// Evaluate native JavaScript data.
    ///
    /// # Errors
    ///
    /// As in the napi-rs build.
    pub fn evaluate(&self, data: &JsValue) -> Result<Decision, JsValue> {
        if data.is_undefined() {
            return Err(undefined_input());
        }
        Ok(Decision {
            core: self.evaluate_native(data)?,
        })
    }

    /// Evaluate an input given as a JSON value.
    ///
    /// # Errors
    ///
    /// As in the napi-rs build.
    #[wasm_bindgen(js_name = evaluateJson)]
    pub fn evaluate_json(&self, input: &JsValue, limits: &JsValue) -> Result<Decision, JsValue> {
        let input = text(raw(input), "the input").or_fail()?;
        let limits = optional_limits(limits)?;
        Ok(Decision {
            core: self.core.evaluate_json(&input, &limits).or_fail()?,
        })
    }

    /// Evaluate each input of an array of native data, in order.
    ///
    /// # Errors
    ///
    /// As in the napi-rs build.
    #[wasm_bindgen(js_name = evaluateMany)]
    pub fn evaluate_many(&self, rows: &JsValue) -> Result<Vec<Decision>, JsValue> {
        if !Array::is_array(rows) {
            return Err(fail(not_a_batch()));
        }
        let mut natives = Vec::new();
        for (index, row) in Array::from(rows).iter().enumerate() {
            if row.is_undefined() {
                return Err(fail(undefined_row(index)));
            }
            natives.push(read(&row, &self.paths)?);
        }
        let inputs = natives.iter().map(|n| n.read.value.clone()).collect();
        let decisions = self.core.evaluate_many(inputs).map_err(|error| {
            let row = error.context().row.and_then(|r| usize::try_from(r).ok());
            match row.and_then(|r| natives.get(r)) {
                Some(native) => chained(&error, native),
                None => core_error(&error, &JsValue::NULL, &JsValue::NULL),
            }
        })?;
        Ok(decisions
            .into_iter()
            .map(|core| Decision { core })
            .collect())
    }

    /// Evaluate a batch given as a JSON array of values.
    ///
    /// # Errors
    ///
    /// As in the napi-rs build.
    #[wasm_bindgen(js_name = evaluateManyJson)]
    pub fn evaluate_many_json(
        &self,
        inputs: &JsValue,
        limits: &JsValue,
    ) -> Result<Vec<Decision>, JsValue> {
        let inputs = text(raw(inputs), "the inputs").or_fail()?;
        let limits = optional_limits(limits)?;
        let decisions = self.core.evaluate_many_json(&inputs, &limits).or_fail()?;
        Ok(decisions
            .into_iter()
            .map(|core| Decision { core })
            .collect())
    }

    /// Read a recorded decision with this rule set.
    ///
    /// # Errors
    ///
    /// As in the napi-rs build.
    #[wasm_bindgen(js_name = readDecision)]
    pub fn read_decision(&self, text: &JsValue, options: &JsValue) -> Result<Decision, JsValue> {
        let text = crate::common::text(raw(text), "the text").or_fail()?;
        let options = raw_options(options)?;
        Ok(Decision {
            core: self.core.read_decision(&text, options.as_ref()).or_fail()?,
        })
    }

    /// Check that every action the rules name is among `names`.
    ///
    /// # Errors
    ///
    /// As in the napi-rs build.
    #[wasm_bindgen(js_name = checkActions)]
    pub fn check_actions(&self, names: &JsValue) -> Result<(), JsValue> {
        let names = strings(raw_list(names, "names")?, "action names").or_fail()?;
        self.core.check_actions(&names).or_fail()
    }

    /// Evaluate native data like `evaluate`, then run the actions of the
    /// selected rules in rule order; returns the decision the actions saw.
    ///
    /// # Errors
    ///
    /// As in the napi-rs build.
    pub fn run(&self, data: &JsValue, actions: &JsValue) -> Result<JsValue, JsValue> {
        // `[names, functions]`, checked by the hook.
        let registry = call_actions(actions)?;
        let functions = Array::from(&registry.get(1));
        let names = Array::from(&registry.get(0))
            .iter()
            .map(|name| name.as_string().unwrap_or_default())
            .collect::<Vec<_>>();
        self.core.check_actions(&names).or_fail()?;
        if data.is_undefined() {
            return Err(undefined_input());
        }
        let core = self.evaluate_native(data)?;
        let steps = self.core.action_steps(&core, &names);
        let decision: JsValue = Decision { core }.into();
        let mut executed: Vec<String> = Vec::new();
        for step in steps {
            let index = names.iter().position(|n| *n == step.action).unwrap_or(0);
            #[allow(clippy::cast_possible_truncation)] // Fewer actions than 2^32.
            let function = functions.get(index as u32);
            #[allow(clippy::cast_precision_loss)] // A rule set never holds 2^53 rules.
            let argument = object(&[
                ("ruleName", JsValue::from_str(&step.rule)),
                ("position", JsValue::from_f64(step.position as f64)),
                ("input", data.clone()),
                ("truth", JsValue::from_str(step.truth)),
                ("outputJson", JsValue::from_str(&step.output)),
                ("decision", decision.clone()),
            ])?;
            // `undefined`, or `[kind, thrown, description]`.
            let outcome = call_action(&function, &argument)?;
            if outcome.is_undefined() {
                executed.push(step.rule);
                continue;
            }
            let failure = Array::from(&outcome);
            let failure_kind = if failure.get(0).as_string().as_deref() == Some("promise") {
                ActionFailure::Promise
            } else {
                ActionFailure::Threw(failure.get(2).as_string().unwrap_or_default())
            };
            let cause = failure.get(1);
            let cause = if cause.is_undefined() {
                JsValue::NULL
            } else {
                cause
            };
            let error = action_error(&step.rule, Some(&step.action), &failure_kind, executed);
            return Err(core_error(&error, &cause, &decision));
        }
        Ok(decision)
    }
}

// --- Decision -------------------------------------------------------------

/// The result of evaluating a rule set on one input, or a recorded decision
/// read with its rule set; immutable.
#[wasm_bindgen]
pub struct Decision {
    pub(crate) core: DecisionCore,
}

#[wasm_bindgen]
impl Decision {
    /// Not a constructor: decisions come from rule sets.
    ///
    /// # Errors
    ///
    /// Always a `TypeError`.
    #[wasm_bindgen(constructor)]
    pub fn new() -> Result<Decision, JsValue> {
        Err(no_constructor("Decision"))
    }

    /// How many rules the rule set has: one result each.
    #[wasm_bindgen(getter)]
    #[must_use]
    #[allow(clippy::cast_possible_truncation)] // A rule set never holds 2^32 rules.
    pub fn length(&self) -> u32 {
        self.core.len() as u32
    }

    /// The name of the rule at position `rule`.
    ///
    /// # Errors
    ///
    /// As in the napi-rs build.
    #[wasm_bindgen(js_name = ruleName)]
    pub fn rule_name(&self, rule: &JsValue) -> Result<String, JsValue> {
        self.core.rule_name(&raw(rule)).or_fail()
    }

    /// Whether the rule at position `rule` fired.
    ///
    /// # Errors
    ///
    /// As in the napi-rs build.
    #[wasm_bindgen(js_name = firedAt)]
    pub fn fired_at(&self, rule: &JsValue) -> Result<bool, JsValue> {
        self.core.fired_at(&raw(rule)).or_fail()
    }

    /// The truth of the rule's condition.
    ///
    /// # Errors
    ///
    /// As in the napi-rs build.
    pub fn truth(&self, rule: &JsValue) -> Result<String, JsValue> {
        Ok(self.core.truth(&raw(rule)).or_fail()?.to_owned())
    }

    /// The positions of the selected rules, in rule order (an `Array`).
    #[must_use]
    pub fn selected(&self) -> Array {
        #[allow(clippy::cast_precision_loss)] // A rule set never holds 2^53 rules.
        self.core
            .selected()
            .iter()
            .map(|&p| JsValue::from_f64(p as f64))
            .collect()
    }

    /// The output of a single-hit decision as a JSON object, or `null`.
    ///
    /// # Errors
    ///
    /// As in the napi-rs build.
    #[wasm_bindgen(js_name = outputJson)]
    pub fn output_json(&self) -> Result<JsValue, JsValue> {
        Ok(nullable(self.core.output_json().or_fail()?))
    }

    /// The outputs of the selected rules, in rule order, as a JSON array.
    #[wasm_bindgen(js_name = outputsJson)]
    #[must_use]
    pub fn outputs_json(&self) -> String {
        self.core.outputs_json()
    }

    /// The evaluated payload of the rule at position `rule`, or `null`.
    ///
    /// # Errors
    ///
    /// As in the napi-rs build.
    #[wasm_bindgen(js_name = ruleOutputJson)]
    pub fn rule_output_json(&self, rule: &JsValue) -> Result<JsValue, JsValue> {
        Ok(nullable(self.core.rule_output_json(&raw(rule)).or_fail()?))
    }

    /// The decision document in `format`.
    ///
    /// # Errors
    ///
    /// As in the napi-rs build.
    pub fn write(&self, format: &JsValue) -> Result<String, JsValue> {
        self.core.write(&raw(format)).or_fail()
    }

    /// The explanation in `format`: its text by default.
    ///
    /// # Errors
    ///
    /// As in the napi-rs build.
    pub fn explain(&self, format: &JsValue) -> Result<String, JsValue> {
        self.core.explain(&raw(format)).or_fail()
    }
}

// --- Node -----------------------------------------------------------------

/// A condition or value node; immutable.
#[wasm_bindgen]
pub struct Node {
    pub(crate) core: NodeCore,
    /// The field paths, made once, when first read: the reader keeps its path
    /// tree by them. Nodes built in code often never read data.
    paths: OnceCell<Array>,
}

impl Node {
    /// A node of the core, as a JavaScript object.
    pub(crate) fn from_core(core: NodeCore) -> Self {
        Self {
            core,
            paths: OnceCell::new(),
        }
    }

    fn paths(&self) -> &Array {
        self.paths
            .get_or_init(|| paths_to_js(&self.core.field_paths()))
    }

    fn native(
        &self,
        data: &JsValue,
        kind: &TreeKind,
        options: &JsValue,
    ) -> Result<String, JsValue> {
        let options = raw_options(options)?;
        let options = tree_options(options.as_ref()).or_fail()?;
        NodeCore::check(kind, &options).or_fail()?;
        if data.is_undefined() {
            return Err(undefined_input());
        }
        let native = read(data, self.paths())?;
        self.core
            .tree(kind, &native.read.value, &options)
            .map_err(|error| chained(&error, &native))
    }

    fn json(&self, input: &JsValue, kind: &TreeKind, options: &JsValue) -> Result<String, JsValue> {
        let input = text(raw(input), "the input").or_fail()?;
        let options = raw_options(options)?;
        let options = tree_options(options.as_ref()).or_fail()?;
        self.core.tree_json(kind, &input, &options).or_fail()
    }
}

#[wasm_bindgen]
impl Node {
    /// Not a constructor: use `Node.read`.
    ///
    /// # Errors
    ///
    /// Always a `TypeError`.
    #[wasm_bindgen(constructor)]
    pub fn new() -> Result<Node, JsValue> {
        Err(no_constructor("Node"))
    }

    /// Read a node.
    ///
    /// # Errors
    ///
    /// As in the napi-rs build.
    pub fn read(text: &JsValue, options: &JsValue) -> Result<Node, JsValue> {
        let text = crate::common::text(raw(text), "the text").or_fail()?;
        let options = raw_options(options)?;
        let core = NodeCore::read(&text, options.as_ref()).or_fail()?;
        Ok(Self::from_core(core))
    }

    /// The node in `format`.
    ///
    /// # Errors
    ///
    /// As in the napi-rs build.
    pub fn write(&self, format: &JsValue) -> Result<String, JsValue> {
        self.core.write(&raw(format)).or_fail()
    }

    /// The field paths the node reads, in order of first appearance.
    #[wasm_bindgen(js_name = fieldPaths)]
    #[must_use]
    pub fn field_paths(&self) -> Array {
        paths_to_js(&self.core.field_paths())
    }

    /// Evaluate the node as a condition on native data.
    ///
    /// # Errors
    ///
    /// As in the napi-rs build.
    #[wasm_bindgen(js_name = evaluateTree)]
    pub fn evaluate_tree(&self, data: &JsValue, options: &JsValue) -> Result<String, JsValue> {
        self.native(data, &TreeKind::Condition, options)
    }

    /// Evaluate the node as a value on native data.
    ///
    /// # Errors
    ///
    /// As in the napi-rs build.
    #[wasm_bindgen(js_name = evaluateValueTree)]
    pub fn evaluate_value_tree(
        &self,
        data: &JsValue,
        options: &JsValue,
    ) -> Result<String, JsValue> {
        self.native(data, &TreeKind::Value, options)
    }

    /// `evaluateTree` on an input given as a JSON value.
    ///
    /// # Errors
    ///
    /// As in the napi-rs build.
    #[wasm_bindgen(js_name = evaluateTreeJson)]
    pub fn evaluate_tree_json(
        &self,
        input: &JsValue,
        options: &JsValue,
    ) -> Result<String, JsValue> {
        self.json(input, &TreeKind::Condition, options)
    }

    /// `evaluateValueTree` on an input given as a JSON value.
    ///
    /// # Errors
    ///
    /// As in the napi-rs build.
    #[wasm_bindgen(js_name = evaluateValueTreeJson)]
    pub fn evaluate_value_tree_json(
        &self,
        input: &JsValue,
        options: &JsValue,
    ) -> Result<String, JsValue> {
        self.json(input, &TreeKind::Value, options)
    }
}
