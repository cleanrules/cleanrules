//! Single nodes (`spec/ast.md`): a condition or a value read on its own and
//! evaluated on one input, with its result tree (`spec/json-format.md`,
//! *Result trees*).

use std::ffi::c_char;

use cleanrules::ast::Node;
use cleanrules::eval::{Options, evaluate_tree, evaluate_value_tree};
use cleanrules::format::{DocumentFormat, parse_as, read_node, write_node, write_result};
use cleanrules::json::{to_string, utf8_text};
use cleanrules::logic::{Kleene, LogicName};
use cleanrules::{Error, ErrorCode};

use crate::boundary::{
    Outcome, bytes, call, clear, format, free, give, give_text, handle, limits_of, utf8_arg,
};
use crate::error::cr_error;
use crate::limits::cr_limits;
use crate::ruleset::paths_value;
use crate::value::{borrow_value, cr_value, give_new_value};
use crate::{cr_format, cr_status};

/// One node: a condition or a value, as in a rule's `when` or payload.
pub struct cr_node {
    pub(crate) node: Node,
}

/// Read a node in JSON or YAML (`spec/json-format.md`, *Conditions*: the node
/// itself, not a document), or in the rule syntax (`spec/text.md`, *Conditions
/// and values*), such as `customer.years > 5` or `order.total * 0.9`.
///
/// # Safety
///
/// `text` is `NULL` or points to `len` readable bytes; `limits` is `NULL` or a
/// live `cr_limits`; `out` is `NULL` or points to a writable `cr_node *`;
/// `error` is `NULL` or points to a writable `cr_error *` set to `NULL`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cr_node_read(
    text: *const c_char,
    len: usize,
    format: cr_format,
    limits: *const cr_limits,
    out: *mut *mut cr_node,
    error: *mut *mut cr_error,
) -> cr_status {
    let body = || {
        // SAFETY: `out` is NULL or writable (# Safety).
        unsafe { clear(out, "out") }?;
        let format = self::format(format)?;
        // SAFETY: `text` and `len` (# Safety).
        let text = unsafe { bytes(text, len, "text") }?;
        // SAFETY: `limits` is NULL or live (# Safety).
        let limits = unsafe { limits_of(limits) };
        let node = match format {
            DocumentFormat::Text => cleanrules::text::parse_node(utf8_text(text)?, &limits)?,
            DocumentFormat::Csv => {
                return Err(
                    Error::new(ErrorCode::Unsupported, "a node is not read from csv")
                        .with_hint("use JSON, YAML or text")
                        .unsupported("csv", "CSV holds decision tables")
                        .into(),
                );
            }
            other => read_node(&parse_as(text, other, &limits)?, "", &limits)?,
        };
        // SAFETY: `out` passed `clear`.
        unsafe { give(out, cr_node { node }) };
        Ok(())
    };
    // SAFETY: `error` is NULL or writable (# Safety).
    unsafe { call("cr_node_read", error, body) }
}

/// Write a node in JSON or YAML (the node itself), or in the rule syntax.
///
/// # Safety
///
/// `node` is `NULL` or a live `cr_node`; `out` is `NULL` or points to a
/// writable `char *`; `len` is `NULL` or points to a writable `size_t`;
/// `error` is `NULL` or points to a writable `cr_error *` set to `NULL`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cr_node_write(
    node: *const cr_node,
    format: cr_format,
    out: *mut *mut c_char,
    len: *mut usize,
    error: *mut *mut cr_error,
) -> cr_status {
    let body = || {
        // SAFETY: `out` is NULL or writable (# Safety).
        unsafe { clear(out, "out") }?;
        // SAFETY: `node` is NULL or live (# Safety).
        let node = &unsafe { handle(node, "node") }?.node;
        let text = match self::format(format)? {
            DocumentFormat::Json => to_string(&write_node(node)),
            DocumentFormat::Yaml => cleanrules::yaml::to_string(&write_node(node)),
            DocumentFormat::Text => node.to_string(),
            other => {
                return Err(Error::new(
                    ErrorCode::Unsupported,
                    format!("a node is not written in {}", other.as_str()),
                )
                .with_hint("use JSON, YAML or text")
                .unsupported(other.as_str(), "CSV holds decision tables")
                .into());
            }
        };
        // SAFETY: `out` passed `clear`; `len` is NULL or writable (# Safety).
        unsafe { give_text(out, len, text) }
    };
    // SAFETY: `error` is NULL or writable (# Safety).
    unsafe { call("cr_node_write", error, body) }
}

/// The logic a node is evaluated under, by name; `boolean` for `NULL`.
///
/// # Safety
///
/// `name` is `NULL` or points to `len` readable bytes.
pub(crate) unsafe fn logic(name: *const c_char, len: usize) -> Outcome<Kleene> {
    let name = if name.is_null() {
        "boolean"
    } else {
        // SAFETY: the caller's promise is `bytes`'.
        utf8_arg(unsafe { bytes(name, len, "logic") }?, "logic")?
    };
    match LogicName::from_name(name)? {
        LogicName::Boolean => Ok(Kleene),
        // A logic that cannot evaluate one node yet (phase J).
        #[allow(unreachable_patterns)]
        _ => Err(Error::new(
            ErrorCode::Unsupported,
            format!("the logic {name:?} cannot evaluate one node yet"),
        )
        .unsupported(
            "evaluating one node",
            "only boolean logic evaluates one node",
        )
        .into()),
    }
}

