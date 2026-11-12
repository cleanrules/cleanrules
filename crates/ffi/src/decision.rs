//! Decisions (`spec/rules.md`, `spec/json-format.md`, *Decisions*): evaluating
//! an input, reading a recorded decision, and what a decision says.

use std::ffi::c_char;
use std::sync::Arc;

use cleanrules::explain::explain;
use cleanrules::format::{
    ValueContext, explanation_value, parse_as, read_parsed_decision_document, read_value,
    write_decision_as, write_explanation_as, write_value,
};
use cleanrules::json::{Json, Limits, parse_bytes, to_string};
use cleanrules::logic::Truth;
use cleanrules::rules::{Decision, RuleSet};
use cleanrules::{Map, Value};

use crate::boundary::{
    Failure, Outcome, bytes, call, clear, clear_borrowed, format, free, give, give_text, handle,
    limits_of, write_out,
};
use crate::error::cr_error;
use crate::limits::cr_limits;
use crate::ruleset::cr_ruleset;
use crate::value::{borrow_text, borrow_value, cr_value, give_new_value, nesting};
use crate::{cr_format, cr_status};

/// Where a decision comes from: it is made again from this whenever it is read,
/// which gives the same decision (CLAUDE.md, invariant 7).
#[derive(Clone)]
enum Source {
    /// An input the rule set evaluated.
    Input(Value),
    /// A recorded decision the rule set accepted.
    Recorded(Json),
}

/// What a decision says, kept when it is made, so that reading it costs
/// nothing; writing and explaining it make it again.
#[derive(Clone)]
struct Facts {
    fired: Vec<bool>,
    truths: Vec<Truth>,
    selected: Vec<usize>,
    /// Each rule's output, when its payload was evaluated.
    rule_outputs: Vec<Option<Vec<(String, Value)>>>,
    outputs: Vec<Vec<(String, Value)>>,
    output: Output,
}

/// The single output of a decision.
#[derive(Clone)]
enum Output {
    /// The winner's output or the aggregate; `None` when nothing was selected.
    Single(Option<Value>),
    /// A policy with several outputs: asking for one is an error.
    Several,
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
            output: decision.output().map_or(Output::Several, Output::Single),
        }
    }
}

/// A decision: what a rule set made of one input, or a recorded decision read
/// with its rule set. What rules fired and were selected, and the outputs, are
/// kept when it is made; writing and explaining it evaluate the input, or check
/// the recorded decision, again.
#[derive(Clone)]
pub struct cr_decision {
    ruleset: Arc<RuleSet>,
    source: Source,
    limits: Limits,
    facts: Facts,
}

impl cr_decision {
    fn new(ruleset: Arc<RuleSet>, source: Source, limits: Limits) -> Outcome<Self> {
        let facts = match &source {
            Source::Input(input) => Facts::of(&ruleset.evaluate(input)?),
            Source::Recorded(json) => {
                Facts::of(&read_parsed_decision_document(json, &ruleset, &limits)?)
            }
        };
        Ok(Self {
            ruleset,
            source,
            limits,
            facts,
        })
    }

    /// The decision of the rule set on an input.
    pub(crate) fn evaluated(ruleset: Arc<RuleSet>, input: Value, limits: Limits) -> Outcome<Self> {
        Self::new(ruleset, Source::Input(input), limits)
    }

    /// A decision the core already made from `input`.
    pub(crate) fn made(
        ruleset: Arc<RuleSet>,
        input: Value,
        limits: Limits,
        decision: &Decision<'_>,
    ) -> Self {
        Self {
            facts: Facts::of(decision),
            ruleset,
            source: Source::Input(input),
            limits,
        }
    }

    /// The input the decision was made from, when it was evaluated.
    pub(crate) fn input(&self) -> Option<&Value> {
        match &self.source {
            Source::Input(input) => Some(input),
            Source::Recorded(_) => None,
        }
    }

    pub(crate) fn ruleset(&self) -> &RuleSet {
        &self.ruleset
    }

    pub(crate) fn selected(&self) -> &[usize] {
        &self.facts.selected
    }

