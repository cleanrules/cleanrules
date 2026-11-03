//! Running actions and evaluating batches (`spec/rules.md`, *Actions* and
//! *Evaluation*).

use std::ffi::{c_char, c_void};
use std::sync::Arc;

use cleanrules::{Error, ErrorCode, Value};

use crate::boundary::{
    Failure, bytes, call, clear, free, give, give_boxed, handle, handle_mut, limits_of, utf8_arg,
};
use crate::cr_status;
use crate::decision::{check_input, cr_decision};
use crate::error::cr_error;
use crate::limits::cr_limits;
use crate::ruleset::cr_ruleset;
use crate::value::{borrow_value, cr_value};

/// An action: called by `cr_ruleset_run` for a selected rule that names it,
/// with the data it was registered with and the call (`cr_action_call_*`). It
/// returns 0 when it succeeds; any other number stops the run with
/// `CR_ACTION_ERROR`, after `cr_action_call_fail` to say why. It may read the
/// decision and the input, and use the rule set, even to run it again; it must
/// not change or free the registry being run, nor the decision or the input,
/// and must not unwind or jump out of the call.
pub type cr_action =
    Option<unsafe extern "C" fn(data: *mut c_void, call: *mut cr_action_call) -> i32>;

type Action = unsafe extern "C" fn(*mut c_void, *mut cr_action_call) -> i32;

/// One call of an action: the decision, the rule and the input, valid during
/// the call only, and the reason the action gives when it fails.
pub struct cr_action_call {
    decision: *const cr_decision,
    rule: usize,
    input: *const cr_value,
    failure: Option<String>,
}

/// The decision being run: the same handle that `cr_ruleset_run` then gives
/// through `out`, or through `cr_error_decision` when an action fails.
///
/// # Safety
///
/// `call` is `NULL` or the call an action received, during that call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cr_action_call_decision(
    call: *const cr_action_call,
) -> *const cr_decision {
    // SAFETY: the caller promises a live call when it is not null.
    unsafe { call.as_ref() }.map_or(std::ptr::null(), |c| c.decision)
}

/// The position of the rule whose action is called; 0 for `NULL`.
///
/// # Safety
///
/// `call` is `NULL` or the call an action received, during that call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cr_action_call_rule(call: *const cr_action_call) -> usize {
    // SAFETY: the caller promises a live call when it is not null.
    unsafe { call.as_ref() }.map_or(0, |c| c.rule)
}

/// The input, as the decision keeps it: exactly what was evaluated. Borrowed
/// from the decision; never free or change it.
///
/// # Safety
///
/// `call` is `NULL` or the call an action received, during that call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cr_action_call_input(call: *const cr_action_call) -> *const cr_value {
    // SAFETY: the caller promises a live call when it is not null.
    unsafe { call.as_ref() }.map_or(std::ptr::null(), |c| c.input)
}

/// Say why the action fails, before it returns a number other than 0: the
/// message (UTF-8; invalid bytes are replaced) ends the error's message and is
/// its `cause` (`spec/errors.md`, *JSON form*). A later call replaces it.
///
/// # Safety
///
/// `call` is `NULL` or the call an action received, during that call;
/// `message` is `NULL` or points to `len` readable bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cr_action_call_fail(
    call: *mut cr_action_call,
    message: *const c_char,
    len: usize,
) {
    // SAFETY: the caller promises a live call when it is not null.
    let Some(call) = (unsafe { call.as_mut() }) else {
        return;
    };
    // SAFETY: `message` and `len` (# Safety).
    if let Ok(text) = unsafe { bytes(message, len, "message") } {
        call.failure = Some(String::from_utf8_lossy(text).into_owned());
    }
}

/// What an action said when it failed: the cause of the `action_error`.
#[derive(Debug)]
struct Cause(String);

impl std::fmt::Display for Cause {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Cause {}

/// An action registry (`spec/rules.md`, *Actions*): names mapped to actions.
/// It changes while it is built (`cr_actions_add`); the data pointers are the
/// program's, and the library only passes them back to their actions.
pub struct cr_actions {
    entries: Vec<(String, Action, *mut c_void)>,
}

impl cr_actions {
    fn names(&self) -> Vec<&str> {
        self.entries
            .iter()
            .map(|(name, ..)| name.as_str())
            .collect()
    }