/// Evaluate a node on an input: its result tree as JSON. With a logic's name
/// (a pointer and a length), as a condition; without, as a value.
///
/// # Safety
///
/// As the exported functions below.
#[allow(clippy::too_many_arguments)]
unsafe fn evaluate(
    name: &str,
    node: *const cr_node,
    input: *const cr_value,
    logic: Option<(*const c_char, usize)>,
    strict: bool,
    out: *mut *mut c_char,
    len: *mut usize,
    error: *mut *mut cr_error,
) -> cr_status {
    let body = || {
        // SAFETY: `out` is NULL or writable (# Safety).
        unsafe { clear(out, "out") }?;
        // SAFETY: `node` is NULL or live (# Safety).
        let node = &unsafe { handle(node, "node") }?.node;
        // SAFETY: `input` is NULL or live (# Safety).
        let input = unsafe { borrow_value(input, "input") }?;
        let options = Options { strict, rule: None };
        let tree = match logic {
            Some((name, len)) => {
                // SAFETY: the name and its length (# Safety).
                let logic = unsafe { self::logic(name, len) }?;
                evaluate_tree(node, input, &logic, options)?
            }
            None => evaluate_value_tree(node, input, &Kleene, options)?,
        };
        // SAFETY: `out` passed `clear`; `len` is NULL or writable (# Safety).
        unsafe { give_text(out, len, to_string(&write_result(&tree))) }
    };
    // SAFETY: `error` is NULL or writable (# Safety).
    unsafe { call(name, error, body) }
}

/// Evaluate a node as a condition on an input (`spec/ast.md`, *Evaluating one
/// node*: `evaluate_tree`), under a logic by name (`boolean` for `NULL`; an
/// unknown one is `CR_REGISTRY_ERROR`), strict or not: its result tree as JSON
/// (`spec/json-format.md`, *Result trees*), whose root holds the truth value. A
/// value node such as `a + 1` is a `CR_EXPRESSION_ERROR`.
///
/// # Safety
///
/// `node` is `NULL` or a live `cr_node`; `input` is `NULL` or a live
/// `cr_value`; `logic` is `NULL` or points to `logic_len` readable bytes; `out`
/// is `NULL` or points to a writable `char *`; `len` is `NULL` or points to a
/// writable `size_t`; `error` is `NULL` or points to a writable `cr_error *`
/// set to `NULL`.
#[unsafe(no_mangle)]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn cr_node_evaluate_tree(
    node: *const cr_node,
    input: *const cr_value,
    logic: *const c_char,
    logic_len: usize,
    strict: bool,
    out: *mut *mut c_char,
    len: *mut usize,
    error: *mut *mut cr_error,
) -> cr_status {
    // SAFETY: the caller's promises are `evaluate`'s.
    unsafe {
        evaluate(
            "cr_node_evaluate_tree",
            node,
            input,
            Some((logic, logic_len)),
            strict,
            out,
            len,
            error,
        )
    }
}

/// Evaluate a node as a value on an input, strict or not (`spec/ast.md`,
/// *Evaluating one node*: `evaluate_value_tree`): its result tree, whose root
/// holds the value. A condition such as `a > 1` is a `CR_EXPRESSION_ERROR`.
///
/// # Safety
///
/// `node` is `NULL` or a live `cr_node`; `input` is `NULL` or a live
/// `cr_value`; `out` is `NULL` or points to a writable `char *`; `len` is
/// `NULL` or points to a writable `size_t`; `error` is `NULL` or points to a
/// writable `cr_error *` set to `NULL`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cr_node_evaluate_value_tree(
    node: *const cr_node,
    input: *const cr_value,
    strict: bool,
    out: *mut *mut c_char,
    len: *mut usize,
    error: *mut *mut cr_error,
) -> cr_status {
    // SAFETY: the caller's promises are `evaluate`'s.
    unsafe {
        evaluate(
            "cr_node_evaluate_value_tree",
            node,
            input,
            None,
            strict,
            out,
            len,
            error,
        )
    }
}

/// The paths of the input that the node reads, in preorder and each once, as
/// a new list of paths, each a list of strings (keys) and ints (indexes),
/// freed with `cr_value_free`. A binding reads only these from native data
/// (`spec/data-model.md`, *Reading native data*), as with
/// `cr_ruleset_field_paths`.
///
/// # Safety
///
/// `node` is `NULL` or a live `cr_node`; `out` is `NULL` or points to a
/// writable `cr_value *`; `error` is `NULL` or points to a writable
/// `cr_error *` set to `NULL`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cr_node_field_paths(
    node: *const cr_node,
    out: *mut *mut cr_value,
    error: *mut *mut cr_error,
) -> cr_status {
    let body = || {
        // SAFETY: `out` is NULL or writable (# Safety).
        unsafe { clear(out, "out") }?;
        // SAFETY: `node` is NULL or live (# Safety).
        let node = &unsafe { handle(node, "node") }?.node;
        let paths = paths_value(node.field_paths().into_iter());
        // SAFETY: `out` is NULL or writable (# Safety).
        unsafe { give_new_value(out, paths) }
    };
    // SAFETY: `error` is NULL or writable (# Safety).
    unsafe { call("cr_node_field_paths", error, body) }
}

/// Free a node. `NULL` is ignored.
///
/// # Safety
///
/// `node` is `NULL` or a `cr_node` from this library, not freed before.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cr_node_free(node: *mut cr_node) {
    // SAFETY: the caller's promise is `free`'s.
    unsafe { free(node) };
}
