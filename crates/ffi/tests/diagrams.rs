//! Diagrams through the C ABI (`spec/visual.md`): ownership of the text,
//! argument checks and the errors of the core.

// A test: a panic is the failure report.
#![allow(clippy::unwrap_used, clippy::panic)]

mod common;

use std::ptr;

use cleanrules::json::Json;
use cleanrules_ffi::*;
use common::{Failure, text};

const RULES: &str = "HIT POLICY first
RULE vip IF years > 5 THEN discount = 0.15 DO notify
RULE rest IF TRUE";

const TABLE: &[u8] = b"HIT POLICY,first\nRULE,years,THEN discount\nvip,> 5,0.15\n";

fn invalid(failure: &Failure, hint: &str) {
    assert_eq!(failure.status, CR_INVALID_ARGUMENT);
    let Json::Object(members) = &failure.json else {
        panic!()
    };
    assert!(
        members
            .iter()
            .any(|(k, v)| k == "hint" && matches!(v, Json::String(h) if h.contains(hint))),
        "{:?}",
        failure.json
    );
}

#[test]
fn every_kind_is_drawn() {
    let ruleset = common::read_ruleset(RULES.as_bytes(), CR_FORMAT_TEXT).unwrap();
    let node = common::read_node("x > 1 AND NOT y", CR_FORMAT_TEXT).unwrap();
    // SAFETY: a live node; default limits.
    let drawn = text(|out, len, error| unsafe {
        cr_node_draw(node.0, CR_DIAGRAM_MERMAID, ptr::null(), out, len, error)
    })
    .unwrap();
    assert!(
        drawn.starts_with("flowchart TD\n  n0([\"all\"])\n"),
        "{drawn}"
    );
    // The second rule's action is code of the program.
    let flags = [false, true];
    // SAFETY: a live rule set and two flags; default limits.
    let drawn = text(|out, len, error| unsafe {
        cr_ruleset_draw(
            ruleset.0,
            flags.as_ptr(),
            2,
            CR_DIAGRAM_DOT,
            ptr::null(),
            out,
            len,
            error,
        )
    })
    .unwrap();
    assert!(
        drawn.contains("DO notify") && drawn.contains("+action"),
        "{drawn}"
    );
    let decision = common::evaluate(&ruleset, br#"{"years": 7}"#).unwrap();
    // SAFETY: a live decision; default limits.
    let drawn = text(|out, len, error| unsafe {
        cr_decision_draw(decision.0, CR_DIAGRAM_HTML, ptr::null(), out, len, error)
    })
    .unwrap();
    assert!(drawn.starts_with("<!DOCTYPE html>") && drawn.contains("[x] vip (selected)"));
    let table = common::read_document(TABLE, CR_FORMAT_CSV, None).unwrap();
    let compiled = common::ruleset_of(&table).unwrap();
    let decision = common::evaluate(&compiled, br#"{"years": 7}"#).unwrap();
    // SAFETY: a live table and decision; default limits.
    let drawn = text(|out, len, error| unsafe {
        cr_document_draw(
            table.0,
            decision.0,
            CR_DIAGRAM_HTML_FRAGMENT,
            ptr::null(),
            out,
            len,
            error,
        )
    })
    .unwrap();
    assert!(
        drawn.contains("<tr class=\"rule_selected\" style=\"background:#e8f0fe;border:2px solid #1a73e8;border-radius:8px\"><th scope=\"row\">vip</th>"),
        "{drawn}"
    );
    // A null length is allowed.
    let mut out = ptr::null_mut();
    // SAFETY: a live node, a writable out, no length or error.
    let status = unsafe {
        cr_node_draw(
            node.0,
            CR_DIAGRAM_DOT,
            ptr::null(),
            &raw mut out,
            ptr::null_mut(),
            ptr::null_mut(),
        )
    };
    assert_eq!(status, CR_OK);
    // SAFETY: a text of this library, freed once.
    unsafe { cr_string_free(out) };
}

#[test]
fn arguments_are_checked() {
    let node = common::read_node("x > 1", CR_FORMAT_TEXT).unwrap();
    for format in [0, 5, -1] {
        // SAFETY: a live node; default limits.
        let failure = text(|out, len, error| unsafe {
            cr_node_draw(node.0, format, ptr::null(), out, len, error)
        })
        .unwrap_err();
        invalid(&failure, "CR_DIAGRAM_HTML_FRAGMENT (4)");
    }
    // SAFETY: NULL handles are refused before anything is read.
    let failure = text(|out, len, error| unsafe {
        cr_node_draw(ptr::null(), CR_DIAGRAM_DOT, ptr::null(), out, len, error)
    })
    .unwrap_err();
    assert_eq!(failure.status, CR_INVALID_ARGUMENT);
    // SAFETY: as above.
    let failure = text(|out, len, error| unsafe {
        cr_rule_draw(ptr::null(), CR_DIAGRAM_DOT, ptr::null(), out, len, error)
    })
    .unwrap_err();
    assert_eq!(failure.status, CR_INVALID_ARGUMENT);
    // SAFETY: as above.
    let failure = text(|out, len, error| unsafe {
        cr_decision_draw(ptr::null(), CR_DIAGRAM_DOT, ptr::null(), out, len, error)
    })
    .unwrap_err();
    assert_eq!(failure.status, CR_INVALID_ARGUMENT);
    let mut error = ptr::null_mut();
    // SAFETY: a live node, no out, a writable error.
    let status = unsafe {
        cr_node_draw(
            node.0,
            CR_DIAGRAM_DOT,
            ptr::null(),
            ptr::null_mut(),
            ptr::null_mut(),
            &raw mut error,
        )
    };
    assert_eq!(status, CR_INVALID_ARGUMENT);
    drop(common::take(error));
    let ruleset = common::read_ruleset(RULES.as_bytes(), CR_FORMAT_TEXT).unwrap();
    let flags = [true];
    // SAFETY: a live rule set and one readable flag.
    let failure = text(|out, len, error| unsafe {
        cr_ruleset_draw(
            ruleset.0,
            flags.as_ptr(),
            1,
            CR_DIAGRAM_DOT,
            ptr::null(),
            out,
            len,
            error,
        )
    })
    .unwrap_err();
    invalid(&failure, "one flag per rule");
    // SAFETY: a live rule set; NULL flags with a length.
    let failure = text(|out, len, error| unsafe {
        cr_ruleset_draw(
            ruleset.0,
            ptr::null(),
            2,
            CR_DIAGRAM_DOT,
            ptr::null(),
            out,
            len,
            error,
        )
    })
    .unwrap_err();
    invalid(&failure, "a length of 0");
    // A decision goes with a decision table only.
    let document = common::read_document(RULES.as_bytes(), CR_FORMAT_TEXT, None).unwrap();
    let decision = common::evaluate(&ruleset, b"{}").unwrap();
    // SAFETY: a live document and decision.
    let failure = text(|out, len, error| unsafe {
        cr_document_draw(
            document.0,
            decision.0,
            CR_DIAGRAM_DOT,
            ptr::null(),
            out,
            len,
            error,
        )
    })
    .unwrap_err();
    assert_eq!(failure.status, CR_UNSUPPORTED);
}

#[test]
fn errors_of_the_core_come_back() {
    let table = common::read_document(TABLE, CR_FORMAT_CSV, None).unwrap();
    // SAFETY: a live table; no decision; default limits.
    let failure = text(|out, len, error| unsafe {
        cr_document_draw(
            table.0,
            ptr::null(),
            CR_DIAGRAM_MERMAID,
            ptr::null(),
            out,
            len,
            error,
        )
    })
    .unwrap_err();
    assert_eq!(failure.status, CR_UNSUPPORTED);
    let other = common::read_ruleset(RULES.as_bytes(), CR_FORMAT_TEXT).unwrap();
    let decision = common::evaluate(&other, b"{}").unwrap();
    // SAFETY: a live table and decision.
    let failure = text(|out, len, error| unsafe {
        cr_document_draw(
            table.0,
            decision.0,
            CR_DIAGRAM_HTML,
            ptr::null(),
            out,
            len,
            error,
        )
    })
    .unwrap_err();
    assert_eq!(failure.status, CR_DECISION_MISMATCH);
    let limits = common::limits(&[(CR_LIMIT_MAX_DIAGRAM_SIZE, 2)]);
    let node = common::read_node("NOT x = 1", CR_FORMAT_TEXT).unwrap();
    // SAFETY: a live node and limits.
    let failure = text(|out, len, error| unsafe {
        cr_node_draw(node.0, CR_DIAGRAM_DOT, limits.0, out, len, error)
    })
    .unwrap_err();
    assert_eq!(failure.status, CR_LIMIT_EXCEEDED);
    let Json::Object(members) = &failure.json else {
        panic!()
    };
    assert!(
        members
            .iter()
            .any(|(k, v)| k == "name" && matches!(v, Json::String(n) if n == "max_diagram_size"))
    );
}