    fn rule(&self, rule: usize) -> Outcome<usize> {
        let count = self.facts.fired.len();
        if rule < count {
            Ok(rule)
        } else {
            Err(Failure::Invalid(
                format!("rule {rule} is out of the rule set of {count}"),
                "pass a position below cr_decision_len",
            ))
        }
    }

    /// The decision, made again; it succeeded before, so it succeeds now.
    /// The single output; an error for a policy with several.
    fn output(&self) -> Outcome<&Option<Value>> {
        match &self.facts.output {
            Output::Single(output) => Ok(output),
            // Made again for its error, which the decision does not keep.
            Output::Several => self
                .with(|d| d.output().map(|_| ()).map_err(Failure::from))
                .and(Err(Failure::Internal(
                    "an output that failed once succeeds; please report it".into(),
                ))),
        }
    }

    /// The outputs of the selected rules, as a list of maps.
    fn outputs(&self) -> Value {
        Value::List(
            self.facts
                .outputs
                .iter()
                .map(|pairs| {
                    let mut map = Map::new();
                    for (key, value) in pairs {
                        map.insert(key.clone(), value.clone());
                    }
                    Value::Map(map)
                })
                .collect(),
        )
    }

    /// What `make` gives of the result tree of the condition of the rule at
    /// position `rule`, made again.
    pub(crate) fn rule_tree<T>(
        &self,
        rule: usize,
        make: impl FnOnce(&cleanrules::eval::Evaluated<'_, Truth>) -> T,
    ) -> Outcome<T> {
        let position = self.rule(rule)?;
        self.with(|d| Ok(make(&d.results()[position].when)))
    }

    pub(crate) fn with<T>(&self, read: impl FnOnce(&Decision<'_>) -> Outcome<T>) -> Outcome<T> {
        let decision = match &self.source {
            Source::Input(input) => self.ruleset.evaluate(input)?,
            Source::Recorded(json) => {
                read_parsed_decision_document(json, &self.ruleset, &self.limits)?
            }
        };
        read(&decision)
    }
}

/// Evaluate the rule set on an input: a data value as JSON
/// (`spec/json-format.md`, *Values*), such as `{"customer": {"years": 7}}`. In
/// strict mode, a field the input lacks is a `CR_MISSING_FIELD`, and so on
/// (`spec/rules.md`).
///
/// # Safety
///
/// `ruleset` is `NULL` or a live `cr_ruleset`; `input` is `NULL` or points to
/// `len` readable bytes; `limits` is `NULL` or a live `cr_limits`; `out` is
/// `NULL` or points to a writable `cr_decision *`; `error` is `NULL` or points
/// to a writable `cr_error *` set to `NULL`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cr_ruleset_evaluate(
    ruleset: *const cr_ruleset,
    input: *const c_char,
    len: usize,
    limits: *const cr_limits,
    out: *mut *mut cr_decision,
    error: *mut *mut cr_error,
) -> cr_status {
    let body = || {
        // SAFETY: `out` is NULL or writable (# Safety).
        unsafe { clear(out, "out") }?;
        // SAFETY: `ruleset` is NULL or live (# Safety).
        let ruleset = Arc::clone(&unsafe { handle(ruleset, "ruleset") }?.ruleset);
        // SAFETY: `limits` is NULL or live (# Safety).
        let limits = unsafe { limits_of(limits) };
        // SAFETY: `input` and `len` (# Safety).
        let input = unsafe { bytes(input, len, "input") }?;
        let input = read_value(&parse_bytes(input, &limits)?, "", ValueContext::Data)?;
        let decision = cr_decision::new(ruleset, Source::Input(input), limits)?;
        // SAFETY: `out` passed `clear`.
        unsafe { give(out, decision) };
        Ok(())
    };
    // SAFETY: `error` is NULL or writable (# Safety).
    unsafe { call(error, body) }
}

/// Read a recorded decision, in JSON or YAML (`CR_FORMAT_*`), with the rule set
/// that made it (`spec/json-format.md`, *Reading a decision*): one that does
/// not belong to it is a `CR_DECISION_MISMATCH`. The rule syntax and CSV hold
/// no decisions: `CR_UNSUPPORTED`.
///
/// # Safety
///
/// `ruleset` is `NULL` or a live `cr_ruleset`; `text` is `NULL` or points to
/// `len` readable bytes; `limits` is `NULL` or a live `cr_limits`; `out` is
/// `NULL` or points to a writable `cr_decision *`; `error` is `NULL` or points
/// to a writable `cr_error *` set to `NULL`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cr_ruleset_read_decision(
    ruleset: *const cr_ruleset,
    text: *const c_char,
    len: usize,
    format: cr_format,
    limits: *const cr_limits,
    out: *mut *mut cr_decision,
    error: *mut *mut cr_error,
) -> cr_status {
    let body = || {
        // SAFETY: `out` is NULL or writable (# Safety).
        unsafe { clear(out, "out") }?;
        // SAFETY: `ruleset` is NULL or live (# Safety).
        let ruleset = Arc::clone(&unsafe { handle(ruleset, "ruleset") }?.ruleset);
        // SAFETY: `limits` is NULL or live (# Safety).
        let limits = unsafe { limits_of(limits) };
        let format = self::format(format)?;
        // SAFETY: `text` and `len` (# Safety).
        let text = unsafe { bytes(text, len, "text") }?;
        let json = parse_as(text, format, &limits)?;
        let decision = cr_decision::new(ruleset, Source::Recorded(json), limits)?;
        // SAFETY: `out` passed `clear`.
        unsafe { give(out, decision) };
        Ok(())
    };
    // SAFETY: `error` is NULL or writable (# Safety).
    unsafe { call(error, body) }
}

