//! Result trees (`spec/ast.md`, *Evaluating one node*; `spec/json-format.md`,
//! *Result trees*) as handles: the tree as a value, with the values themselves
//! (opaque ones keep their token), and the node of each result, for bindings
//! that give the tree as objects.

use std::ffi::c_char;

use cleanrules::Value;
use cleanrules::ast::Node;
use cleanrules::eval::{Evaluated, Options, evaluate_tree, evaluate_value_tree};
use cleanrules::format::result_value;
use cleanrules::logic::{Kleene, Truth};

use crate::boundary::{Failure, Outcome, call, clear, free, give, handle};
use crate::construct::build;
use crate::cr_status;
use crate::decision::cr_decision;
use crate::error::cr_error;
use crate::node::{cr_node, logic};
use crate::value::{borrow_value, cr_value, give_new_value};

/// A result tree: what evaluating a node gave, and each child's result.
pub struct cr_tree {
    /// The tree as a value, in the structure of the JSON result tree.
    value: Value,
    /// The node of each result, in pre-order.
    nodes: Vec<Node>,
}

/// Each result's node, in pre-order.
fn nodes(evaluated: &Evaluated<'_, Truth>, out: &mut Vec<Node>) {
    // In depth-first pre-order, with an explicit stack: a result tree is as
    // deep as its condition.
    let mut pending = vec![evaluated];
    while let Some(evaluated) = pending.pop() {
        out.push(evaluated.node.as_ref().clone());
        pending.extend(evaluated.children.iter().rev());
    }
}

impl cr_tree {
    fn of(evaluated: &Evaluated<'_, Truth>) -> Self {
        let mut found = Vec::new();
        nodes(evaluated, &mut found);
        Self {
            value: result_value(evaluated),
            nodes: found,
        }
    }
}

/// Evaluate a node and hand its tree out.
///
/// # Safety
///
/// `node` is `NULL` or a live `cr_node`; `input` is `NULL` or a live
/// `cr_value`; `out` is `NULL` or points to a writable `cr_tree *`; `error` is
/// `NULL` or points to a writable `cr_error *` set to `NULL`.
unsafe fn evaluate(
    name: &str,
    node: *const cr_node,
    input: *const cr_value,
    out: *mut *mut cr_tree,
    error: *mut *mut cr_error,
    tree: impl FnOnce(&Node, &Value) -> Outcome<cr_tree>,
) -> cr_status {
    let body = || {
        // SAFETY: `out` is NULL or writable (# Safety).
        unsafe { clear(out, "out") }?;
        // SAFETY: `node` is NULL or live (# Safety).
        let node = &unsafe { handle(node, "node") }?.node;
        // SAFETY: `input` is NULL or live (# Safety).
        let input = unsafe { borrow_value(input, "input") }?;
        let made = tree(node, input)?;
        // SAFETY: `out` passed `clear`.
        unsafe { give(out, made) };
        Ok(())
    };
    // SAFETY: `error` is NULL or writable (# Safety).
    unsafe { call(name, error, body) }
}

/// Evaluate a node as a condition on an input, as `cr_node_evaluate_tree`
/// does (a logic by name, `boolean` for `NULL`; strict or not; a value node is
/// a `CR_EXPRESSION_ERROR`), and give its result tree as a handle, freed with
/// `cr_tree_free`. The input stays the caller's.
///
/// # Safety
///
/// `node` is `NULL` or a live `cr_node`; `input` is `NULL` or a live
/// `cr_value`; `logic` is `NULL` or points to `logic_len` readable bytes; `out`
/// is `NULL` or points to a writable `cr_tree *`; `error` is `NULL` or points
/// to a writable `cr_error *` set to `NULL`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cr_tree_new_condition(
    node: *const cr_node,
    input: *const cr_value,
    logic: *const c_char,
    logic_len: usize,
    strict: bool,
    out: *mut *mut cr_tree,
    error: *mut *mut cr_error,
) -> cr_status {
    let tree = |node: &Node, input: &Value| {
        // SAFETY: the logic's name and its length (# Safety).
        let logic = unsafe { self::logic(logic, logic_len) }?;
        let options = Options { strict, rule: None };
        Ok(cr_tree::of(&evaluate_tree(node, input, &logic, options)?))
    };
    // SAFETY: the caller's promises are `evaluate`'s.
    unsafe { evaluate("cr_tree_new_condition", node, input, out, error, tree) }
}

