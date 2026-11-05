//! Actions as C callbacks, single nodes, grids and batches through the C ABI.

// A test: a panic is the failure report.
#![allow(clippy::unwrap_used, clippy::panic)]

mod common;

use std::ffi::{CStr, c_void};
use std::ptr;

use cleanrules_ffi::*;
use common::{Actions, Value, arg, value_from_json, value_to_json};

const RULES: &str = "HIT POLICY rule_order
RULE vip
IF years > 5
THEN discount = 0.15
DO notify

RULE loyal
IF years > 2
THEN discount = 0.05
DO log

RULE quiet
IF years > 1";

/// What the actions saw: each call's action, rule position and rule name.
#[derive(Default)]
struct Calls {
    seen: Vec<(&'static str, usize, String, String)>,
    fail_on: Option<&'static str>,
}

thread_local! {
    /// The decision the last call saw on this thread (actions run on the
    /// thread of `cr_ruleset_run`), to compare with the one the run gives.
    static SEEN: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// The action registered as `name`: records the call and fails when told to.
unsafe extern "C" fn record(data: *mut c_void, call: *mut cr_action_call) -> i32 {
    // SAFETY: `data` is the `(name, Calls)` pair the test registered, alive
    // during the run, which only this thread uses.
    let (name, calls) = unsafe { &mut *data.cast::<(&'static str, *mut Calls)>() };
    // SAFETY: as above.
    let calls = unsafe { &mut **calls };
    // SAFETY: the call is live during the action.
    let (decision, rule, input) = unsafe {
        (
            cr_action_call_decision(call),
            cr_action_call_rule(call),
            cr_action_call_input(call),
        )
    };
    SEEN.set(decision as usize);
    let (mut text, mut len) = (ptr::null(), 0);
    // SAFETY: the decision lives during the call; its rule names are borrowed.
    let status = unsafe {
        cr_decision_rule_name(decision, rule, &raw mut text, &raw mut len, ptr::null_mut())
    };
    assert_eq!(status, CR_OK);
    // SAFETY: `len` bytes borrowed from the decision.
    let rule_name =
        String::from_utf8(unsafe { std::slice::from_raw_parts(text.cast::<u8>(), len) }.to_vec())
            .unwrap();
    let mut output = ptr::null_mut();
    // SAFETY: the decision lives during the call.
    let status =
        unsafe { cr_decision_rule_output(decision, rule, &raw mut output, ptr::null_mut()) };
    assert_eq!(status, CR_OK);
    let output = Value(output);
    assert_eq!(value_to_json(input), r#"{"years":7}"#);
    calls
        .seen
        .push((name, rule, rule_name, value_to_json(output.0)));
    if calls.fail_on == Some(*name) {
        let message = "the log is full";
        // SAFETY: the call is live; the message outlives the call.
        unsafe { cr_action_call_fail(call, message.as_ptr().cast(), message.len()) };
        return 3;
    }
    0
}

/// A registry of `notify` and `log`, both recording into `calls`.
// Boxed: the registry keeps pointers to each entry, which must not move.
#[allow(clippy::vec_box)]
fn registry(calls: *mut Calls, data: &mut Vec<Box<(&'static str, *mut Calls)>>) -> Actions {
    let actions = Actions(cr_actions_new());
    for name in ["notify", "log"] {
        data.push(Box::new((name, calls)));
        let pointer = ptr::from_mut(data.last_mut().unwrap().as_mut()).cast::<c_void>();
        let (p, n) = arg(name.as_bytes());
        // SAFETY: a live registry; the data outlives every run below.
        let status =
            unsafe { cr_actions_add(actions.0, p, n, Some(record), pointer, ptr::null_mut()) };
        assert_eq!(status, CR_OK);
    }
    actions
}

fn run(
    ruleset: &common::RuleSet,
    actions: Option<&Actions>,
    input: &Value,
) -> Result<common::Decision, common::Failure> {
    let actions = actions.map_or(ptr::null(), |a| a.0.cast_const());
    // SAFETY: live handles.
    common::handle(|out, error| unsafe {
        cr_ruleset_run(ruleset.0, actions, input.0, ptr::null(), out, error)
    })
    .map(common::Decision)
}

#[test]
fn actions_run_for_the_selected_rules_in_rule_order() {
    let ruleset = common::read_ruleset(RULES.as_bytes(), CR_FORMAT_TEXT).unwrap();
    // One raw pointer from start to end: the actions write through it.
    let calls = Box::into_raw(Box::<Calls>::default());
    let mut data = Vec::new();
    let actions = registry(calls, &mut data);
    let input = value_from_json(r#"{"years":7}"#).unwrap();
    let decision = run(&ruleset, Some(&actions), &input).unwrap();
    // SAFETY: no action runs while the record is read.
    let seen = unsafe { (*calls).seen.as_slice() };
    assert_eq!(
        seen,
        [
            (
                "notify",
                0,
                "vip".into(),
                r#"{"discount":{"$decimal":"0.15"}}"#.into()
            ),
            (
                "log",
                1,
                "loyal".into(),
                r#"{"discount":{"$decimal":"0.05"}}"#.into()
            ),
        ]
    );
    // SAFETY: a live decision; `quiet` fired without an action.
    assert_eq!(unsafe { cr_decision_selected_len(decision.0) }, 3);
    // SAFETY: `evaluate` never runs actions.
    let checked = common::ok_or(|e| unsafe { cr_ruleset_check_actions(ruleset.0, actions.0, e) });
    assert!(checked.is_ok());
    // SAFETY: no action runs while the record is changed or read.
    unsafe { (*calls).seen.clear() };
    common::evaluate_value(&ruleset, &input).unwrap();
    // SAFETY: as above; the record is freed once, after the last run.
    let calls = unsafe { Box::from_raw(calls) };
    assert_eq!(calls.seen, []);
}

#[test]
fn the_first_failing_action_stops_the_run() {
    let ruleset = common::read_ruleset(RULES.as_bytes(), CR_FORMAT_TEXT).unwrap();
    let calls = Box::into_raw(Box::new(Calls {
        fail_on: Some("log"),
        ..Calls::default()
    }));
    let mut data = Vec::new();
    let actions = registry(calls, &mut data);
    let input = value_from_json(r#"{"years":7}"#).unwrap();
    let mut out = ptr::null_mut();
    let mut error = ptr::null_mut();
    // SAFETY: live handles and writable out-parameters.
    let status = unsafe {
        cr_ruleset_run(
            ruleset.0,
            actions.0,
            input.0,
            ptr::null(),
            &raw mut out,
            &raw mut error,
        )
    };
    assert_eq!(status, CR_ACTION_ERROR);
    assert!(out.is_null());
    // SAFETY: the error is live; its decision is borrowed from it.
    unsafe {
        let decision = cr_error_decision(error);
        assert!(!decision.is_null());
        assert_eq!(cr_decision_selected_len(decision), 3);
        assert_eq!(decision as usize, SEEN.get());
        let json = CStr::from_ptr(cr_error_json(error)).to_str().unwrap();
        assert!(json.contains(r#""cause":"the log is full""#), "{json}");
        assert!(json.contains(r#""rule":"loyal""#), "{json}");
        assert!(json.contains(r#""executed":["vip"]"#), "{json}");
        cr_error_free(error);
        assert!(cr_error_decision(ptr::null()).is_null());
    }
    // SAFETY: the run is over; the record is freed once.
    let calls = unsafe { Box::from_raw(calls) };
    assert_eq!(calls.seen.len(), 2);
}

#[test]
fn unregistered_actions_stop_the_run_before_it_starts() {
    let ruleset = common::read_ruleset(RULES.as_bytes(), CR_FORMAT_TEXT).unwrap();
    let input = value_from_json(r#"{"years":7}"#).unwrap();
    let failure = run(&ruleset, None, &input).err().unwrap();
    assert_eq!(failure.status, CR_REGISTRY_ERROR);
    // SAFETY: a live rule set; no registry is an empty one.
    let failure = common::ok_or(|e| unsafe { cr_ruleset_check_actions(ruleset.0, ptr::null(), e) })
        .err()
        .unwrap();
    assert_eq!(failure.status, CR_REGISTRY_ERROR);
    // A strict rule set that cannot read its input fails before any action.
    let plain = common::read_ruleset(b"RULE a\nIF x > 1", CR_FORMAT_TEXT).unwrap();
    assert!(run(&plain, None, &input).is_ok());
}

#[test]
fn a_registry_refuses_what_it_cannot_call() {
    let actions = Actions(cr_actions_new());
    // SAFETY: a live registry; the arguments are wrong on purpose.
    unsafe {
        assert_eq!(
            cr_actions_add(
                actions.0,
                c"a".as_ptr(),
                1,
                None,
                ptr::null_mut(),
                ptr::null_mut()
            ),
            CR_INVALID_ARGUMENT
        );
        assert_eq!(
            cr_actions_add(
                actions.0,
                c"a".as_ptr(),
                1,
                Some(record),
                ptr::null_mut(),
                ptr::null_mut()
            ),
            CR_OK
        );
        assert_eq!(
            cr_actions_add(
                actions.0,
                c"a".as_ptr(),
                1,
                Some(record),
                ptr::null_mut(),
                ptr::null_mut()
            ),
            CR_INVALID_ARGUMENT
        );
        assert_eq!(
            cr_actions_add(
                actions.0,
                c"\xff".as_ptr(),
                1,
                Some(record),
                ptr::null_mut(),
                ptr::null_mut()
            ),
            CR_FORMAT_ERROR
        );
        assert_eq!(
            cr_actions_add(
                ptr::null_mut(),
                c"b".as_ptr(),
                1,
                Some(record),
                ptr::null_mut(),
                ptr::null_mut()
            ),
            CR_INVALID_ARGUMENT
        );
        cr_actions_free(ptr::null_mut());
    }
}

#[test]
fn a_node_is_read_written_and_evaluated() {
    let node = common::read_node("years > 5 AND tier = 'gold'", CR_FORMAT_TEXT).unwrap();
    // SAFETY: a live node.
    let text =
        common::text(|o, l, e| unsafe { cr_node_write(node.0, CR_FORMAT_TEXT, o, l, e) }).unwrap();
    assert_eq!(text, r#"years > 5 AND tier = "gold""#);
    // SAFETY: as above.
    let json =
        common::text(|o, l, e| unsafe { cr_node_write(node.0, CR_FORMAT_JSON, o, l, e) }).unwrap();
    let again = common::read_node(&json, CR_FORMAT_JSON).unwrap();
    let input = value_from_json(r#"{"years": 7, "tier": "gold"}"#).unwrap();
    let result = common::evaluate_node(&again, &input, None, false, false).unwrap();
    assert!(
        result.starts_with(r#"{"op":"and","truth":true"#),
        "{result}"
    );
    // The paths it reads, in preorder and each once.
    let twice = common::read_node("a.b[1] > 5 OR (x = 1 AND a.b[1] < 2)", CR_FORMAT_TEXT).unwrap();
    // SAFETY: a live node.
    let paths =
        Value(common::handle(|o, e| unsafe { cr_node_field_paths(twice.0, o, e) }).unwrap());
    assert_eq!(value_to_json(paths.0), r#"[["a","b",1],["x"]]"#);
    // SAFETY: no node: an invalid argument, and the out-parameter is NULL.
    unsafe {
        let mut out = cr_value_new_null();
        let previous = out;
        assert_eq!(
            cr_node_field_paths(ptr::null(), &raw mut out, ptr::null_mut()),
            CR_INVALID_ARGUMENT
        );
        assert!(out.is_null());
        cr_value_free(previous);
    }
    // A value node is evaluated as a value, not as a condition.
    let sum = common::read_node("years * 2", CR_FORMAT_TEXT).unwrap();
    assert_eq!(
        common::evaluate_node(&sum, &input, Some("boolean"), false, false)
            .err()
            .unwrap()
            .status,
        CR_EXPRESSION_ERROR
    );
    let result = common::evaluate_node(&sum, &input, None, false, true).unwrap();
    assert!(result.starts_with(r#"{"op":"mul","value":14"#), "{result}");
    // Strict mode, and an unknown logic.
    let empty = value_from_json("{}").unwrap();
    assert_eq!(
        common::evaluate_node(&again, &empty, None, true, false)
            .err()
            .unwrap()
            .status,
        CR_MISSING_FIELD
    );
    assert_eq!(
        common::evaluate_node(&again, &input, Some("fuzzy"), false, false)
            .err()
            .unwrap()
            .status,
        CR_REGISTRY_ERROR
    );
    // SAFETY: as above; CSV holds no nodes.
    let failure = common::text(|o, l, e| unsafe { cr_node_write(node.0, CR_FORMAT_CSV, o, l, e) });
    assert_eq!(failure.err().unwrap().status, CR_UNSUPPORTED);
    assert_eq!(
        common::read_node("years >", CR_FORMAT_TEXT)
            .err()
            .unwrap()
            .status,
        CR_RULE_SYNTAX_ERROR
    );
}

#[test]
fn a_grid_builds_a_table() {
    let rows = [
        r#"["new", "< 1", "'high'"]"#.to_owned(),
        r#"["old", "-", "'low'"]"#.to_owned(),
    ];
    let table = common::grid(
        [Some("priority"), None, None],
        false,
        r#"["RULE", "years", "THEN risk IN ('high', 'low')"]"#,
        &rows,
        None,
    )
    .unwrap();
    assert_eq!(common::kind(&table), CR_DOCUMENT_DECISION_TABLE);
    let csv = common::write_document(&table, CR_FORMAT_CSV).unwrap();
    assert_eq!(
        csv,
        "HIT POLICY,priority\nRULE,years,\"THEN risk IN ('high', 'low')\"\nnew,< 1,'high'\nold,-,'low'\n"
    );
    let ruleset = common::ruleset_of(&table).unwrap();
    let decision = common::evaluate(&ruleset, br#"{"years": 0}"#).unwrap();
    assert_eq!(common::output(&decision).unwrap(), r#"{"risk":"high"}"#);
    // Errors point into the grid; settings and headers are checked first.
    let failure = common::grid([Some("last"), None, None], false, r#"["x"]"#, &[], None)
        .err()
        .unwrap();
    assert_eq!(failure.status, CR_RULE_DEFINITION_ERROR);
    let failure = common::grid([None, None, None], false, r#"["x", 1]"#, &[], None)
        .err()
        .unwrap();
    assert_eq!(failure.status, CR_INVALID_ARGUMENT);
    let failure = common::grid([None, None, None], false, r#""x""#, &[], None)
        .err()
        .unwrap();
    assert_eq!(failure.status, CR_INVALID_ARGUMENT);
    let failure = common::grid(
        [None, None, None],
        false,
        r#"["x"]"#,
        &[r#"["> )"]"#.to_owned()],
        None,
    )
    .err()
    .unwrap();
    assert_eq!(failure.status, CR_RULE_SYNTAX_ERROR);
    // SAFETY: a NULL grid is refused; freeing NULL is ignored.
    unsafe {
        let mut out = ptr::null_mut();
        assert_eq!(
            cr_grid_finish(ptr::null_mut(), &raw mut out, ptr::null_mut()),
            CR_INVALID_ARGUMENT
        );
        cr_grid_free(ptr::null_mut());
    }
}

#[test]
fn a_batch_has_one_decision_per_input() {
    let ruleset = common::read_ruleset(RULES.as_bytes(), CR_FORMAT_TEXT).unwrap();
    let inputs = value_from_json(r#"[{"years": 7}, {"years": 0}]"#).unwrap();
    let batch = common::evaluate_many(&ruleset, &inputs).unwrap();
    // SAFETY: a live batch; its decisions are borrowed.
    unsafe {
        assert_eq!(cr_batch_len(batch.0), 2);
        assert_eq!(cr_decision_selected_len(cr_batch_get(batch.0, 0)), 3);
        assert_eq!(cr_decision_selected_len(cr_batch_get(batch.0, 1)), 0);
        assert!(cr_batch_get(batch.0, 2).is_null());
        assert_eq!(cr_batch_len(ptr::null()), 0);
    }
    let strict = common::read_ruleset(
        RULES
            .replace("rule_order\n", "rule_order\nSTRICT\n")
            .as_bytes(),
        CR_FORMAT_TEXT,
    )
    .unwrap();
    let failure =
        common::evaluate_many(&strict, &value_from_json(r#"[{"years": 1}, {}]"#).unwrap())
            .err()
            .unwrap();
    assert_eq!(failure.status, CR_MISSING_FIELD);
    assert!(cleanrules::json::to_string(&failure.json).contains(r#""row":1"#));
    let failure = common::evaluate_many(&ruleset, &value_from_json(r#"{"years": 1}"#).unwrap())
        .err()
        .unwrap();
    assert_eq!(failure.status, CR_EVALUATION_ERROR);
    let limits = common::limits(&[(CR_LIMIT_MAX_NESTING, 1)]);
    let deep = value_from_json(r#"[{"years": 1}, {"years": [1]}]"#).unwrap();
    // SAFETY: live handles.
    let failure = common::handle(|o, e| unsafe {
        cr_ruleset_evaluate_many(ruleset.0, deep.0, limits.0, o, e)
    });
    let failure = failure.err().unwrap();
    assert_eq!(failure.status, CR_LIMIT_EXCEEDED);
    assert!(cleanrules::json::to_string(&failure.json).contains(r#""row":1"#));
}

#[test]
fn the_actions_see_the_decision_the_run_gives() {
    let ruleset = common::read_ruleset(RULES.as_bytes(), CR_FORMAT_TEXT).unwrap();
    let calls = Box::into_raw(Box::<Calls>::default());
    let mut data = Vec::new();
    let actions = registry(calls, &mut data);
    let input = value_from_json(r#"{"years":7}"#).unwrap();
    let decision = run(&ruleset, Some(&actions), &input).unwrap();
    assert_eq!(decision.0 as usize, SEEN.get());
    // SAFETY: the run is over; the record is freed once.
    drop(unsafe { Box::from_raw(calls) });
}

#[test]
fn review_cases_are_refused() {
    // A logic named by an empty text, and an action named so.
    let node = common::read_node("a > 1", CR_FORMAT_TEXT).unwrap();
    let input = value_from_json("{}").unwrap();
    let failure = common::evaluate_node(&node, &input, Some(""), false, false)
        .err()
        .unwrap();
    assert_eq!(failure.status, CR_REGISTRY_ERROR);
    let actions = Actions(cr_actions_new());
    // SAFETY: a live registry; the empty name is refused.
    let status = unsafe {
        cr_actions_add(
            actions.0,
            c"".as_ptr(),
            0,
            Some(record),
            ptr::null_mut(),
            ptr::null_mut(),
        )
    };
    assert_eq!(status, CR_INVALID_ARGUMENT);
    // CSV holds no nodes.
    assert_eq!(
        common::read_node("a", CR_FORMAT_CSV).err().unwrap().status,
        CR_UNSUPPORTED
    );
    // NULL arguments are named.
    // SAFETY: the NULL header is refused before it is read.
    let failure = common::handle(|o, e| unsafe {
        cr_grid_new(
            ptr::null(),
            0,
            ptr::null(),
            0,
            ptr::null(),
            0,
            false,
            ptr::null(),
            ptr::null(),
            o,
            e,
        )
    });
    let json = cleanrules::json::to_string(&failure.err().unwrap().json);
    assert!(json.contains("cr_grid_new: header is NULL"), "{json}");
}

#[test]
fn a_grid_with_a_failed_row_cannot_be_finished() {
    let header = value_from_json(r#"["x"]"#).unwrap();
    // SAFETY: a live header.
    let grid = common::handle(|o, e| unsafe {
        cr_grid_new(
            ptr::null(),
            0,
            ptr::null(),
            0,
            ptr::null(),
            0,
            false,
            header.0,
            ptr::null(),
            o,
            e,
        )
    })
    .unwrap();
    let good = value_from_json(r#"["> 1"]"#).unwrap();
    let bad = value_from_json(r#"["> )"]"#).unwrap();
    // SAFETY: a live grid and live rows.
    unsafe {
        assert_eq!(cr_grid_push_row(grid, good.0, ptr::null_mut()), CR_OK);
        assert_eq!(
            cr_grid_push_row(grid, bad.0, ptr::null_mut()),
            CR_RULE_SYNTAX_ERROR
        );
        assert_eq!(
            cr_grid_push_row(grid, good.0, ptr::null_mut()),
            CR_INVALID_ARGUMENT
        );
        let mut out = ptr::null_mut();
        // The grid is consumed all the same.
        assert_eq!(
            cr_grid_finish(grid, &raw mut out, ptr::null_mut()),
            CR_INVALID_ARGUMENT
        );
        assert!(out.is_null());
    }
}

#[test]
fn a_batch_reports_the_first_input_that_fails() {
    let strict = common::read_ruleset(
        RULES
            .replace("rule_order\n", "rule_order\nSTRICT\n")
            .as_bytes(),
        CR_FORMAT_TEXT,
    )
    .unwrap();
    let limits = common::limits(&[(CR_LIMIT_MAX_NESTING, 1)]);
    let inputs = value_from_json(r#"[{}, {"years": [1]}]"#).unwrap();
    // SAFETY: live handles.
    let failure = common::handle(|o, e| unsafe {
        cr_ruleset_evaluate_many(strict.0, inputs.0, limits.0, o, e)
    });
    let failure = failure.err().unwrap();
    assert_eq!(failure.status, CR_MISSING_FIELD);
    assert!(cleanrules::json::to_string(&failure.json).contains(r#""row":0"#));
}

#[test]
fn a_borrowed_decision_is_kept_by_cloning_it() {
    let ruleset = common::read_ruleset(RULES.as_bytes(), CR_FORMAT_TEXT).unwrap();
    let inputs = value_from_json(r#"[{"years": 7}, {"years": 0}]"#).unwrap();
    let batch = common::evaluate_many(&ruleset, &inputs).unwrap();
    // SAFETY: the batch is live while its first decision is cloned.
    let copy = common::Decision(
        common::handle(|o, e| unsafe { cr_decision_clone(cr_batch_get(batch.0, 0), o, e) })
            .unwrap(),
    );
    let written = common::write_decision(&copy, CR_FORMAT_JSON).unwrap();
    drop(batch);
    drop(ruleset);
    // SAFETY: the copy owns itself and keeps the rule set alive.
    unsafe { assert_eq!(cr_decision_selected_len(copy.0), 3) };
    assert_eq!(
        common::write_decision(&copy, CR_FORMAT_JSON).unwrap(),
        written
    );
    // SAFETY: NULL is refused.
    let refused = common::handle(|o, e| unsafe { cr_decision_clone(ptr::null(), o, e) });
    let Err(refused) = refused else {
        panic!("a NULL decision was cloned");
    };
    assert_eq!(refused.code(), "invalid_argument");
}
