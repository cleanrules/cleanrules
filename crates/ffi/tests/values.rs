//! Native values through the C ABI: constructors, readers, ownership, and
//! decisions read without JSON.

// A test: a panic is the failure report.
#![allow(clippy::unwrap_used, clippy::panic)]

mod common;

use std::ptr;

use cleanrules_ffi::*;
use common::{Value, arg, value_from_json, value_to_json};

/// Make a value with a fallible constructor.
fn made(
    call: impl FnOnce(*mut *mut cr_value, *mut *mut cr_error) -> cr_status,
) -> Result<Value, common::Failure> {
    common::handle(call).map(Value)
}

fn decimal(text: &str) -> Value {
    let (p, n) = arg(text.as_bytes());
    // SAFETY: the text outlives the call.
    made(|o, e| unsafe { cr_value_new_decimal(p, n, o, e) }).unwrap()
}

fn string(text: &str) -> Value {
    let (p, n) = arg(text.as_bytes());
    // SAFETY: the text outlives the call.
    made(|o, e| unsafe { cr_value_new_string(p, n, o, e) }).unwrap()
}

fn date(year: i32, month: i32, day: i32) -> Value {
    // SAFETY: valid out-parameters.
    made(|o, e| unsafe { cr_value_new_date(year, month, day, o, e) }).unwrap()
}

fn aware(seconds: i64, offset: i32, zone: &str) -> Value {
    let (p, n) = arg(zone.as_bytes());
    // SAFETY: the zone outlives the call.
    made(|o, e| unsafe { cr_value_new_aware_datetime(seconds, 0, offset, p, n, o, e) }).unwrap()
}

