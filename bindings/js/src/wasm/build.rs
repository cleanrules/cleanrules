//! The wasm-bindgen side of the idiomatic API, with the names, arguments and
//! errors of the napi-rs build's (`src/napi/build.rs`): conditions, rules and
//! rule sets built through the core's constructors, and what decisions,
//! explanations and summaries give back. Internal: no entry point exports
//! them.

use js_sys::Array;
use wasm_bindgen::prelude::*;

use super::classes::{Decision, Node, RuleSet};
use super::{OrFail, fail, raw, raw_list};
use crate::common::build::{self, RuleArgs};
use crate::common::core::{ActionFailure, NodeCore, RulesCore, action_error_json};
use crate::common::strings;

fn node(core: cleanrules::ast::Node) -> Node {
    Node::from_core(NodeCore::new(core))
}

/// Nodes in order, each with an optional key: the operands of `and` and `or`,
/// or a rule's payload (wasm-bindgen cannot borrow objects inside an array).
#[wasm_bindgen]
pub struct NodeList {
    items: Vec<(String, cleanrules::ast::Node)>,
}

#[wasm_bindgen]
impl NodeList {
    /// An empty list.
    #[wasm_bindgen(constructor)]
    #[must_use]
    pub fn new() -> NodeList {
        Self { items: Vec::new() }
    }

    /// Add `node`, under `key` for a payload.
    pub fn push(&mut self, node: &Node, key: Option<String>) {
        self.items
            .push((key.unwrap_or_default(), node.core.node().clone()));
    }
}

impl Default for NodeList {
    fn default() -> Self {
        Self::new()
    }
}

/// Rules in order, for a rule set.
#[wasm_bindgen]
pub struct RuleList {
    items: Vec<cleanrules::rules::Rule>,
}

#[wasm_bindgen]
impl RuleList {
    /// An empty list.
    #[wasm_bindgen(constructor)]
    #[must_use]
    pub fn new() -> RuleList {
        Self { items: Vec::new() }
    }

    /// Add `rule`.
    pub fn push(&mut self, rule: &NativeRule) {
        self.items.push(rule.rule.clone());
    }
}

impl Default for RuleList {
    fn default() -> Self {
        Self::new()
    }
}

/// A rule of the core, built by `ruleNew`; immutable.
#[wasm_bindgen]
pub struct NativeRule {
    pub(crate) rule: cleanrules::rules::Rule,
}

#[wasm_bindgen]
impl NativeRule {
    /// Not a constructor: rules come from `ruleNew`.
    ///
    /// # Errors
    ///
    /// Always a `TypeError`.
    #[wasm_bindgen(constructor)]
    pub fn new() -> Result<NativeRule, JsValue> {
        Err(fail(crate::common::no_constructor("NativeRule")))
    }
}

/// A field, by its path of keys and indices.
///
/// # Errors
///
/// As in the napi-rs build.
#[wasm_bindgen(js_name = nodeField)]
pub fn node_field(path: &JsValue) -> Result<Node, JsValue> {
    let path = raw_list(path, "a field path")?;
    build::field(path).map(node).or_fail()
}

/// A constant, in the reader's encoding.
///
/// # Errors
///
/// As in the napi-rs build.
#[wasm_bindgen(js_name = nodeConstant)]
pub fn node_constant(wire: &str) -> Result<Node, JsValue> {
    build::constant(wire).map(node).or_fail()
}

/// `left op right`.
///
/// # Errors
///
/// As in the napi-rs build.
#[wasm_bindgen(js_name = nodeCompare)]
pub fn node_compare(op: &str, left: &Node, right: &Node) -> Result<Node, JsValue> {
    build::compare(op, left.core.node(), right.core.node())
        .map(node)
        .or_fail()
}

/// `and` or `or` of the nodes of `operands`.
///
/// # Errors
///
/// As in the napi-rs build.
#[wasm_bindgen(js_name = nodeConnective)]
pub fn node_connective(tag: &str, operands: &NodeList) -> Result<Node, JsValue> {
    let operands = operands.items.iter().map(|(_, n)| n.clone()).collect();
    build::connective(tag, operands).map(node).or_fail()
}

/// `not`, `is_null`, `abs` or `neg`.
///
/// # Errors
///
/// As in the napi-rs build.
#[wasm_bindgen(js_name = nodeUnary)]
pub fn node_unary(tag: &str, operand: &Node) -> Result<Node, JsValue> {
    build::unary(tag, operand.core.node()).map(node).or_fail()
}

/// Membership in constants, a list in the reader's encoding.
///
/// # Errors
///
/// As in the napi-rs build.
#[wasm_bindgen(js_name = nodeIsIn)]
pub fn node_is_in(operand: &Node, wire: &str) -> Result<Node, JsValue> {
    build::is_in(operand.core.node(), wire).map(node).or_fail()
}

/// A range test.
///
/// # Errors
///
/// As in the napi-rs build.
#[wasm_bindgen(js_name = nodeBetween)]
pub fn node_between(
    operand: &Node,
    low: &Node,
    high: &Node,
    closed: &str,
) -> Result<Node, JsValue> {
    build::between(
        operand.core.node(),
        low.core.node(),
        high.core.node(),
        closed,
    )
    .map(node)
    .or_fail()
}