/// Write a decision as a JSON or YAML document (`CR_FORMAT_*`). A decision has
/// no text form: `CR_UNSUPPORTED`.
///
/// # Safety
///
/// `decision` is `NULL` or a live `cr_decision`; `out` is `NULL` or points to a
/// writable `char *`; `len` is `NULL` or points to a writable `size_t`;
/// `error` is `NULL` or points to a writable `cr_error *` set to `NULL`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cr_decision_write(
    decision: *const cr_decision,
    format: cr_format,
    out: *mut *mut c_char,
    len: *mut usize,
    error: *mut *mut cr_error,
) -> cr_status {
    let body = || {
        // SAFETY: `out` is NULL or writable (# Safety).
        unsafe { clear(out, "out") }?;
        // SAFETY: `decision` is NULL or live (# Safety).
        let decision = unsafe { handle(decision, "decision") }?;
        let format = self::format(format)?;
        let text = decision.with(|d| Ok(write_decision_as(d, format)?))?;
        // SAFETY: `out` passed `clear`; `len` is NULL or writable (# Safety).
        unsafe { give_text(out, len, text) }
    };
    // SAFETY: `error` is NULL or writable (# Safety).
    unsafe { call(error, body) }
}

/// The decision's single output (`spec/rules.md`, *Outputs*), as a JSON data
/// value: the winner's payload as an object, the aggregate, or `null` when no
/// rule was selected. Policies with several outputs (`rule_order`, `collect`
/// without an aggregate) give `CR_HIT_POLICY_ERROR`: use
/// `cr_decision_outputs`.
///
/// # Safety
///
/// `decision` is `NULL` or a live `cr_decision`; `out` is `NULL` or points to a
/// writable `char *`; `len` is `NULL` or points to a writable `size_t`;
/// `error` is `NULL` or points to a writable `cr_error *` set to `NULL`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cr_decision_output(
    decision: *const cr_decision,
    out: *mut *mut c_char,
    len: *mut usize,
    error: *mut *mut cr_error,
) -> cr_status {
    let body = || {
        // SAFETY: `out` is NULL or writable (# Safety).
        unsafe { clear(out, "out") }?;
        // SAFETY: `decision` is NULL or live (# Safety).
        let decision = unsafe { handle(decision, "decision") }?;
        let output = decision.output()?;
        let json = output.as_ref().map_or(Json::Null, write_value);
        // SAFETY: `out` passed `clear`; `len` is NULL or writable (# Safety).
        unsafe { give_text(out, len, to_string(&json)) }
    };
    // SAFETY: `error` is NULL or writable (# Safety).
    unsafe { call(error, body) }
}

