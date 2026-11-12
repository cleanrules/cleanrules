//! Diagrams (`spec/visual.md`): conditions, rules, rule sets, explanations and
//! decision tables drawn as DOT, Mermaid or HTML text.
//!
//! Each function writes a new text to `*out`, which the caller frees with
//! `cr_string_free`, and its length in bytes to `*len` when `len` is not
//! `NULL`; the text holds no NUL. On failure, `*out` is `NULL`.

use std::ffi::c_char;

use cleanrules::explain::{explain, summary};
use cleanrules::visual::{
    DiagramFormat, draw_condition, draw_document, draw_explanation, draw_rule, draw_ruleset,
};

use crate::boundary::{Failure, Outcome, call, clear, give_text, handle, limits_of};
use crate::cr_status;
use crate::decision::cr_decision;
use crate::document::cr_document;
use crate::error::cr_error;
use crate::limits::cr_limits;
use crate::node::cr_node;
use crate::rule::cr_rule;
use crate::ruleset::cr_ruleset;

/// A format diagrams are written in (`CR_DIAGRAM_*`).
pub type cr_diagram = i32;

/// A Graphviz graph (DOT).
pub const CR_DIAGRAM_DOT: cr_diagram = 1;
/// A Mermaid flowchart.
pub const CR_DIAGRAM_MERMAID: cr_diagram = 2;
/// A standalone HTML document, with no script and no external resource.
pub const CR_DIAGRAM_HTML: cr_diagram = 3;
/// The HTML document's style and content, to embed in a page or a notebook.
pub const CR_DIAGRAM_HTML_FRAGMENT: cr_diagram = 4;

/// A diagram format argument.
fn diagram(format: cr_diagram) -> Outcome<DiagramFormat> {
    match format {
        CR_DIAGRAM_DOT => Ok(DiagramFormat::Dot),
        CR_DIAGRAM_MERMAID => Ok(DiagramFormat::Mermaid),
        CR_DIAGRAM_HTML => Ok(DiagramFormat::Html),
        CR_DIAGRAM_HTML_FRAGMENT => Ok(DiagramFormat::HtmlFragment),
        other => Err(Failure::Invalid(
            format!("diagram format {other} is not a CR_DIAGRAM_* constant"),
            "use CR_DIAGRAM_DOT (1), CR_DIAGRAM_MERMAID (2), CR_DIAGRAM_HTML (3) or \
             CR_DIAGRAM_HTML_FRAGMENT (4)",
        )),
    }
}

/// The tree of a condition, or of any node (`spec/visual.md`, *Conditions*),
/// in `format` (`CR_DIAGRAM_*`). A diagram of more elements than
/// `CR_LIMIT_MAX_DIAGRAM_SIZE` is a `CR_LIMIT_EXCEEDED`.
///
/// # Safety
///
/// `node` is `NULL` or a live `cr_node`; `limits` is `NULL` or a live
/// `cr_limits`; `out` is `NULL` or points to a writable `char *`; `len` is
/// `NULL` or points to a writable `size_t`; `error` is `NULL` or points to a
/// writable `cr_error *` set to `NULL`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cr_node_draw(
    node: *const cr_node,
    format: cr_diagram,
    limits: *const cr_limits,
    out: *mut *mut c_char,
    len: *mut usize,
    error: *mut *mut cr_error,
) -> cr_status {
    let body = || {
        // SAFETY: `out` is NULL or writable (# Safety).
        unsafe { clear(out, "out") }?;
        // SAFETY: `node` is NULL or live (# Safety).
        let node = &unsafe { handle(node, "node") }?.node;
        let format = diagram(format)?;
        // SAFETY: `limits` is NULL or live (# Safety).
        let limits = unsafe { limits_of(limits) };
        let text = draw_condition(node, format, &limits)?;
        // SAFETY: `out` passed `clear`; `len` is NULL or writable (# Safety).
        unsafe { give_text(out, len, text) }
    };
    // SAFETY: `error` is NULL or writable (# Safety).
    unsafe { call(error, body) }
}