/// A regular expression search.
///
/// # Errors
///
/// As in the napi-rs build.
#[wasm_bindgen(js_name = nodeMatches)]
pub fn node_matches(operand: &Node, pattern: &str) -> Result<Node, JsValue> {
    build::matches(operand.core.node(), pattern)
        .map(node)
        .or_fail()
}

/// `starts_with`, `ends_with` or `contains`.
///
/// # Errors
///
/// As in the napi-rs build.
#[wasm_bindgen(js_name = nodeStringTest)]
pub fn node_string_test(op: &str, operand: &Node, text: &str) -> Result<Node, JsValue> {
    build::string_test(op, operand.core.node(), text)
        .map(node)
        .or_fail()
}

/// Arithmetic.
///
/// # Errors
///
/// As in the napi-rs build.
#[wasm_bindgen(js_name = nodeArith)]
pub fn node_arith(op: &str, left: &Node, right: &Node) -> Result<Node, JsValue> {
    build::arith(op, left.core.node(), right.core.node())
        .map(node)
        .or_fail()
}

/// A node's tag, kind and own data, as JSON.
#[wasm_bindgen(js_name = nodeDetails)]
#[must_use]
pub fn node_details(node: &Node) -> String {
    build::details(node.core.node())
}

/// A node's children, in order.
#[wasm_bindgen(js_name = nodeChildren)]
#[must_use]
pub fn node_children(parent: &Node) -> Vec<Node> {
    parent
        .core
        .node()
        .children()
        .into_iter()
        .map(|child| node(child.clone()))
        .collect()
}

/// Whether two nodes are structurally identical (`spec/ast.md`).
#[wasm_bindgen(js_name = nodeSameAs)]
#[must_use]
pub fn node_same_as(left: &Node, right: &Node) -> bool {
    left.core.node() == right.core.node()
}

/// A rule.
///
/// # Errors
///
/// As in the napi-rs build.
#[wasm_bindgen(js_name = ruleNew)]
#[allow(clippy::too_many_arguments)] // A rule's parts, as `RuleArgs` holds them.
pub fn rule_new(
    name: String,
    when: &Node,
    then: &NodeList,
    priority: String,
    tags: &JsValue,
    description: Option<String>,
    metadata: Option<String>,
    action: Option<String>,
) -> Result<NativeRule, JsValue> {
    let tags = strings(raw_list(tags, "tags")?, "tags").or_fail()?;
    let args = RuleArgs {
        name,
        when: when.core.node().clone(),
        then: then.items.clone(),
        priority,
        tags,
        description,
        metadata,
        action,
    };
    build::rule(args).map(|rule| NativeRule { rule }).or_fail()
}

/// A rule's tags, metadata and action name, as JSON.
#[wasm_bindgen(js_name = ruleDetails)]
#[must_use]
pub fn rule_details(rule: &NativeRule) -> String {
    build::rule_details(&rule.rule)
}

/// A rule set of `rules`, with its options as given.
///
/// # Errors
///
/// As in the napi-rs build.
#[wasm_bindgen(js_name = ruleSetNew)]
pub fn rule_set_new(
    rules: &RuleList,
    hit_policy: &JsValue,
    aggregate: &JsValue,
    logic: &JsValue,
    strict: &JsValue,
) -> Result<RuleSet, JsValue> {
    let ruleset = build::ruleset(
        rules.items.clone(),
        &raw(hit_policy),
        &raw(aggregate),
        &raw(logic),
        &raw(strict),
    )
    .or_fail()?;
    Ok(RuleSet::from_core(RulesCore::new(ruleset)))
}

/// The summary with the rules whose action is code marked, in `format`.
///
/// # Errors
///
/// As in the napi-rs build.
#[wasm_bindgen(js_name = summaryWith)]
pub fn summary_with(
    ruleset: &RuleSet,
    actions: &JsValue,
    format: &JsValue,
) -> Result<String, JsValue> {
    let actions = Array::from(actions)
        .iter()
        .map(|flag| flag.as_bool().unwrap_or(false))
        .collect();
    ruleset.core.summary_with(actions, &raw(format)).or_fail()
}

/// What a decision gives back, in the binding's encoding.
#[wasm_bindgen(js_name = decisionData)]
#[must_use]
pub fn decision_data(decision: &Decision) -> String {
    decision.core.data()
}

/// A decision's explanation, in the binding's encoding.
///
/// # Errors
///
/// As in the napi-rs build.
#[wasm_bindgen(js_name = explanationData)]
pub fn explanation_data(decision: &Decision) -> Result<String, JsValue> {
    decision.core.explanation().or_fail()
}

/// The JSON form of the `ActionError` of an action given as a function (or
/// bound to `action`) that failed.
#[wasm_bindgen(js_name = actionErrorData)]
#[must_use]
pub fn action_error_data(
    rule: &str,
    action: Option<String>,
    kind: &str,
    detail: String,
    executed: &JsValue,
) -> String {
    let failure = if kind == "promise" {
        ActionFailure::Promise
    } else {
        ActionFailure::Threw(detail)
    };
    let executed = Array::from(executed)
        .iter()
        .filter_map(|name| name.as_string())
        .collect();
    action_error_json(rule, action.as_deref(), &failure, executed)
}