/// The outputs of the selected rules, in rule order, as a JSON array of data
/// values (objects).
///
/// # Safety
///
/// `decision` is `NULL` or a live `cr_decision`; `out` is `NULL` or points to a
/// writable `char *`; `len` is `NULL` or points to a writable `size_t`;
/// `error` is `NULL` or points to a writable `cr_error *` set to `NULL`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cr_decision_outputs(
    decision: *const cr_decision,
    out: *mut *mut c_char,
    len: *mut usize,
    error: *mut *mut cr_error,
) -> cr_status {
    let body = || {
        // SAFETY: `out` is NULL or writable (# Safety).
        unsafe { clear(out, "out") }?;
        // SAFETY: `decision` is NULL or live (# Safety).
        let decision = unsafe { handle(decision, "decision") }?;
        // SAFETY: `out` passed `clear`; `len` is NULL or writable (# Safety).
        unsafe { give_text(out, len, to_string(&write_value(&decision.outputs()))) }
    };
    // SAFETY: `error` is NULL or writable (# Safety).
    unsafe { call(error, body) }
}

/// The decision's explanation (`spec/explain.md`) as a JSON or YAML document,
/// or as text (`CR_FORMAT_TEXT`).
///
/// # Safety
///
/// `decision` is `NULL` or a live `cr_decision`; `out` is `NULL` or points to a
/// writable `char *`; `len` is `NULL` or points to a writable `size_t`;
/// `error` is `NULL` or points to a writable `cr_error *` set to `NULL`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cr_decision_explain(
    decision: *const cr_decision,
    format: cr_format,
    out: *mut *mut c_char,
    len: *mut usize,
    error: *mut *mut cr_error,
) -> cr_status {
    let body = || {
        // SAFETY: `out` is NULL or writable (# Safety).
        unsafe { clear(out, "out") }?;
        // SAFETY: `decision` is NULL or live (# Safety).
        let decision = unsafe { handle(decision, "decision") }?;
        let format = self::format(format)?;
        let text = decision.with(|d| Ok(write_explanation_as(&explain(d), format)?))?;
        // SAFETY: `out` passed `clear`; `len` is NULL or writable (# Safety).
        unsafe { give_text(out, len, text) }
    };
    // SAFETY: `error` is NULL or writable (# Safety).
    unsafe { call(error, body) }
}

/// The decision's explanation (`spec/explain.md`) as a new value, freed with
/// `cr_value_free`, for bindings that build objects of it: the structure of
/// the explanation document (`spec/json-format.md`, *Explanations*: the map
/// under `explanation`, with `hit_policy`, `logic`, `selected` and `rules`),
/// where values are the values themselves (an opaque one keeps its token), a
/// cause's `condition` is the JSON text of its node, a truth is a boolean or
/// null, and a gap is a number or a map of `days` or `microseconds` to an int.
/// It adds two keys to the document's: `text`, the part of the explanation's
/// text of each rule and of each cause, and `holds`, whether each cause went
/// the way its rule needed. Maps may gain keys in later versions.
///
/// # Safety
///
/// `decision` is `NULL` or a live `cr_decision`; `out` is `NULL` or points to
/// a writable `cr_value *`; `error` is `NULL` or points to a writable
/// `cr_error *` set to `NULL`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cr_decision_explain_value(
    decision: *const cr_decision,
    out: *mut *mut cr_value,
    error: *mut *mut cr_error,
) -> cr_status {
    let body = || {
        // SAFETY: `decision` is NULL or live (# Safety).
        let decision = unsafe { handle(decision, "decision") }?;
        let value = decision.with(|d| Ok(explanation_value(&explain(d))))?;
        // SAFETY: `out` is NULL or writable (# Safety).
        unsafe { give_new_value(out, value) }
    };
    // SAFETY: `error` is NULL or writable (# Safety).
    unsafe { call(error, body) }
}