#[test]
fn every_kind_is_built_and_written() {
    // SAFETY: valid out-parameters.
    let datetime =
        made(|o, e| unsafe { cr_value_new_naive_datetime(2026, 11, 3, 9, 30, 0, 250, o, e) });
    let (p, n) = arg(b"bytes");
    // SAFETY: the type name outlives the call.
    let opaque = made(|o, e| unsafe { cr_value_new_opaque(p, n, 42, o, e) });
    let cases: Vec<(Value, &str)> = vec![
        (Value(cr_value_new_null()), "null"),
        (Value(cr_value_new_bool(true)), "true"),
        (Value(cr_value_new_int(-7)), "-7"),
        (Value(cr_value_new_float(1.5)), "1.5"),
        (Value(cr_value_new_float(f64::NAN)), "null"),
        (decimal("0.10"), r#"{"$decimal":"0.10"}"#),
        (decimal("1e40"), "null"),
        (string("héllo"), r#""héllo""#),
        (date(2026, 11, 3), r#"{"$date":"2026-11-03"}"#),
        (
            datetime.unwrap(),
            r#"{"$datetime":"2026-11-03T09:30:00.000250"}"#,
        ),
        (
            aware(1_793_694_600, 3600, "Europe/Madrid"),
            r#"{"$datetime":"2026-11-03T09:30:00+01:00[Europe/Madrid]"}"#,
        ),
        (
            aware(1_793_694_600, 7200, "Europe/Madrid"),
            r#"{"$datetime":"2026-11-03T10:30:00+02:00"}"#,
        ),
        (aware(0, 0, ""), r#"{"$datetime":"1970-01-01T00:00:00Z"}"#),
        (opaque.unwrap(), r#"{"$opaque":"bytes"}"#),
    ];
    for (value, wanted) in cases {
        assert_eq!(value_to_json(value.0), wanted);
    }
    let overflow = decimal("1e40");
    let mut reason = 0;
    // SAFETY: a live value and a writable reason; no detail asked for.
    let status = unsafe {
        cr_value_get_null(
            overflow.0,
            &raw mut reason,
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
        )
    };
    assert_eq!((status, reason), (CR_OK, CR_REASON_DECIMAL_OVERFLOW));
}

#[test]
fn bad_arguments_are_refused() {
    let status = |r: Result<Value, common::Failure>| r.err().unwrap().status;
    // SAFETY: valid pointers; the arguments are wrong on purpose.
    unsafe {
        assert_eq!(
            status(made(|o, e| cr_value_new_decimal(c"1,5".as_ptr(), 3, o, e))),
            CR_FORMAT_ERROR
        );
        assert_eq!(
            status(made(|o, e| cr_value_new_string(c"\xff".as_ptr(), 1, o, e))),
            CR_FORMAT_ERROR
        );
        assert_eq!(
            status(made(|o, e| cr_value_new_date(2026, 2, 30, o, e))),
            CR_INVALID_ARGUMENT
        );
        assert_eq!(
            status(made(|o, e| cr_value_new_naive_datetime(
                2026, 1, 1, 24, 0, 0, 0, o, e
            ))),
            CR_INVALID_ARGUMENT
        );
        assert_eq!(
            status(made(|o, e| cr_value_new_naive_datetime(
                2026, 1, 1, 0, 0, 0, 1_000_000, o, e
            ))),
            CR_INVALID_ARGUMENT
        );
        assert_eq!(
            status(made(|o, e| cr_value_new_aware_datetime(
                0,
                0,
                19 * 3600,
                ptr::null(),
                0,
                o,
                e
            ))),
            CR_INVALID_ARGUMENT
        );
        assert_eq!(
            status(made(|o, e| cr_value_new_aware_datetime(
                0,
                -1,
                0,
                ptr::null(),
                0,
                o,
                e
            ))),
            CR_INVALID_ARGUMENT
        );
        assert_eq!(
            status(made(|o, e| cr_value_new_null_reason(
                9,
                ptr::null(),
                0,
                o,
                e
            ))),
            CR_INVALID_ARGUMENT
        );
    }
}

#[test]
fn nulls_keep_their_reason() {
    let detail = "getter raised";
    let (p, n) = arg(detail.as_bytes());
    // SAFETY: the detail outlives the call.
    let value =
        made(|o, e| unsafe { cr_value_new_null_reason(CR_REASON_READ_ERROR, p, n, o, e) }).unwrap();
    let (mut reason, mut text, mut len) = (0, ptr::null(), 0);
    // SAFETY: a live value and writable out-parameters.
    let status = unsafe {
        cr_value_get_null(
            value.0,
            &raw mut reason,
            &raw mut text,
            &raw mut len,
            ptr::null_mut(),
        )
    };
    assert_eq!((status, reason), (CR_OK, CR_REASON_READ_ERROR));
    // SAFETY: the detail is `len` bytes borrowed from the live value.
    let read = unsafe { std::slice::from_raw_parts(text.cast::<u8>(), len) };
    assert_eq!(read, detail.as_bytes());
    let plain = Value(cr_value_new_null());
    // SAFETY: a live value and a writable reason.
    let status = unsafe {
        cr_value_get_null(
            plain.0,
            &raw mut reason,
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
        )
    };
    assert_eq!((status, reason), (CR_OK, CR_REASON_NULL));
}

#[test]
fn lists_and_maps_are_built_from_their_children() {
    let list = Value(cr_value_new_list(3));
    let map = Value(cr_value_new_map());
    // SAFETY: owned values, moved into their parents once each.
    unsafe {
        assert_eq!(
            cr_value_list_set(list.0, 1, cr_value_new_int(5), ptr::null_mut()),
            CR_OK
        );
        // Out of the list: the item is freed all the same (Miri checks it).
        let mut error = ptr::null_mut();
        assert_eq!(
            cr_value_list_set(list.0, 3, cr_value_new_int(6), &raw mut error),
            CR_INVALID_ARGUMENT
        );
        common::take(error);
        assert_eq!(
            cr_value_map_insert(
                map.0,
                c"a".as_ptr(),
                1,
                cr_value_new_int(1),
                ptr::null_mut()
            ),
            CR_OK
        );
        assert_eq!(
            cr_value_map_insert(
                map.0,
                c"b".as_ptr(),
                1,
                cr_value_new_int(2),
                ptr::null_mut()
            ),
            CR_OK
        );
        assert_eq!(
            cr_value_map_insert(
                map.0,
                c"a".as_ptr(),
                1,
                cr_value_new_int(3),
                ptr::null_mut()
            ),
            CR_OK
        );
        // Into a value that is not a map: refused, and the item is freed.
        assert_eq!(
            cr_value_map_insert(
                list.0,
                c"a".as_ptr(),
                1,
                cr_value_new_int(4),
                ptr::null_mut()
            ),
            CR_INVALID_ARGUMENT
        );
        assert_eq!(
            cr_value_map_insert(map.0, c"a".as_ptr(), 1, ptr::null_mut(), ptr::null_mut()),
            CR_INVALID_ARGUMENT
        );
    }
    assert_eq!(value_to_json(list.0), "[null,5,null]");
    assert_eq!(value_to_json(map.0), r#"{"a":3,"b":2}"#);
    // SAFETY: live values; borrowed items are only read.
    unsafe {
        assert_eq!(cr_value_len(list.0), 3);
        assert_eq!(cr_value_kind(cr_value_list_get(list.0, 1)), CR_VALUE_INT);
        assert!(cr_value_list_get(list.0, 3).is_null());
        assert!(cr_value_list_get(map.0, 0).is_null());
        assert_eq!(
            cr_value_kind(cr_value_map_get(map.0, c"b".as_ptr(), 1)),
            CR_VALUE_INT
        );
        assert!(cr_value_map_get(map.0, c"z".as_ptr(), 1).is_null());
        assert!(cr_value_map_get(list.0, c"b".as_ptr(), 1).is_null());
        let mut error = ptr::null_mut();
        assert_eq!(
            cr_value_map_entry(
                map.0,
                2,
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                &raw mut error
            ),
            CR_INVALID_ARGUMENT
        );
        common::take(error);
        assert!(!cr_value_is_record(map.0));
        assert_eq!(cr_value_len(ptr::null()), 0);
        assert_eq!(cr_value_kind(ptr::null()), 0);
    }
}

#[test]
fn readers_check_the_kind() {
    let value = Value(cr_value_new_int(1));
    let mut b = false;
    let mut error = ptr::null_mut();
    // SAFETY: a live value and writable out-parameters.
    let status = unsafe { cr_value_get_bool(value.0, &raw mut b, &raw mut error) };
    assert_eq!(status, CR_INVALID_ARGUMENT);
    let failure = common::take(error);
    assert_eq!(failure.code(), "invalid_argument");
    let mut i = 0;
    // SAFETY: as above; a NULL out-parameter is refused.
    unsafe {
        assert_eq!(
            cr_value_get_int(value.0, &raw mut i, ptr::null_mut()),
            CR_OK
        );
        assert_eq!(
            cr_value_get_int(value.0, ptr::null_mut(), ptr::null_mut()),
            CR_INVALID_ARGUMENT
        );
        assert_eq!(
            cr_value_get_int(ptr::null(), &raw mut i, ptr::null_mut()),
            CR_INVALID_ARGUMENT
        );
    }
    assert_eq!(i, 1);
}

const RULES: &str = r#"{"version":1,"ruleset":{"hit_policy":"collect","rules":[
    {"name":"vip","when":{"op":"gt","left":{"op":"field","path":["customer","years"]},"right":{"op":"const","value":5}},
     "then":{"discount":{"op":"const","value":{"$decimal":"0.15"}},"badge":{"op":"field","path":["customer","badge"]}}},
    {"name":"first","when":{"op":"eq","left":{"op":"field","path":["items",0]},"right":{"op":"const","value":"book"}},
     "then":{"discount":{"op":"const","value":0}}},
    {"name":"blocked","when":{"op":"field","path":["customer","blocked"]}}]}}"#;

#[test]
fn a_rule_set_says_what_it_reads() {
    let ruleset = common::read_ruleset(RULES.as_bytes(), CR_FORMAT_JSON).unwrap();
    // SAFETY: a live rule set and writable out-parameters.
    unsafe {
        assert_eq!(cr_ruleset_len(ruleset.0), 3);
        assert_eq!(
            std::ffi::CStr::from_ptr(cr_ruleset_hit_policy(ruleset.0))
                .to_str()
                .unwrap(),
            "collect"
        );
        assert!(cr_ruleset_hit_policy(ptr::null()).is_null());
        assert_eq!(
            std::ffi::CStr::from_ptr(cr_ruleset_logic(ruleset.0))
                .to_str()
                .unwrap(),
            "boolean"
        );
        assert!(cr_ruleset_logic(ptr::null()).is_null());
        let (mut name, mut len) = (ptr::null(), 0);
        assert_eq!(
            cr_ruleset_rule_name(ruleset.0, 1, &raw mut name, &raw mut len, ptr::null_mut()),
            CR_OK
        );
        assert_eq!(std::slice::from_raw_parts(name.cast::<u8>(), len), b"first");
        assert_eq!(
            cr_ruleset_rule_name(ruleset.0, 3, &raw mut name, &raw mut len, ptr::null_mut()),
            CR_INVALID_ARGUMENT
        );
    }
    // SAFETY: a live rule set.
    let paths =
        Value(common::handle(|o, e| unsafe { cr_ruleset_field_paths(ruleset.0, o, e) }).unwrap());
    let paths = value_to_json(paths.0);
    assert_eq!(
        paths,
        r#"[["customer","years"],["customer","badge"],["items",0],["customer","blocked"]]"#
    );
}

#[test]
#[allow(clippy::too_many_lines)] // One native input, read back step by step.
fn a_decision_is_read_without_json() {
    let ruleset = common::read_ruleset(RULES.as_bytes(), CR_FORMAT_JSON).unwrap();
    // A native input: a record (its positions are read too), a token for an
    // object of the program, and a list with holes.
    let input = Value(cr_value_new_map());
    let customer = cr_value_new_record();
    let items = cr_value_new_list(2);
    // SAFETY: owned values, each moved once into its parent.
    unsafe {
        assert_eq!(
            cr_value_map_insert(
                customer,
                c"years".as_ptr(),
                5,
                cr_value_new_int(7),
                ptr::null_mut()
            ),
            CR_OK
        );
        let mut badge = ptr::null_mut();
        assert_eq!(
            cr_value_new_opaque(c"image".as_ptr(), 5, 99, &raw mut badge, ptr::null_mut()),
            CR_OK
        );
        assert_eq!(
            cr_value_map_insert(customer, c"badge".as_ptr(), 5, badge, ptr::null_mut()),
            CR_OK
        );
        let mut book = ptr::null_mut();
        assert_eq!(
            cr_value_new_string(c"book".as_ptr(), 4, &raw mut book, ptr::null_mut()),
            CR_OK
        );
        assert_eq!(cr_value_list_set(items, 0, book, ptr::null_mut()), CR_OK);
        assert_eq!(
            cr_value_map_insert(input.0, c"customer".as_ptr(), 8, customer, ptr::null_mut()),
            CR_OK
        );
        assert_eq!(
            cr_value_map_insert(input.0, c"items".as_ptr(), 5, items, ptr::null_mut()),
            CR_OK
        );
    }
    let decision = common::evaluate_value(&ruleset, &input).unwrap();
    drop(input); // The decision keeps its own copy.
    // SAFETY: a live decision and writable out-parameters.
    unsafe {
        assert_eq!(cr_decision_len(decision.0), 3);
        let (mut fired, mut truth) = (false, 0);
        for (rule, wanted_fired, wanted_truth) in [
            (0, true, CR_TRUTH_TRUE),
            (1, true, CR_TRUTH_TRUE),
            (2, false, CR_TRUTH_UNKNOWN),
        ] {
            assert_eq!(
                cr_decision_fired(decision.0, rule, &raw mut fired, ptr::null_mut()),
                CR_OK
            );
            assert_eq!(
                cr_decision_truth(decision.0, rule, &raw mut truth, ptr::null_mut()),
                CR_OK
            );
            assert_eq!((fired, truth), (wanted_fired, wanted_truth), "rule {rule}");
        }
        assert_eq!(
            cr_decision_fired(decision.0, 3, &raw mut fired, ptr::null_mut()),
            CR_INVALID_ARGUMENT
        );
        assert_eq!(cr_decision_selected_len(decision.0), 2);
        let mut rule = 9;
        assert_eq!(
            cr_decision_selected(decision.0, 1, &raw mut rule, ptr::null_mut()),
            CR_OK
        );
        assert_eq!(rule, 1);
        assert_eq!(
            cr_decision_selected(decision.0, 2, &raw mut rule, ptr::null_mut()),
            CR_INVALID_ARGUMENT
        );
        // Several outputs: the single one is an error, the list is not.
        let mut error = ptr::null_mut();
        let mut output = ptr::null_mut();
        assert_eq!(
            cr_decision_output_value(decision.0, &raw mut output, &raw mut error),
            CR_HIT_POLICY_ERROR
        );
        assert!(output.is_null());
        common::take(error);
        assert_eq!(
            cr_decision_outputs_value(decision.0, &raw mut output, ptr::null_mut()),
            CR_OK
        );
        let outputs = Value(output);
        let first = cr_value_list_get(outputs.0, 0);
        let badge = cr_value_map_get(first, c"badge".as_ptr(), 5);
        let mut token = 0;
        assert_eq!(
            cr_value_get_opaque_token(badge, &raw mut token, ptr::null_mut()),
            CR_OK
        );
        assert_eq!(token, 99);
        assert_eq!(
            value_to_json(outputs.0),
            r#"[{"discount":{"$decimal":"0.15"},"badge":{"$opaque":"image"}},{"discount":0}]"#
        );
    }
    // The same in JSON.
    assert_eq!(
        common::outputs(&decision).unwrap(),
        r#"[{"discount":{"$decimal":"0.15"},"badge":{"$opaque":"image"}},{"discount":0}]"#
    );
}

#[test]
fn a_single_output_is_a_value() {
    let first = RULES.replace(r#""hit_policy":"collect""#, r#""hit_policy":"first""#);
    let ruleset = common::read_ruleset(first.as_bytes(), CR_FORMAT_JSON).unwrap();
    let input = value_from_json(r#"{"customer": {"years": 1}, "items": []}"#).unwrap();
    let decision = common::evaluate_value(&ruleset, &input).unwrap();
    let mut output = ptr::null_mut();
    // SAFETY: a live decision and a writable out-parameter.
    let status = unsafe { cr_decision_output_value(decision.0, &raw mut output, ptr::null_mut()) };
    assert_eq!(status, CR_OK);
    let output = Value(output);
    // Nothing selected: a plain null.
    assert_eq!(value_to_json(output.0), "null");
    // SAFETY: an input that is NULL is refused.
    let failure = common::handle(|o, e| unsafe {
        cr_ruleset_evaluate_value(ruleset.0, ptr::null(), ptr::null(), o, e)
    });
    assert_eq!(failure.err().unwrap().status, CR_INVALID_ARGUMENT);
}

#[test]
fn values_round_trip_through_json_and_copies() {
    let text = r#"{"a":[1,1.5,{"$decimal":"inf"},"x",{"$date":"2026-11-03"},{"$datetime":"2026-11-03T09:30:00"},{"$datetime":"2026-11-03T09:30:00.5+05:30"},null,{"$map":{"$k":true}}],"b":{"$int":"99999999999999999999"}}"#;
    let value = value_from_json(text).unwrap();
    let copy = Value(common::copy(value.0));
    assert_eq!(value_to_json(copy.0), value_to_json(value.0));
    assert!(value_to_json(value.0).ends_with(r#""b":null}"#));
    assert_eq!(value_from_json("[").err().unwrap().status, CR_FORMAT_ERROR);
}

#[test]
#[allow(clippy::too_many_lines)] // One call per reader, for every kind.
fn every_reader_refuses_another_kind() {
    let values = value_from_json(
        r#"[null,true,1,1.5,{"$decimal":"1"},"s",{"$date":"2026-01-01"},{"$datetime":"2026-01-01T00:00:00"},{"$datetime":"2026-01-01T00:00:00Z"},[],{}]"#,
    )
    .unwrap();
    let mut opaque = ptr::null_mut();
    // SAFETY: valid arguments.
    let status =
        unsafe { cr_value_new_opaque(c"x".as_ptr(), 1, 0, &raw mut opaque, ptr::null_mut()) };
    assert_eq!(status, CR_OK);
    let opaque = Value(opaque);
    // SAFETY: live values; every out-parameter is a local or NULL.
    unsafe {
        let mut items: Vec<*const cr_value> = (0..cr_value_len(values.0))
            .map(|i| cr_value_list_get(values.0, i))
            .collect();
        items.push(opaque.0);
        for item in items {
            let kind = cr_value_kind(item);
            let (mut b, mut i, mut f, mut t) = (false, 0_i64, 0.0, 0_u64);
            let (mut text, mut len, mut out) = (ptr::null(), 0, ptr::null_mut());
            let refused = |status: cr_status, wanted: cr_value_type| {
                assert_eq!(
                    status == CR_OK,
                    kind == wanted,
                    "kind {kind}, reader for {wanted}"
                );
            };
            refused(
                cr_value_get_bool(item, &raw mut b, ptr::null_mut()),
                CR_VALUE_BOOL,
            );
            refused(
                cr_value_get_int(item, &raw mut i, ptr::null_mut()),
                CR_VALUE_INT,
            );
            refused(
                cr_value_get_float(item, &raw mut f, ptr::null_mut()),
                CR_VALUE_FLOAT,
            );
            refused(
                cr_value_get_opaque_token(item, &raw mut t, ptr::null_mut()),
                CR_VALUE_OPAQUE,
            );
            refused(
                cr_value_get_string(item, &raw mut text, &raw mut len, ptr::null_mut()),
                CR_VALUE_STRING,
            );
            refused(
                cr_value_get_opaque_type(item, &raw mut text, &raw mut len, ptr::null_mut()),
                CR_VALUE_OPAQUE,
            );
            let status = cr_value_get_decimal(item, &raw mut out, ptr::null_mut(), ptr::null_mut());
            cr_string_free(out);
            refused(status, CR_VALUE_DECIMAL);
            let status = cr_value_get_aware_datetime(
                item,
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
            );
            refused(status, CR_VALUE_AWARE_DATETIME);
            let status = cr_value_get_null(
                item,
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
            );
            assert_eq!(status == CR_OK, kind == CR_VALUE_NULL);
            let status = cr_value_map_entry(
                item,
                0,
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
            );
            assert_ne!(status, CR_OK);
            let status = cr_value_get_date(
                item,
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
            );
            assert_eq!(status == CR_OK, kind == CR_VALUE_DATE);
            let null = ptr::null_mut();
            let status = cr_value_get_naive_datetime(
                item,
                null,
                null,
                null,
                null,
                null,
                null,
                null,
                ptr::null_mut(),
            );
            assert_eq!(status == CR_OK, kind == CR_VALUE_NAIVE_DATETIME);
        }
    }
}

#[test]
fn values_c_cannot_hold_are_refused() {
    let status = |r: Result<Value, common::Failure>| r.err().unwrap().status;
    // SAFETY: valid pointers; the arguments are outside what a value holds.
    unsafe {
        assert_eq!(
            status(made(|o, e| cr_value_new_date(10_000, 1, 1, o, e))),
            CR_INVALID_ARGUMENT
        );
        assert_eq!(
            status(made(|o, e| cr_value_new_date(0, 2, 29, o, e))),
            CR_INVALID_ARGUMENT
        );
        assert_eq!(
            status(made(|o, e| cr_value_new_date(-1, 3, 1, o, e))),
            CR_INVALID_ARGUMENT
        );
        let far = made(|o, e| cr_value_new_aware_datetime(i64::MAX, 0, 0, ptr::null(), 0, o, e));
        assert_eq!(status(far), CR_INVALID_ARGUMENT);
    }
    assert!(cr_value_new_list(usize::MAX).is_null());
}

#[test]
fn instants_before_1970_round_trip() {
    let before = aware(-1, 0, "");
    let read = value_from_json(r#"{"$datetime":"1969-12-31T23:59:59.5Z"}"#).unwrap();
    let (mut seconds, mut micro) = (0, 0);
    // SAFETY: a live value and writable out-parameters.
    let status = unsafe {
        cr_value_get_aware_datetime(
            read.0,
            &raw mut seconds,
            &raw mut micro,
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
        )
    };
    assert_eq!((status, seconds, micro), (CR_OK, -1, 500_000));
    let copy = Value(common::copy(read.0));
    assert_eq!(value_to_json(copy.0), value_to_json(read.0));
    assert_eq!(
        value_to_json(before.0),
        r#"{"$datetime":"1969-12-31T23:59:59Z"}"#
    );
}

#[test]
fn a_zone_is_kept_only_under_its_own_spelling() {
    let lower = aware(1_793_694_600, 3600, "europe/madrid");
    let written = value_to_json(lower.0);
    assert_eq!(written, r#"{"$datetime":"2026-11-03T09:30:00+01:00"}"#);
    assert!(value_from_json(&written).is_ok());
}

#[test]
fn an_item_cannot_go_into_itself() {
    let list = Value(cr_value_new_list(1));
    let mut error = ptr::null_mut();
    // SAFETY: an owned list; the call refuses it as its own item and keeps it.
    let status = unsafe { cr_value_list_set(list.0, 0, list.0, &raw mut error) };
    assert_eq!(status, CR_INVALID_ARGUMENT);
    common::take(error);
    let map = Value(cr_value_new_map());
    // SAFETY: as above, for a map.
    let status = unsafe { cr_value_map_insert(map.0, c"a".as_ptr(), 1, map.0, ptr::null_mut()) };
    assert_eq!(status, CR_INVALID_ARGUMENT);
    assert_eq!(value_to_json(list.0), "[null]");
}

#[test]
#[cfg_attr(miri, ignore = "builds a thousand nested lists")]
fn values_nest_as_deep_as_json_allows() {
    let mut value = cr_value_new_list(0);
    for _ in 1..1024 {
        let parent = cr_value_new_list(1);
        // SAFETY: owned values; the child moves into its parent.
        let status = unsafe { cr_value_list_set(parent, 0, value, ptr::null_mut()) };
        assert_eq!(status, CR_OK);
        value = parent;
    }
    let deep = Value(value);
    let parent = Value(cr_value_new_list(1));
    let mut error = ptr::null_mut();
    // SAFETY: owned values; the refused item is consumed all the same.
    let status = unsafe { cr_value_list_set(parent.0, 0, deep.0, &raw mut error) };
    std::mem::forget(deep);
    assert_eq!(status, CR_LIMIT_EXCEEDED);
    common::take(error);
}

#[test]
fn an_input_nested_too_deep_is_refused() {
    let ruleset = common::read_ruleset(RULES.as_bytes(), CR_FORMAT_JSON).unwrap();
    let input = value_from_json(r#"{"customer": {"years": [1]}}"#).unwrap();
    let limits = common::limits(&[(CR_LIMIT_MAX_NESTING, 2)]);
    // SAFETY: live handles.
    let failure = common::handle(|o, e| unsafe {
        cr_ruleset_evaluate_value(ruleset.0, input.0, limits.0, o, e)
    });
    assert_eq!(failure.err().unwrap().status, CR_LIMIT_EXCEEDED);
}

#[test]
fn argument_errors_name_the_function_and_the_argument() {
    let map = Value(cr_value_new_map());
    let mut error = ptr::null_mut();
    // SAFETY: an owned map and item; the key is not UTF-8 on purpose.
    let status = unsafe {
        cr_value_map_insert(
            map.0,
            c"\xff".as_ptr(),
            1,
            cr_value_new_int(1),
            &raw mut error,
        )
    };
    assert_eq!(status, CR_FORMAT_ERROR);
    let failure = common::take(error);
    let common::Failure {
        json: cleanrules::json::Json::Object(members),
        ..
    } = &failure
    else {
        panic!()
    };
    let message = members.iter().find(|(k, _)| k == "message").unwrap();
    assert!(
        matches!(&message.1, cleanrules::json::Json::String(m) if m.starts_with("cr_value_map_insert: key is not valid UTF-8")),
        "{message:?}"
    );
}

#[test]
fn borrowed_outputs_are_cleared_on_failure_and_never_dangle() {
    let int = Value(cr_value_new_int(1));
    let (mut text, mut len) = (c"stale".as_ptr(), 5);
    // SAFETY: a live value and writable out-parameters.
    let status =
        unsafe { cr_value_get_string(int.0, &raw mut text, &raw mut len, ptr::null_mut()) };
    assert_eq!(status, CR_INVALID_ARGUMENT);
    assert!(text.is_null());
    assert_eq!(len, 0);
    let empty = string("");
    // SAFETY: as above.
    let status =
        unsafe { cr_value_get_string(empty.0, &raw mut text, &raw mut len, ptr::null_mut()) };
    assert_eq!((status, len), (CR_OK, 0));
    assert!(!text.is_null());
}

#[test]
fn a_rule_set_names_its_aggregate() {
    let sum = RULES.replace(
        r#""hit_policy":"collect""#,
        r#""hit_policy":"collect","aggregate":"sum","strict":true"#,
    );
    let ruleset = common::read_ruleset(sum.as_bytes(), CR_FORMAT_JSON).unwrap();
    let plain = common::read_ruleset(RULES.as_bytes(), CR_FORMAT_JSON).unwrap();
    // SAFETY: live rule sets; static strings.
    unsafe {
        assert_eq!(
            std::ffi::CStr::from_ptr(cr_ruleset_aggregate(ruleset.0)),
            c"sum"
        );
        assert!(cr_ruleset_aggregate(plain.0).is_null());
        assert!(cr_ruleset_aggregate(ptr::null()).is_null());
        assert!(cr_ruleset_strict(ruleset.0));
        assert!(!cr_ruleset_strict(plain.0));
    }
}

#[test]
fn a_clone_keeps_what_json_loses() {
    let detail = "getter raised";
    let (p, n) = arg(detail.as_bytes());
    // SAFETY: the detail outlives the call.
    let null =
        made(|o, e| unsafe { cr_value_new_null_reason(CR_REASON_READ_ERROR, p, n, o, e) }).unwrap();
    let (p, n) = arg(b"handle");
    // SAFETY: the type name outlives the call.
    let opaque = made(|o, e| unsafe { cr_value_new_opaque(p, n, 7, o, e) }).unwrap();
    let list = Value(cr_value_new_list(2));
    // SAFETY: live values, each moved into the list once.
    unsafe {
        assert_eq!(cr_value_list_set(list.0, 0, null.0, ptr::null_mut()), CR_OK);
        assert_eq!(
            cr_value_list_set(list.0, 1, opaque.0, ptr::null_mut()),
            CR_OK
        );
    }
    std::mem::forget((null, opaque));
    // SAFETY: a live value.
    let copy = made(|o, e| unsafe { cr_value_clone(list.0, o, e) }).unwrap();
    drop(list);
    // SAFETY: the copy is live; its items are borrowed from it.
    unsafe {
        let (mut reason, mut token) = (0, 0);
        let first = cr_value_list_get(copy.0, 0);
        let status = cr_value_get_null(
            first,
            &raw mut reason,
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
        );
        assert_eq!((status, reason), (CR_OK, CR_REASON_READ_ERROR));
        let second = cr_value_list_get(copy.0, 1);
        assert_eq!(
            cr_value_get_opaque_token(second, &raw mut token, ptr::null_mut()),
            CR_OK
        );
        assert_eq!(token, 7);
    }
    // SAFETY: NULL is refused, and the out-parameter stays NULL.
    let Err(refused) = made(|o, e| unsafe { cr_value_clone(ptr::null(), o, e) }) else {
        panic!("a NULL value was cloned");
    };
    assert_eq!(refused.code(), "invalid_argument");
}