    fn get(&self, name: &str) -> Option<(Action, *mut c_void)> {
        self.entries
            .iter()
            .find(|(n, ..)| n == name)
            .map(|&(_, action, data)| (action, data))
    }
}

/// A new, empty action registry. Free it with `cr_actions_free`.
#[unsafe(no_mangle)]
pub extern "C" fn cr_actions_new() -> *mut cr_actions {
    Box::into_raw(Box::new(cr_actions {
        entries: Vec::new(),
    }))
}

/// Register `action` under `name`, with `data`, which is passed back to it.
/// The registry keeps both until it is freed. A name is not empty, has no
/// structure and is registered once (`CR_INVALID_ARGUMENT` otherwise).
///
/// # Safety
///
/// `actions` is `NULL` or a live `cr_actions` that no other thread uses during
/// the call; `name` is `NULL` or points to `len` readable bytes; `action` is
/// `NULL` or a function of the type `cr_action`, which stays callable with
/// `data` as long as the registry is used; `error` is `NULL` or points to a
/// writable `cr_error *` set to `NULL`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cr_actions_add(
    actions: *mut cr_actions,
    name: *const c_char,
    len: usize,
    action: cr_action,
    data: *mut c_void,
    error: *mut *mut cr_error,
) -> cr_status {
    let body = || {
        // SAFETY: `actions` is NULL or live and used by this call only.
        let actions = unsafe { handle_mut(actions, "actions") }?;
        // SAFETY: `name` and `len` (# Safety).
        let name = utf8_arg(unsafe { bytes(name, len, "name") }?, "name")?;
        if name.is_empty() {
            return Err(Failure::Invalid(
                "the name is empty".into(),
                "rules name their actions with non-empty strings",
            ));
        }
        let Some(action) = action else {
            return Err(Failure::Invalid(
                "action is NULL".into(),
                "pass the function to call",
            ));
        };
        if actions.get(name).is_some() {
            return Err(Failure::Invalid(
                format!("the action {name:?} is registered already"),
                "register each name once",
            ));
        }
        actions.entries.push((name.to_owned(), action, data));
        Ok(())
    };
    // SAFETY: `error` is NULL or writable (# Safety).
    unsafe { call("cr_actions_add", error, body) }
}

/// Free an action registry. `NULL` is ignored. The data it holds stays the
/// program's.
///
/// # Safety
///
/// `actions` is `NULL` or a `cr_actions` from this library, not freed before,
/// and not being run.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cr_actions_free(actions: *mut cr_actions) {
    // SAFETY: the caller's promise is `free`'s.
    unsafe { free(actions) };
}

/// The names of a registry; none for `NULL`.
///
/// # Safety
///
/// `actions` is `NULL` or a live `cr_actions`.
unsafe fn registry<'a>(actions: *const cr_actions) -> Option<&'a cr_actions> {
    // SAFETY: the caller promises a live registry when it is not null.
    unsafe { actions.as_ref() }
}

/// Check that every action the rule set names is registered, in rule order
/// (`spec/rules.md`, *Actions*): the first that is not is a
/// `CR_REGISTRY_ERROR` with the rule, the name and the registered names. `NULL`
/// is an empty registry. This is the check of Python's `RuleSet.bind`, which
/// other bindings may call `bind` too.
///
/// # Safety
///
/// `ruleset` is `NULL` or a live `cr_ruleset`; `actions` is `NULL` or a live
/// `cr_actions`; `error` is `NULL` or points to a writable `cr_error *` set to
/// `NULL`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cr_ruleset_check_actions(
    ruleset: *const cr_ruleset,
    actions: *const cr_actions,
    error: *mut *mut cr_error,
) -> cr_status {
    let body = || {
        // SAFETY: `ruleset` is NULL or live (# Safety).
        let ruleset = &unsafe { handle(ruleset, "ruleset") }?.ruleset;
        // SAFETY: `actions` is NULL or live (# Safety).
        let names = unsafe { registry(actions) }
            .map(cr_actions::names)
            .unwrap_or_default();
        ruleset.check_action_names(&names)?;
        Ok(())
    };
    // SAFETY: `error` is NULL or writable (# Safety).
    unsafe { call("cr_ruleset_check_actions", error, body) }
}