/// A rule's diagram (`spec/visual.md`, *Rules*), in `format`
/// (`CR_DIAGRAM_*`).
///
/// # Safety
///
/// `rule` is `NULL` or a live `cr_rule`; `limits` is `NULL` or a live
/// `cr_limits`; `out` is `NULL` or points to a writable `char *`; `len` is
/// `NULL` or points to a writable `size_t`; `error` is `NULL` or points to a
/// writable `cr_error *` set to `NULL`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cr_rule_draw(
    rule: *const cr_rule,
    format: cr_diagram,
    limits: *const cr_limits,
    out: *mut *mut c_char,
    len: *mut usize,
    error: *mut *mut cr_error,
) -> cr_status {
    let body = || {
        // SAFETY: `out` is NULL or writable (# Safety).
        unsafe { clear(out, "out") }?;
        // SAFETY: `rule` is NULL or live (# Safety).
        let rule = &unsafe { handle(rule, "rule") }?.rule;
        let format = diagram(format)?;
        // SAFETY: `limits` is NULL or live (# Safety).
        let limits = unsafe { limits_of(limits) };
        let text = draw_rule(rule, format, &limits)?;
        // SAFETY: `out` passed `clear`; `len` is NULL or writable (# Safety).
        unsafe { give_text(out, len, text) }
    };
    // SAFETY: `error` is NULL or writable (# Safety).
    unsafe { call(error, body) }
}

/// A rule set's diagram, from its summary (`spec/visual.md`, *Rule sets*), in
/// `format` (`CR_DIAGRAM_*`), with the rules whose action is code of the
/// program marked as `cr_ruleset_summary_actions` marks them: `actions` holds
/// one flag per rule, in rule order, or none (`actions_len` 0). Another number
/// of flags, or `NULL` with flags, is `CR_INVALID_ARGUMENT`.
///
/// # Safety
///
/// `ruleset` is `NULL` or a live `cr_ruleset`; `actions` is `NULL` or points
/// to `actions_len` readable `bool`s; `limits` is `NULL` or a live
/// `cr_limits`; `out` is `NULL` or points to a writable `char *`; `len` is
/// `NULL` or points to a writable `size_t`; `error` is `NULL` or points to a
/// writable `cr_error *` set to `NULL`.
#[unsafe(no_mangle)]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn cr_ruleset_draw(
    ruleset: *const cr_ruleset,
    actions: *const bool,
    actions_len: usize,
    format: cr_diagram,
    limits: *const cr_limits,
    out: *mut *mut c_char,
    len: *mut usize,
    error: *mut *mut cr_error,
) -> cr_status {
    let body = || {
        // SAFETY: `out` is NULL or writable (# Safety).
        unsafe { clear(out, "out") }?;
        // SAFETY: `ruleset` is NULL or live (# Safety).
        let ruleset = &unsafe { handle(ruleset, "ruleset") }?.ruleset;
        let count = ruleset.rules().len();
        if actions_len != 0 && actions_len != count {
            return Err(Failure::Invalid(
                format!("actions_len is {actions_len}, and the rule set has {count} rules"),
                "pass one flag per rule, or none",
            ));
        }
        let flags: &[bool] = if actions_len == 0 {
            &[]
        } else if actions.is_null() {
            return Err(Failure::Invalid(
                format!("actions is NULL with a length of {actions_len}"),
                "pass the flags, or a length of 0",
            ));
        } else {
            // SAFETY: the caller promises `actions_len` readable flags.
            unsafe { std::slice::from_raw_parts(actions, actions_len) }
        };
        let format = diagram(format)?;
        // SAFETY: `limits` is NULL or live (# Safety).
        let limits = unsafe { limits_of(limits) };
        let overview = summary(ruleset).with_actions(flags.iter().copied());
        let text = draw_ruleset(&overview, format, &limits)?;
        // SAFETY: `out` passed `clear`; `len` is NULL or writable (# Safety).
        unsafe { give_text(out, len, text) }
    };
    // SAFETY: `error` is NULL or writable (# Safety).
    unsafe { call(error, body) }
}

