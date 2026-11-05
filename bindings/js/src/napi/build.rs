//! The napi-rs side of the idiomatic API (`lib/api/`): conditions, rules and
//! rule sets built through the core's constructors (`common::build`), and
//! what decisions, explanations and summaries give back (`common::out`).
//!
//! These functions and classes are internal: the addon exports them, but
//! neither `cleanrules` nor `cleanrules/native` does. The WebAssembly build
//! has the same ones (`src/wasm/build.rs`), with the same checks and errors.

use napi::bindgen_prelude::Unknown;
use napi::{Env, Result};
use napi_derive::napi;

use super::classes::{Decision, Node, RuleSet};
use super::{OrFail, fail, raw, raw_list};
use crate::common::build::{self, RuleArgs};
use crate::common::core::{ActionFailure, NodeCore, RulesCore, action_error_json};
use crate::common::strings;

fn node(core: cleanrules::ast::Node) -> Node {
    Node {
        core: NodeCore::new(core),
    }
}

/// Nodes in order, each with an optional key: the operands of `and` and `or`,
/// or a rule's payload. wasm-bindgen cannot borrow objects inside an array, so
/// both builds pass them one by one.
#[napi]
pub struct NodeList {
    items: Vec<(String, cleanrules::ast::Node)>,
}

#[napi]
impl NodeList {
    /// An empty list.
    #[napi(constructor)]
    #[must_use]
    pub fn new() -> Self {
        Self { items: Vec::new() }
    }

    /// Add `node`, under `key` for a payload.
    #[napi]
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
#[napi]
pub struct RuleList {
    items: Vec<cleanrules::rules::Rule>,
}

#[napi]
impl RuleList {
    /// An empty list.
    #[napi(constructor)]
    #[must_use]
    pub fn new() -> Self {
        Self { items: Vec::new() }
    }

    /// Add `rule`.
    #[napi]
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
#[napi]
pub struct NativeRule {
    rule: cleanrules::rules::Rule,
}

#[napi]
impl NativeRule {
    /// Not a constructor: rules come from `ruleNew`.
    ///
    /// # Errors
    ///
    /// Always a `TypeError`, the same as the WebAssembly build's.
    #[napi(constructor)]
    pub fn new(env: &Env) -> Result<Self> {
        Err(fail(env, crate::common::no_constructor("NativeRule")))
    }
}

/// A field, by its path of keys and indices.
///
/// # Errors
///
/// `ExpressionError` for an invalid path.
#[napi(ts_args_type = "path: Array<string | number>")]
pub fn node_field(env: &Env, path: Unknown<'_>) -> Result<Node> {
    let path = raw_list(env, &path, "a field path")?;
    build::field(path).map(node).or_fail(env)
}

/// A constant, in the reader's encoding.
///
/// # Errors
///
/// `ExpressionError` for a value that is not a constant.
#[napi]
pub fn node_constant(env: &Env, wire: String) -> Result<Node> {
    build::constant(&wire).map(node).or_fail(env)
}

/// `left op right`.
///
/// # Errors
///
/// `ExpressionError`, as the core reports it.
#[napi]
pub fn node_compare(env: &Env, op: String, left: &Node, right: &Node) -> Result<Node> {
    build::compare(&op, left.core.node(), right.core.node())
        .map(node)
        .or_fail(env)
}

/// `and` or `or` of the nodes of `operands`.
///
/// # Errors
///
/// `ExpressionError`, as the core reports it.
#[napi]
pub fn node_connective(env: &Env, tag: String, operands: &NodeList) -> Result<Node> {
    let operands = operands.items.iter().map(|(_, n)| n.clone()).collect();
    build::connective(&tag, operands).map(node).or_fail(env)
}

/// `not`, `is_null`, `abs` or `neg`.
///
/// # Errors
///
/// `ExpressionError`, as the core reports it.
#[napi]
pub fn node_unary(env: &Env, tag: String, operand: &Node) -> Result<Node> {
    build::unary(&tag, operand.core.node())
        .map(node)
        .or_fail(env)
}

/// Membership in constants, a list in the reader's encoding.
///
/// # Errors
///
/// `ExpressionError`, as the core reports it.
#[napi]
pub fn node_is_in(env: &Env, operand: &Node, wire: String) -> Result<Node> {
    build::is_in(operand.core.node(), &wire)
        .map(node)
        .or_fail(env)
}

/// A range test.
///
/// # Errors
///
/// `ExpressionError`, as the core reports it.
#[napi]
pub fn node_between(
    env: &Env,
    operand: &Node,
    low: &Node,
    high: &Node,
    closed: String,
) -> Result<Node> {
    build::between(
        operand.core.node(),
        low.core.node(),
        high.core.node(),
        &closed,
    )
    .map(node)
    .or_fail(env)
}

/// A regular expression search.
///
/// # Errors
///
/// `ExpressionError` for a pattern that does not compile.
#[napi]
pub fn node_matches(env: &Env, operand: &Node, pattern: String) -> Result<Node> {
    build::matches(operand.core.node(), &pattern)
        .map(node)
        .or_fail(env)
}

/// `starts_with`, `ends_with` or `contains`.
///
/// # Errors
///
/// `ExpressionError`, as the core reports it.
#[napi]
pub fn node_string_test(env: &Env, op: String, operand: &Node, text: String) -> Result<Node> {
    build::string_test(&op, operand.core.node(), &text)
        .map(node)
        .or_fail(env)
}

/// Arithmetic.
///
/// # Errors
///
/// `ExpressionError`, as the core reports it.
#[napi]
pub fn node_arith(env: &Env, op: String, left: &Node, right: &Node) -> Result<Node> {
    build::arith(&op, left.core.node(), right.core.node())
        .map(node)
        .or_fail(env)
}

/// A node's tag, kind and own data, as JSON.
#[napi]
#[must_use]
pub fn node_details(node: &Node) -> String {
    build::details(node.core.node())
}

/// A node's children, in order.
#[napi]
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
#[napi]
#[must_use]
pub fn node_same_as(left: &Node, right: &Node) -> bool {
    left.core.node() == right.core.node()
}

/// A rule.
///
/// # Errors
///
/// `RuleDefinitionError` or `ExpressionError`, as the core reports them.
#[napi(
    ts_args_type = "name: string, when: Node, then: NodeList, priority: string, tags: string[], description?: string | null, metadata?: string | null, action?: string | null"
)]
#[allow(clippy::too_many_arguments)] // A rule's parts, as `RuleArgs` holds them.
pub fn rule_new(
    env: &Env,
    name: String,
    when: &Node,
    then: &NodeList,
    priority: String,
    tags: Unknown<'_>,
    description: Option<String>,
    metadata: Option<String>,
    action: Option<String>,
) -> Result<NativeRule> {
    let tags = strings(raw_list(env, &tags, "tags")?, "tags").or_fail(env)?;
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
    build::rule(args)
        .map(|rule| NativeRule { rule })
        .or_fail(env)
}