/// Evaluate the rule set on an input, then call the actions of the selected
/// rules, in rule order (`spec/rules.md`, *Actions*). Every action the rule set
/// names must be registered, which is checked before evaluating, so nothing
/// runs otherwise (`CR_REGISTRY_ERROR`); a selected rule without an action is
/// skipped. The first action that fails stops the run with `CR_ACTION_ERROR`,
/// whose context has the rule and the rules whose actions had run
/// (`executed`), whose cause is what the action gave `cr_action_call_fail`, and
/// whose decision `cr_error_decision` gives: the same handle the actions saw.
/// The actions run on the calling thread.
///
/// # Safety
///
/// `ruleset` is `NULL` or a live `cr_ruleset`; `actions` is `NULL` or a live
/// `cr_actions`, whose actions may be called with their data, and which stays
/// unchanged during the run; `input` is `NULL` or a live `cr_value`; `limits`
/// is `NULL` or a live `cr_limits`; `out` is `NULL` or points to a writable
/// `cr_decision *`; `error` is `NULL` or points to a writable `cr_error *` set
/// to `NULL`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cr_ruleset_run(
    ruleset: *const cr_ruleset,
    actions: *const cr_actions,
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
        // SAFETY: `actions` is NULL or live (# Safety).
        let registry = unsafe { registry(actions) };
        let names = registry.map(cr_actions::names).unwrap_or_default();
        ruleset.check_action_names(&names)?;
        // SAFETY: `limits` is NULL or live (# Safety).
        let limits = unsafe { limits_of(limits) };
        // SAFETY: `input` is NULL or live (# Safety).
        let value = unsafe { borrow_value(input, "input") }?;
        check_input(value, &limits)?;
        // Boxed before any action runs: the actions see the handle the caller
        // gets, through `out` or the error.
        let decision = Box::into_raw(Box::new(cr_decision::evaluated(
            ruleset,
            value.clone(),
            limits,
        )?));
        // SAFETY: `decision` was just boxed, and is only read until it is boxed
        // again below.
        let failure = unsafe { run_actions(&*decision, decision, registry) };
        // SAFETY: the box made above, taken back once.
        let decision = unsafe { Box::from_raw(decision) };
        if let Some(error) = failure {
            return Err(Failure::Action(error, decision));
        }
        // SAFETY: `out` passed `clear`.
        unsafe { give_boxed(out, decision) };
        Ok(())
    };
    // SAFETY: `error` is NULL or writable (# Safety).
    unsafe { call("cr_ruleset_run", error, body) }
}

/// Call the actions of the selected rules; the error of the first that fails.
///
/// # Safety
///
/// `pointer` is the boxed `decision`; the registry's actions may be called with
/// their data.
unsafe fn run_actions(
    decision: &cr_decision,
    pointer: *const cr_decision,
    registry: Option<&cr_actions>,
) -> Option<Error> {
    let mut executed: Vec<String> = Vec::new();
    for &position in decision.selected() {
        let rule = &decision.ruleset().rules()[position];
        let Some(name) = rule.get_action_name() else {
            continue;
        };
        // Checked before: every named action is registered.
        let Some((action, data)) = registry.and_then(|r| r.get(name)) else {
            continue;
        };
        let mut call = cr_action_call {
            decision: pointer,
            rule: position,
            input: decision
                .input()
                .map_or(std::ptr::null(), |v| std::ptr::from_ref(v).cast()),
            failure: None,
        };
        // SAFETY: the program registered `action` with `data` and keeps them
        // callable (# Safety of `cr_actions_add`); the call, the decision and
        // its input live during the call.
        let code = unsafe { action(data, &raw mut call) };
        if code != 0 {
            let said = call
                .failure
                .as_deref()
                .map_or(String::new(), |m| format!(": {m}"));
            let mut failed = Error::new(
                ErrorCode::ActionError,
                format!(
                    "the action {name:?} of rule {:?} failed with {code}{said}",
                    rule.name()
                ),
            )
            .with_hint("later actions did not run; cr_error_decision gives the decision");
            if let Some(message) = call.failure {
                failed = failed.caused_by(Cause(message));
            }
            failed.context_mut().rule = Some(rule.name().to_owned());
            failed.context_mut().executed = executed;
            return Some(failed);
        }
        executed.push(rule.name().to_owned());
    }
    None
}