/// A copy of a decision that the caller owns, freed with `cr_decision_free`:
/// to keep a decision borrowed from a batch, an action call or an error after
/// what lends it is freed. It shares the rule set and holds its own copy of
/// the input.
///
/// # Safety
///
/// `decision` is `NULL` or a live `cr_decision`; `out` is `NULL` or points to
/// a writable `cr_decision *`; `error` is `NULL` or points to a writable
/// `cr_error *` set to `NULL`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cr_decision_clone(
    decision: *const cr_decision,
    out: *mut *mut cr_decision,
    error: *mut *mut cr_error,
) -> cr_status {
    let body = || {
        // SAFETY: `out` is NULL or writable (# Safety).
        unsafe { clear(out, "out") }?;
        // SAFETY: `decision` is NULL or live (# Safety).
        let copy = unsafe { handle(decision, "decision") }?.clone();
        // SAFETY: `out` passed `clear`.
        unsafe { give(out, copy) };
        Ok(())
    };
    // SAFETY: `error` is NULL or writable (# Safety).
    unsafe { call(error, body) }
}

/// Free a decision. `NULL` is ignored.
///
/// # Safety
///
/// `decision` is `NULL` or a `cr_decision` from this library, not freed before.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cr_decision_free(decision: *mut cr_decision) {
    // SAFETY: the caller's promise is `free`'s.
    unsafe { free(decision) };
}

/// Evaluate the rule set on an input built with the `cr_value_*` functions (or
/// read with `cr_value_from_json`), which stays the caller's: the decision
/// keeps a copy. Lists and maps nested deeper than the limits' JSON nesting
/// are `CR_LIMIT_EXCEEDED`, as in JSON.
///
/// # Safety
///
/// `ruleset` is `NULL` or a live `cr_ruleset`; `input` is `NULL` or a live
/// `cr_value`; `limits` is `NULL` or a live `cr_limits`; `out` is `NULL` or
/// points to a writable `cr_decision *`; `error` is `NULL` or points to a
/// writable `cr_error *` set to `NULL`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cr_ruleset_evaluate_value(
    ruleset: *const cr_ruleset,
    input: *const cr_value,
    limits: *const cr_limits,
    out: *mut *mut cr_decision,
    error: *mut *mut cr_error,
) -> cr_status {
    let body = || {
        // SAFETY: `out` is NULL or writable (# Safety).
        unsafe { clear(out, "out") }?;
        // SAFETY: `ruleset` is NULL or live (# Safety).
        let ruleset = Arc::clone(&unsafe { handle(ruleset, "ruleset") }?.ruleset);
        // SAFETY: `limits` is NULL or live (# Safety).
        let limits = unsafe { limits_of(limits) };
        // SAFETY: `input` is NULL or live (# Safety).
        let input = unsafe { borrow_value(input, "input") }?;
        check_input(input, &limits)?;
        let decision = cr_decision::new(ruleset, Source::Input(input.clone()), limits)?;
        // SAFETY: `out` passed `clear`.
        unsafe { give(out, decision) };
        Ok(())
    };
    // SAFETY: `error` is NULL or writable (# Safety).
    unsafe { call(error, body) }
}

/// A truth value (`CR_TRUTH_*`).
pub type cr_truth = i32;

/// True.
pub const CR_TRUTH_TRUE: cr_truth = 1;
/// False.
pub const CR_TRUTH_FALSE: cr_truth = 2;
/// Unknown: a missing field, a null or a type mismatch.
pub const CR_TRUTH_UNKNOWN: cr_truth = 3;

/// The number of rules of the decision's rule set; 0 for `NULL`.
///
/// # Safety
///
/// `decision` is `NULL` or a live `cr_decision`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cr_decision_len(decision: *const cr_decision) -> usize {
    // SAFETY: `decision` is NULL or live (# Safety).
    unsafe { decision.as_ref() }.map_or(0, |d| d.facts.fired.len())
}

/// Whether the rule at position `rule` fired: its condition passed the logic's
/// activation test (`spec/rules.md`).
///
/// # Safety
///
/// `decision` is `NULL` or a live `cr_decision`; `out` is `NULL` or points to a
/// writable `bool`; `error` is `NULL` or points to a writable `cr_error *` set
/// to `NULL`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cr_decision_fired(
    decision: *const cr_decision,
    rule: usize,
    out: *mut bool,
    error: *mut *mut cr_error,
) -> cr_status {
    let body = || {
        // SAFETY: `decision` is NULL or live (# Safety).
        let decision = unsafe { handle(decision, "decision") }?;
        let fired = decision.facts.fired[decision.rule(rule)?];
        // SAFETY: `out` is NULL or writable (# Safety).
        unsafe { write_out(out, fired, "out") }
    };
    // SAFETY: `error` is NULL or writable (# Safety).
    unsafe { call(error, body) }
}