/// The diagram of a decision's explanation (`spec/visual.md`,
/// *Explanations*), in `format` (`CR_DIAGRAM_*`).
///
/// # Safety
///
/// `decision` is `NULL` or a live `cr_decision`; `limits` is `NULL` or a live
/// `cr_limits`; `out` is `NULL` or points to a writable `char *`; `len` is
/// `NULL` or points to a writable `size_t`; `error` is `NULL` or points to a
/// writable `cr_error *` set to `NULL`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cr_decision_draw(
    decision: *const cr_decision,
    format: cr_diagram,
    limits: *const cr_limits,
    out: *mut *mut c_char,
    len: *mut usize,
    error: *mut *mut cr_error,
) -> cr_status {
    let body = || {
        // SAFETY: `out` is NULL or writable (# Safety).
        unsafe { clear(out, "out") }?;
        // SAFETY: `decision` is NULL or live (# Safety).
        let decision = unsafe { handle(decision, "decision") }?;
        let format = diagram(format)?;
        // SAFETY: `limits` is NULL or live (# Safety).
        let limits = unsafe { limits_of(limits) };
        let text = decision.with(|d| Ok(draw_explanation(&explain(d), format, &limits)?))?;
        // SAFETY: `out` passed `clear`; `len` is NULL or writable (# Safety).
        unsafe { give_text(out, len, text) }
    };
    // SAFETY: `error` is NULL or writable (# Safety).
    unsafe { call(error, body) }
}

/// A document's diagram, in `format` (`CR_DIAGRAM_*`): a condition's, a
/// rule's, a rule set's (from its summary) or a decision table's grid, which
/// is HTML only (DOT and Mermaid are `CR_UNSUPPORTED`). `decision` is `NULL`,
/// or, for a decision table, a decision of the table's rule set
/// (`cr_ruleset_from_document`), whose statuses the grid shows; a decision of
/// another rule set is a `CR_DECISION_MISMATCH`, and a decision with any other
/// document `CR_UNSUPPORTED`.
///
/// # Safety
///
/// `document` is `NULL` or a live `cr_document`; `decision` is `NULL` or a
/// live `cr_decision`; `limits` is `NULL` or a live `cr_limits`; `out` is
/// `NULL` or points to a writable `char *`; `len` is `NULL` or points to a
/// writable `size_t`; `error` is `NULL` or points to a writable `cr_error *`
/// set to `NULL`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cr_document_draw(
    document: *const cr_document,
    decision: *const cr_decision,
    format: cr_diagram,
    limits: *const cr_limits,
    out: *mut *mut c_char,
    len: *mut usize,
    error: *mut *mut cr_error,
) -> cr_status {
    let body = || {
        // SAFETY: `out` is NULL or writable (# Safety).
        unsafe { clear(out, "out") }?;
        // SAFETY: `document` is NULL or live (# Safety).
        let document = &unsafe { handle(document, "document") }?.document;
        let format = diagram(format)?;
        // SAFETY: `limits` is NULL or live (# Safety).
        let limits = unsafe { limits_of(limits) };
        let text = if decision.is_null() {
            draw_document(document, None, format, &limits)?
        } else {
            // SAFETY: `decision` is live (# Safety) and not NULL.
            let decision = unsafe { handle(decision, "decision") }?;
            decision.with(|d| Ok(draw_document(document, Some(d), format, &limits)?))?
        };
        // SAFETY: `out` passed `clear`; `len` is NULL or writable (# Safety).
        unsafe { give_text(out, len, text) }
    };
    // SAFETY: `error` is NULL or writable (# Safety).
    unsafe { call(error, body) }
}