/// The decisions of a batch, one per input, in order.
pub struct cr_batch {
    decisions: Vec<cr_decision>,
}

/// Evaluate the rule set on each input of a list, in order (`spec/rules.md`,
/// *Evaluation*). The first input that fails gives the error, with its
/// position as `row`; anything but a list is a `CR_EVALUATION_ERROR`.
///
/// # Safety
///
/// `ruleset` is `NULL` or a live `cr_ruleset`; `inputs` is `NULL` or a live
/// `cr_value`; `limits` is `NULL` or a live `cr_limits`; `out` is `NULL` or
/// points to a writable `cr_batch *`; `error` is `NULL` or points to a writable
/// `cr_error *` set to `NULL`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cr_ruleset_evaluate_many(
    ruleset: *const cr_ruleset,
    inputs: *const cr_value,
    limits: *const cr_limits,
    out: *mut *mut cr_batch,
    error: *mut *mut cr_error,
) -> cr_status {
    let body = || {
        // SAFETY: `out` is NULL or writable (# Safety).
        unsafe { clear(out, "out") }?;
        // SAFETY: `ruleset` is NULL or live (# Safety).
        let ruleset = Arc::clone(&unsafe { handle(ruleset, "ruleset") }?.ruleset);
        // SAFETY: `limits` is NULL or live (# Safety).
        let limits = unsafe { limits_of(limits) };
        // SAFETY: `inputs` is NULL or live (# Safety).
        let Value::List(inputs) = unsafe { borrow_value(inputs, "inputs") }? else {
            return Err(
                Error::new(ErrorCode::EvaluationError, "a batch is a list of inputs")
                    .with_hint("make the inputs the items of a list")
                    .into(),
            );
        };
        // The first input that fails gives the error (`spec/errors.md`): one
        // nested too deep fails after those before it are evaluated.
        let deep = inputs
            .iter()
            .enumerate()
            .find_map(|(row, input)| check_input(input, &limits).err().map(|f| (row, f)));
        let usable = deep.as_ref().map_or(inputs.len(), |(row, _)| *row);
        let made = ruleset.evaluate_many(&inputs[..usable])?;
        if let Some((row, failure)) = deep {
            return Err(at_row(failure, row));
        }
        let decisions = made
            .iter()
            .zip(inputs)
            .map(|(decision, input)| {
                cr_decision::made(Arc::clone(&ruleset), input.clone(), limits, decision)
            })
            .collect();
        // SAFETY: `out` passed `clear`.
        unsafe { give(out, cr_batch { decisions }) };
        Ok(())
    };
    // SAFETY: `error` is NULL or writable (# Safety).
    unsafe { call("cr_ruleset_evaluate_many", error, body) }
}

/// A failure of the input at `row` of a batch.
fn at_row(failure: Failure, row: usize) -> Failure {
    match failure {
        Failure::Core(mut error) => {
            error.context_mut().row = u64::try_from(row).ok();
            Failure::Core(error)
        }
        other => other,
    }
}

/// The number of decisions of a batch; 0 for `NULL`.
///
/// # Safety
///
/// `batch` is `NULL` or a live `cr_batch`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cr_batch_len(batch: *const cr_batch) -> usize {
    // SAFETY: `batch` is NULL or live (# Safety).
    unsafe { batch.as_ref() }.map_or(0, |b| b.decisions.len())
}

/// The decision of the input at `index`, borrowed from the batch; `NULL` when
/// `index` is out of it.
///
/// # Safety
///
/// `batch` is `NULL` or a live `cr_batch`. The decision lives as long as the
/// batch and must never be freed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cr_batch_get(batch: *const cr_batch, index: usize) -> *const cr_decision {
    // SAFETY: `batch` is NULL or live (# Safety).
    unsafe { batch.as_ref() }
        .and_then(|b| b.decisions.get(index))
        .map_or(std::ptr::null(), std::ptr::from_ref)
}

/// Free a batch and its decisions. `NULL` is ignored.
///
/// # Safety
///
/// `batch` is `NULL` or a `cr_batch` from this library, not freed before.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cr_batch_free(batch: *mut cr_batch) {
    // SAFETY: the caller's promise is `free`'s.
    unsafe { free(batch) };
}