/// A rule's tags, metadata and action name, as JSON.
#[napi]
#[must_use]
pub fn rule_details(rule: &NativeRule) -> String {
    build::rule_details(&rule.rule)
}

/// A rule set of `rules`, with its options as given.
///
/// # Errors
///
/// `RuleDefinitionError` or `RegistryError`, as the core reports them.
#[napi]
pub fn rule_set_new(
    env: &Env,
    rules: &RuleList,
    hit_policy: Unknown<'_>,
    aggregate: Unknown<'_>,
    logic: Unknown<'_>,
    strict: Unknown<'_>,
) -> Result<RuleSet> {
    let ruleset = build::ruleset(
        rules.items.clone(),
        &raw(&hit_policy)?,
        &raw(&aggregate)?,
        &raw(&logic)?,
        &raw(&strict)?,
    )
    .or_fail(env)?;
    Ok(RuleSet {
        core: RulesCore::new(ruleset),
    })
}

/// The summary with the rules whose action is code marked, in `format`.
///
/// # Errors
///
/// `UnsupportedError` for CSV.
#[napi]
pub fn summary_with(
    env: &Env,
    ruleset: &RuleSet,
    actions: Vec<bool>,
    format: Unknown<'_>,
) -> Result<String> {
    ruleset
        .core
        .summary_with(actions, &raw(&format)?)
        .or_fail(env)
}

/// What a decision gives back, in the binding's encoding.
#[napi]
#[must_use]
pub fn decision_data(decision: &Decision) -> String {
    decision.core.data()
}

/// A decision's explanation, in the binding's encoding.
///
/// # Errors
///
/// `UnsupportedError` for a logic explanations do not support.
#[napi]
pub fn explanation_data(env: &Env, decision: &Decision) -> Result<String> {
    decision.core.explanation().or_fail(env)
}

/// The JSON form of the `ActionError` of an action given as a function (or
/// bound to `action`) that failed: it threw (`kind` `"threw"`, described by
/// `detail`) or returned a promise (`"promise"`).
#[napi]
#[must_use]
pub fn action_error_data(
    rule: String,
    action: Option<String>,
    kind: String,
    detail: String,
    executed: Vec<String>,
) -> String {
    let failure = if kind == "promise" {
        ActionFailure::Promise
    } else {
        ActionFailure::Threw(detail)
    };
    action_error_json(&rule, action.as_deref(), &failure, executed)
}