/// The truth value of the condition of the rule at position `rule`
/// (`CR_TRUTH_TRUE`, `CR_TRUTH_FALSE`, `CR_TRUTH_UNKNOWN`).
///
/// # Safety
///
/// `decision` is `NULL` or a live `cr_decision`; `out` is `NULL` or points to a
/// writable `cr_truth`; `error` is `NULL` or points to a writable `cr_error *`
/// set to `NULL`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cr_decision_truth(
    decision: *const cr_decision,
    rule: usize,
    out: *mut cr_truth,
    error: *mut *mut cr_error,
) -> cr_status {
    let body = || {
        // SAFETY: `decision` is NULL or live (# Safety).
        let decision = unsafe { handle(decision, "decision") }?;
        let truth = match decision.facts.truths[decision.rule(rule)?] {
            Truth::True => CR_TRUTH_TRUE,
            Truth::False => CR_TRUTH_FALSE,
            Truth::Unknown => CR_TRUTH_UNKNOWN,
        };
        // SAFETY: `out` is NULL or writable (# Safety).
        unsafe { write_out(out, truth, "out") }
    };
    // SAFETY: `error` is NULL or writable (# Safety).
    unsafe { call(error, body) }
}

/// The number of rules the hit policy selected; 0 for `NULL`.
///
/// # Safety
///
/// `decision` is `NULL` or a live `cr_decision`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cr_decision_selected_len(decision: *const cr_decision) -> usize {
    // SAFETY: `decision` is NULL or live (# Safety).
    unsafe { decision.as_ref() }.map_or(0, |d| d.facts.selected.len())
}

/// The position in the rule set of the `index`-th selected rule, in rule order.
///
/// # Safety
///
/// `decision` is `NULL` or a live `cr_decision`; `rule` is `NULL` or points to
/// a writable `size_t`; `error` is `NULL` or points to a writable `cr_error *`
/// set to `NULL`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cr_decision_selected(
    decision: *const cr_decision,
    index: usize,
    rule: *mut usize,
    error: *mut *mut cr_error,
) -> cr_status {
    let body = || {
        // SAFETY: `decision` is NULL or live (# Safety).
        let decision = unsafe { handle(decision, "decision") }?;
        let selected = &decision.facts.selected;
        let position = *selected.get(index).ok_or_else(|| {
            Failure::Invalid(
                format!(
                    "index {index} is out of the {} selected rules",
                    selected.len()
                ),
                "pass a position below cr_decision_selected_len",
            )
        })?;
        // SAFETY: `rule` is NULL or writable (# Safety).
        unsafe { write_out(rule, position, "rule") }
    };
    // SAFETY: `error` is NULL or writable (# Safety).
    unsafe { call(error, body) }
}

/// The decision's single output as a new value, freed with `cr_value_free`: a
/// map for the winner's payload, the aggregate, or a plain null when no rule
/// was selected. Policies with several outputs give `CR_HIT_POLICY_ERROR`.
///
/// # Safety
///
/// `decision` is `NULL` or a live `cr_decision`; `out` is `NULL` or points to a
/// writable `cr_value *`; `error` is `NULL` or points to a writable
/// `cr_error *` set to `NULL`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cr_decision_output_value(
    decision: *const cr_decision,
    out: *mut *mut cr_value,
    error: *mut *mut cr_error,
) -> cr_status {
    let body = || {
        // SAFETY: `out` is NULL or writable (# Safety).
        unsafe { clear(out, "out") }?;
        // SAFETY: `decision` is NULL or live (# Safety).
        let decision = unsafe { handle(decision, "decision") }?;
        let value = decision
            .output()?
            .clone()
            .unwrap_or(Value::Null(cleanrules::NullCause::Null));
        // SAFETY: `out` is NULL or writable (# Safety).
        unsafe { give_new_value(out, value) }
    };
    // SAFETY: `error` is NULL or writable (# Safety).
    unsafe { call(error, body) }
}

