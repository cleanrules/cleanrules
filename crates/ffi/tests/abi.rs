//! The C ABI's own rules: statuses, errors, NULL arguments, limits, ownership
//! and threads.

// A test: a panic is the failure report.
#![allow(clippy::unwrap_used, clippy::panic)]

mod common;

use std::ffi::CStr;
use std::ptr;
use std::sync::Arc;

use cleanrules::json::{Json, same};
use cleanrules_ffi::*;
use common::{arg, json};

const RULES: &str = r#"{"version":1,"ruleset":{"hit_policy":"first","strict":false,"rules":[
    {"name":"vip","when":{"op":"gt","left":{"op":"field","path":["years"]},"right":{"op":"const","value":5}},
     "then":{"discount":{"op":"const","value":{"$decimal":"0.15"}}}},
    {"name":"other","when":{"op":"const","value":true},"then":{"discount":{"op":"const","value":0}}}]}}"#;

fn rules() -> common::RuleSet {
    common::read_ruleset(RULES.as_bytes(), CR_FORMAT_JSON).unwrap()
}

fn c_str(text: *const std::ffi::c_char) -> Option<&'static str> {
    // SAFETY: the library's static strings, or strings an error owns that the
    // test reads before freeing it.
    (!text.is_null()).then(|| unsafe { CStr::from_ptr(text) }.to_str().unwrap())
}

#[test]
fn versions() {
    assert_eq!(cr_abi_version(), CR_ABI_VERSION);
    assert_eq!(cr_format_version(), 1);
    assert_eq!(c_str(cr_version()), Some(env!("CARGO_PKG_VERSION")));
}

#[test]
fn statuses_have_names_and_parents() {
    assert_eq!(
        c_str(cr_status_name(CR_MISSING_FIELD)),
        Some("missing_field")
    );
    assert_eq!(c_str(cr_status_name(CR_UNSUPPORTED)), Some("unsupported"));
    assert_eq!(
        c_str(cr_status_name(CR_INVALID_ARGUMENT)),
        Some("invalid_argument")
    );
    assert_eq!(
        c_str(cr_status_name(CR_INTERNAL_ERROR)),
        Some("internal_error")
    );
    assert_eq!(c_str(cr_status_name(CR_OK)), Some("ok"));
    assert_eq!(c_str(cr_status_name(99)), None);
    assert_eq!(cr_error_parent(CR_LIMIT_EXCEEDED), CR_FORMAT_ERROR);
    assert_eq!(cr_error_parent(CR_TYPE_MISMATCH), CR_EVALUATION_ERROR);
    assert_eq!(cr_error_parent(CR_FORMAT_ERROR), CR_OK);
    assert_eq!(cr_error_parent(-1), CR_OK);
    assert!(cr_error_is(CR_MISSING_FIELD, CR_EVALUATION_ERROR));
    assert!(cr_error_is(CR_MISSING_FIELD, CR_MISSING_FIELD));
    assert!(!cr_error_is(CR_EVALUATION_ERROR, CR_MISSING_FIELD));
    assert!(cr_error_is(CR_INVALID_ARGUMENT, CR_INVALID_ARGUMENT));
    assert!(!cr_error_is(CR_INVALID_ARGUMENT, CR_INTERNAL_ERROR));
    assert!(!cr_error_is(CR_OK, CR_OK));
    assert!(!cr_error_is(CR_FORMAT_ERROR, CR_INVALID_ARGUMENT));
}