/// Evaluate a node as a value on an input, as `cr_node_evaluate_value_tree`
/// does (a condition is a `CR_EXPRESSION_ERROR`), and give its result tree as
/// a handle, freed with `cr_tree_free`. The input stays the caller's.
///
/// # Safety
///
/// `node` is `NULL` or a live `cr_node`; `input` is `NULL` or a live
/// `cr_value`; `out` is `NULL` or points to a writable `cr_tree *`; `error` is
/// `NULL` or points to a writable `cr_error *` set to `NULL`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cr_tree_new_value(
    node: *const cr_node,
    input: *const cr_value,
    strict: bool,
    out: *mut *mut cr_tree,
    error: *mut *mut cr_error,
) -> cr_status {
    let tree = |node: &Node, input: &Value| {
        let options = Options { strict, rule: None };
        Ok(cr_tree::of(&evaluate_value_tree(
            node, input, &Kleene, options,
        )?))
    };
    // SAFETY: the caller's promises are `evaluate`'s.
    unsafe { evaluate("cr_tree_new_value", node, input, out, error, tree) }
}

/// The result tree of the condition of the rule at position `rule` in a
/// decision (as its explanation reads it), freed with `cr_tree_free`.
///
/// # Safety
///
/// `decision` is `NULL` or a live `cr_decision`; `out` is `NULL` or points to
/// a writable `cr_tree *`; `error` is `NULL` or points to a writable
/// `cr_error *` set to `NULL`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cr_decision_rule_tree(
    decision: *const cr_decision,
    rule: usize,
    out: *mut *mut cr_tree,
    error: *mut *mut cr_error,
) -> cr_status {
    let body = || {
        // SAFETY: `out` is NULL or writable (# Safety).
        unsafe { clear(out, "out") }?;
        // SAFETY: `decision` is NULL or live (# Safety).
        let decision = unsafe { handle(decision, "decision") }?;
        let tree = decision.rule_tree(rule, cr_tree::of)?;
        // SAFETY: `out` passed `clear`.
        unsafe { give(out, tree) };
        Ok(())
    };
    // SAFETY: `error` is NULL or writable (# Safety).
    unsafe { call("cr_decision_rule_tree", error, body) }
}

/// The tree as a new value, freed with `cr_value_free`, in the structure of
/// the JSON result tree (`spec/json-format.md`, *Result trees*): each result a
/// map with `op`, `path` (a field's), `truth` (a boolean, or null for unknown)
/// or `value` (the value itself: an opaque one keeps its token), `reason` (a
/// map with `code`, and `paths`, `types` and `detail` when it has them) and
/// `children`. `cr_value_to_json` of it is the tree's JSON.
///
/// # Safety
///
/// `tree` is `NULL` or a live `cr_tree`; `out` is `NULL` or points to a
/// writable `cr_value *`; `error` is `NULL` or points to a writable
/// `cr_error *` set to `NULL`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cr_tree_value(
    tree: *const cr_tree,
    out: *mut *mut cr_value,
    error: *mut *mut cr_error,
) -> cr_status {
    let body = || {
        // SAFETY: `tree` is NULL or live (# Safety).
        let value = unsafe { handle(tree, "tree") }?.value.clone();
        // SAFETY: `out` is NULL or writable (# Safety).
        unsafe { give_new_value(out, value) }
    };
    // SAFETY: `error` is NULL or writable (# Safety).
    unsafe { call("cr_tree_value", error, body) }
}

/// The number of results in the tree, its root and every descendant; 0 for
/// `NULL`.
///
/// # Safety
///
/// `tree` is `NULL` or a live `cr_tree`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cr_tree_len(tree: *const cr_tree) -> usize {
    // SAFETY: `tree` is NULL or live (# Safety).
    unsafe { tree.as_ref() }.map_or(0, |t| t.nodes.len())
}

/// A copy of the node of the result at `index`, counting the results in
/// pre-order (the root, then each child's subtree in order), freed with
/// `cr_node_free`. The halves of a `between` are comparisons the evaluation
/// made.
///
/// # Safety
///
/// `tree` is `NULL` or a live `cr_tree`; `out` is `NULL` or points to a
/// writable `cr_node *`; `error` is `NULL` or points to a writable
/// `cr_error *` set to `NULL`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cr_tree_node(
    tree: *const cr_tree,
    index: usize,
    out: *mut *mut cr_node,
    error: *mut *mut cr_error,
) -> cr_status {
    let make = || {
        // SAFETY: `tree` is NULL or live (# Safety).
        let nodes = &unsafe { handle(tree, "tree") }?.nodes;
        nodes.get(index).cloned().ok_or_else(|| {
            Failure::Invalid(
                format!("result {index} is out of the tree of {}", nodes.len()),
                "pass an index below cr_tree_len",
            )
        })
    };
    // SAFETY: `out` and `error` (# Safety).
    unsafe { build("cr_tree_node", out, error, make) }
}

/// Free a result tree. `NULL` is ignored.
///
/// # Safety
///
/// `tree` is `NULL` or a `cr_tree` from this library, not freed before.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cr_tree_free(tree: *mut cr_tree) {
    // SAFETY: the caller's promise is `free`'s.
    unsafe { free(tree) };
}