/// The outputs of the selected rules, in rule order, as a new list of maps,
/// freed with `cr_value_free`.
///
/// # Safety
///
/// `decision` is `NULL` or a live `cr_decision`; `out` is `NULL` or points to a
/// writable `cr_value *`; `error` is `NULL` or points to a writable
/// `cr_error *` set to `NULL`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cr_decision_outputs_value(
    decision: *const cr_decision,
    out: *mut *mut cr_value,
    error: *mut *mut cr_error,
) -> cr_status {
    let body = || {
        // SAFETY: `decision` is NULL or live (# Safety).
        let decision = unsafe { handle(decision, "decision") }?;
        // SAFETY: `out` is NULL or writable (# Safety).
        unsafe { give_new_value(out, decision.outputs()) }
    };
    // SAFETY: `error` is NULL or writable (# Safety).
    unsafe { call(error, body) }
}

/// The name of the rule at position `rule`, borrowed from the decision and not
/// NUL-terminated.
///
/// # Safety
///
/// `decision` is `NULL` or a live `cr_decision`; `name` and `len` are `NULL` or
/// point to a writable `const char *` and `size_t`; `error` is `NULL` or points
/// to a writable `cr_error *` set to `NULL`. The name lives as long as the
/// decision.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cr_decision_rule_name(
    decision: *const cr_decision,
    rule: usize,
    name: *mut *const c_char,
    len: *mut usize,
    error: *mut *mut cr_error,
) -> cr_status {
    let body = || {
        // SAFETY: `name` and `len` are NULL or writable (# Safety).
        unsafe { clear_borrowed(name, len) };
        // SAFETY: `decision` is NULL or live (# Safety).
        let decision = unsafe { handle(decision, "decision") }?;
        let found = &decision.ruleset.rules()[decision.rule(rule)?];
        // SAFETY: `name` and `len` are NULL or writable (# Safety).
        unsafe { borrow_text(Some(found.name()), name, len) };
        Ok(())
    };
    // SAFETY: `error` is NULL or writable (# Safety).
    unsafe { call(error, body) }
}

/// The output of the rule at position `rule`, as a new map freed with
/// `cr_value_free`: its payload's keys and values, when its payload was
/// evaluated (`spec/rules.md`, *When payloads are evaluated*), and a plain
/// null otherwise.
///
/// # Safety
///
/// `decision` is `NULL` or a live `cr_decision`; `out` is `NULL` or points to a
/// writable `cr_value *`; `error` is `NULL` or points to a writable
/// `cr_error *` set to `NULL`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cr_decision_rule_output(
    decision: *const cr_decision,
    rule: usize,
    out: *mut *mut cr_value,
    error: *mut *mut cr_error,
) -> cr_status {
    let body = || {
        // SAFETY: `out` is NULL or writable (# Safety).
        unsafe { clear(out, "out") }?;
        // SAFETY: `decision` is NULL or live (# Safety).
        let decision = unsafe { handle(decision, "decision") }?;
        let value = match &decision.facts.rule_outputs[decision.rule(rule)?] {
            None => Value::Null(cleanrules::NullCause::Null),
            Some(pairs) => {
                let mut map = Map::new();
                for (key, value) in pairs {
                    map.insert(key.clone(), value.clone());
                }
                Value::Map(map)
            }
        };
        // SAFETY: `out` is NULL or writable (# Safety).
        unsafe { give_new_value(out, value) }
    };
    // SAFETY: `error` is NULL or writable (# Safety).
    unsafe { call(error, body) }
}

/// Check that an input nests no deeper than the limits allow, as JSON input
/// does (`spec/json-format.md`, *Errors and limits*).
pub(crate) fn check_input(input: &Value, limits: &Limits) -> Outcome {
    let depth = nesting(input);
    if depth > limits.max_nesting {
        return Err(cleanrules::Error::new(
            cleanrules::ErrorCode::LimitExceeded,
            format!(
                "the input nests lists and maps {depth} deep, more than the limit of {}",
                limits.max_nesting
            ),
        )
        .with_hint("raise max_nesting if the input is trusted")
        .into());
    }
    Ok(())
}