#[test]
fn an_error_has_a_message_a_hint_and_its_json_form() {
    let mut error = ptr::null_mut();
    let mut out = ptr::null_mut();
    let (p, n) = arg(b"{\"version\": 1,");
    // SAFETY: valid pointers; the text outlives the call.
    let status = unsafe {
        cr_document_read(
            p,
            n,
            CR_FORMAT_JSON,
            ptr::null(),
            &raw mut out,
            &raw mut error,
        )
    };
    assert_eq!(status, CR_FORMAT_ERROR);
    assert!(out.is_null());
    // SAFETY: a live error, freed once after its strings are read.
    unsafe {
        assert_eq!(cr_error_code(error), CR_FORMAT_ERROR);
        assert_ne!(c_str(cr_error_message(error)).unwrap(), "");
        let actual = json(c_str(cr_error_json(error)).unwrap());
        let wanted = json(r#"{"code":"format_error","line":1,"column":15}"#);
        let (Json::Object(actual), Json::Object(wanted)) = (&actual, &wanted) else {
            panic!()
        };
        for (key, value) in wanted {
            assert!(
                actual.iter().any(|(k, v)| k == key && same(v, value)),
                "{key}"
            );
        }
        cr_error_free(error);
    }
    // An invalid argument names the function and says how to fix it.
    let failure = common::read_document(b"{}", 0, None).err().unwrap();
    let Json::Object(members) = &failure.json else {
        panic!()
    };
    let text = |key: &str| match members.iter().find(|(k, _)| k == key) {
        Some((_, Json::String(s))) => s.clone(),
        _ => panic!("no {key}"),
    };
    assert!(text("message").starts_with("cr_document_read: format 0"));
    assert!(text("hint").contains("CR_FORMAT_JSON (1)"));
    assert!(common::read_ruleset(b"", CR_FORMAT_JSON).is_err());
    // SAFETY: NULL is accepted everywhere an error is read.
    unsafe {
        assert_eq!(cr_error_code(ptr::null()), CR_INVALID_ARGUMENT);
        assert!(cr_error_message(ptr::null()).is_null());
        assert!(cr_error_hint(ptr::null()).is_null());
        assert!(cr_error_json(ptr::null()).is_null());
        cr_error_free(ptr::null_mut());
    }
}

#[test]
fn the_error_argument_may_be_null() {
    let mut out = ptr::null_mut();
    let (p, n) = arg(b"[");
    // SAFETY: valid pointers; no error asked for.
    let status = unsafe {
        cr_document_read(
            p,
            n,
            CR_FORMAT_JSON,
            ptr::null(),
            &raw mut out,
            ptr::null_mut(),
        )
    };
    assert_eq!(status, CR_FORMAT_ERROR);
    assert!(out.is_null());
}

#[test]
fn null_and_unknown_arguments_are_invalid() {
    let invalid = |failure: common::Failure| {
        assert_eq!(failure.status, CR_INVALID_ARGUMENT);
        assert_eq!(failure.code(), "invalid_argument");
    };
    // A text of some length at NULL.
    // SAFETY: the library checks the NULL before reading.
    invalid(
        common::handle(|out, error| unsafe {
            cr_document_read(ptr::null(), 3, CR_FORMAT_JSON, ptr::null(), out, error)
        })
        .err()
        .unwrap(),
    );
    // NULL with a length of 0 is the empty text, which is not a document.
    // SAFETY: the empty text.
    let empty = common::handle(|out, error| unsafe {
        cr_document_read(ptr::null(), 0, CR_FORMAT_JSON, ptr::null(), out, error)
    });
    assert_eq!(empty.err().unwrap().status, CR_FORMAT_ERROR);
    // Unknown formats and limits.
    invalid(common::read_document(b"{}", 0, None).err().unwrap());
    invalid(common::read_document(b"{}", 5, None).err().unwrap());
    let limits = common::limits(&[]);
    let mut error = ptr::null_mut();
    // SAFETY: a live handle and a writable error.
    let status = unsafe { cr_limits_set(limits.0, 9, 1, &raw mut error) };
    assert_eq!(status, CR_INVALID_ARGUMENT);
    invalid(common::take(error));
    // NULL handles and out-parameters.
    // SAFETY: the library checks every NULL before using it.
    unsafe {
        invalid(
            common::text(|out, len, error| {
                cr_ruleset_write(ptr::null(), CR_FORMAT_JSON, out, len, error)
            })
            .err()
            .unwrap(),
        );
        invalid(
            common::text(|out, len, error| {
                cr_decision_explain(ptr::null(), CR_FORMAT_JSON, out, len, error)
            })
            .err()
            .unwrap(),
        );
        let mut error = ptr::null_mut();
        let ruleset = rules();
        assert_eq!(
            cr_ruleset_write(
                ruleset.0,
                CR_FORMAT_JSON,
                ptr::null_mut(),
                ptr::null_mut(),
                &raw mut error
            ),
            CR_INVALID_ARGUMENT
        );
        invalid(common::take(error));
        let mut value = 0;
        assert_eq!(
            cr_limits_get(
                ptr::null(),
                CR_LIMIT_MAX_DEPTH,
                &raw mut value,
                ptr::null_mut()
            ),
            CR_INVALID_ARGUMENT
        );
        assert_eq!(
            cr_limits_get(
                limits.0,
                CR_LIMIT_MAX_DEPTH,
                ptr::null_mut(),
                ptr::null_mut()
            ),
            CR_INVALID_ARGUMENT
        );
        assert_eq!(
            cr_limits_set(ptr::null_mut(), CR_LIMIT_MAX_DEPTH, 1, ptr::null_mut()),
            CR_INVALID_ARGUMENT
        );
        assert_eq!(cr_document_kind(ptr::null()), 0);
        cr_document_free(ptr::null_mut());
        cr_ruleset_free(ptr::null_mut());
        cr_decision_free(ptr::null_mut());
        cr_limits_free(ptr::null_mut());
        cr_string_free(ptr::null_mut());
    }
}

#[test]
fn limits_are_set_and_read() {
    let limits = common::limits(&[
        (CR_LIMIT_MAX_SIZE, 10),
        (CR_LIMIT_MAX_DEPTH, 7),
        (CR_LIMIT_MAX_NESTING, 5),
        (CR_LIMIT_MAX_TABLE_SIZE, 3),
    ]);
    for (which, wanted) in [
        (CR_LIMIT_MAX_SIZE, 10),
        (CR_LIMIT_MAX_DEPTH, 7),
        (CR_LIMIT_MAX_NESTING, 5),
        (CR_LIMIT_MAX_TABLE_SIZE, 3),
        (CR_LIMIT_MAX_YAML_SIZE, 10),
        (CR_LIMIT_MAX_TEXT_SIZE, 10),
    ] {
        let mut value = 0;
        // SAFETY: a live handle and a writable value.
        let status = unsafe { cr_limits_get(limits.0, which, &raw mut value, ptr::null_mut()) };
        assert_eq!(status, CR_OK);
        assert_eq!(value, wanted);
    }
    // The size limit applies to every format.
    let failure = common::read_document(b"RULE a\nIF x > 1", CR_FORMAT_TEXT, Some(&limits))
        .err()
        .unwrap();
    assert_eq!(failure.status, CR_LIMIT_EXCEEDED);
    // Each size on its own: the JSON limit stays when YAML's is lowered.
    let limits = common::limits(&[(CR_LIMIT_MAX_YAML_SIZE, 4), (CR_LIMIT_MAX_TEXT_SIZE, 4)]);
    let mut value = 0;
    // SAFETY: a live handle and a writable value.
    let status =
        unsafe { cr_limits_get(limits.0, CR_LIMIT_MAX_SIZE, &raw mut value, ptr::null_mut()) };
    assert_eq!((status, value), (CR_OK, 16 * 1024 * 1024));
    let failure = common::read_document(b"version: 1\n", CR_FORMAT_YAML, Some(&limits))
        .err()
        .unwrap();
    assert_eq!(failure.status, CR_LIMIT_EXCEEDED);
    assert!(common::read_document(RULES.as_bytes(), CR_FORMAT_JSON, Some(&limits)).is_ok());
}

#[test]
fn documents_change_format() {
    let document = common::read_document(RULES.as_bytes(), CR_FORMAT_JSON, None).unwrap();
    assert_eq!(common::kind(&document), CR_DOCUMENT_RULESET);
    let text = common::write_document(&document, CR_FORMAT_TEXT).unwrap();
    let again = common::read_document(text.as_bytes(), CR_FORMAT_TEXT, None).unwrap();
    let yaml = common::write_document(&again, CR_FORMAT_YAML).unwrap();
    let back = common::read_document(yaml.as_bytes(), CR_FORMAT_YAML, None).unwrap();
    let canonical = common::write_document(&document, CR_FORMAT_JSON).unwrap();
    assert_eq!(
        common::write_document(&back, CR_FORMAT_JSON).unwrap(),
        canonical
    );
    let failure = common::write_document(&back, CR_FORMAT_CSV).err().unwrap();
    assert_eq!(failure.status, CR_UNSUPPORTED);
    // Bytes that are not UTF-8, in any format.
    let failure = common::read_document(b"RULE a\nIF \xff", CR_FORMAT_TEXT, None)
        .err()
        .unwrap();
    assert_eq!(failure.status, CR_FORMAT_ERROR);
    let table = common::read_document(
        b"RULE,years,THEN discount\nvip,> 5,0.15\n",
        CR_FORMAT_CSV,
        None,
    )
    .unwrap();
    assert_eq!(common::kind(&table), CR_DOCUMENT_DECISION_TABLE);
    let compiled = common::ruleset_of(&table).unwrap();
    let decision = common::evaluate(&compiled, br#"{"years": 7}"#).unwrap();
    assert_eq!(
        common::output(&decision).unwrap(),
        r#"{"discount":{"$decimal":"0.15"}}"#
    );
    let condition = common::read_document(
        br#"{"version":1,"condition":{"op":"const","value":true}}"#,
        CR_FORMAT_JSON,
        None,
    )
    .unwrap();
    assert_eq!(common::kind(&condition), CR_DOCUMENT_CONDITION);
    assert_eq!(
        common::ruleset_of(&condition).err().unwrap().status,
        CR_FORMAT_ERROR
    );
}

#[test]
fn one_rule_is_a_rule_set_of_one() {
    let ruleset = common::read_ruleset(
        b"RULE vip\nIF years > 5\nTHEN discount = 0.15",
        CR_FORMAT_TEXT,
    )
    .unwrap();
    let summary = common::summary(&ruleset, CR_FORMAT_JSON).unwrap();
    assert!(summary.contains(r#""hit_policy":"first""#), "{summary}");
    let decision = common::evaluate(&ruleset, br#"{"years": 7}"#).unwrap();
    assert_eq!(
        common::output(&decision).unwrap(),
        r#"{"discount":{"$decimal":"0.15"}}"#
    );
    let rule = br#"{"version":1,"rule":{"name":"r","when":{"op":"const","value":true}}}"#;
    assert!(common::read_ruleset(rule, CR_FORMAT_JSON).is_ok());
    // A condition is not a rule.
    let failure = common::read_ruleset(b"years > 5", CR_FORMAT_TEXT)
        .err()
        .unwrap();
    assert_eq!(failure.status, CR_FORMAT_ERROR);
}

#[test]
fn a_decision_says_what_it_decided() {
    let ruleset = rules();
    let decision = common::evaluate(&ruleset, br#"{"years": 7}"#).unwrap();
    assert_eq!(
        common::output(&decision).unwrap(),
        r#"{"discount":{"$decimal":"0.15"}}"#
    );
    assert_eq!(
        common::outputs(&decision).unwrap(),
        r#"[{"discount":{"$decimal":"0.15"}}]"#
    );
    let text = common::explain(&decision, CR_FORMAT_TEXT).unwrap();
    assert!(text.starts_with("HIT POLICY first: selected vip"), "{text}");
    let summary = common::summary(&ruleset, CR_FORMAT_TEXT).unwrap();
    assert!(summary.contains("vip"), "{summary}");
    // A decision has no text form, and is read back from JSON or YAML.
    assert_eq!(
        common::write_decision(&decision, CR_FORMAT_TEXT)
            .err()
            .unwrap()
            .status,
        CR_UNSUPPORTED
    );
    for format in [CR_FORMAT_JSON, CR_FORMAT_YAML] {
        let written = common::write_decision(&decision, format).unwrap();
        let read = common::read_decision(&ruleset, written.as_bytes(), format, None).unwrap();
        assert_eq!(common::write_decision(&read, format).unwrap(), written);
        assert_eq!(common::explain(&read, CR_FORMAT_TEXT).unwrap(), text);
    }
    let failure = common::read_decision(&ruleset, b"{}", CR_FORMAT_TEXT, None)
        .err()
        .unwrap();
    assert_eq!(failure.status, CR_UNSUPPORTED);
    // An input that is not a data value.
    assert_eq!(
        common::evaluate(&ruleset, b"{\"years\": ")
            .err()
            .unwrap()
            .status,
        CR_FORMAT_ERROR
    );
}

#[test]
fn several_outputs_need_outputs() {
    let collect = RULES.replace(r#""hit_policy":"first""#, r#""hit_policy":"collect""#);
    let ruleset = common::read_ruleset(collect.as_bytes(), CR_FORMAT_JSON).unwrap();
    let decision = common::evaluate(&ruleset, br#"{"years": 7}"#).unwrap();
    assert_eq!(
        common::output(&decision).err().unwrap().status,
        CR_HIT_POLICY_ERROR
    );
    assert_eq!(
        common::outputs(&decision).unwrap(),
        r#"[{"discount":{"$decimal":"0.15"}},{"discount":0}]"#
    );
}

#[test]
fn strict_errors_come_from_evaluate() {
    let strict = RULES.replace(r#""strict":false"#, r#""strict":true"#);
    let ruleset = common::read_ruleset(strict.as_bytes(), CR_FORMAT_JSON).unwrap();
    let failure = common::evaluate(&ruleset, b"{}").err().unwrap();
    assert_eq!(failure.status, CR_MISSING_FIELD);
    assert!(cr_error_is(failure.status, CR_EVALUATION_ERROR));
}

#[test]
fn a_decision_outlives_its_rule_set() {
    let ruleset = rules();
    let decision = common::evaluate(&ruleset, br#"{"years": 1}"#).unwrap();
    drop(ruleset);
    assert_eq!(common::output(&decision).unwrap(), r#"{"discount":0}"#);
}

#[test]
fn handles_are_shared_between_threads() {
    struct Shared(*mut cr_ruleset);
    // SAFETY: a rule set never changes once made, so threads may read it at once.
    unsafe impl Send for Shared {}
    // SAFETY: as above.
    unsafe impl Sync for Shared {}
    let ruleset = rules();
    let shared = Arc::new(Shared(ruleset.0));
    let threads: Vec<_> = (0..4)
        .map(|i| {
            let shared = Arc::clone(&shared);
            std::thread::spawn(move || {
                let input = format!(r#"{{"years": {i}}}"#);
                let (p, n) = arg(input.as_bytes());
                // SAFETY: the rule set outlives the threads, joined below.
                let decision = common::handle(|out, error| unsafe {
                    cr_ruleset_evaluate(shared.0, p, n, ptr::null(), out, error)
                })
                .map(common::Decision)
                .unwrap();
                common::output(&decision).unwrap()
            })
        })
        .collect();
    for thread in threads {
        assert_eq!(thread.join().unwrap(), r#"{"discount":0}"#);
    }
}
