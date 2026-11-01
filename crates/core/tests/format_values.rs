//! Reading and writing values against the `format/values` conformance vectors.
//!
//! Each of these vectors compares `a` with one constant; the constant is read at
//! `/condition/right/value` and written back in canonical form. The documents
//! around it are read by later tasks.

// A test: a panic is the failure report.
#![allow(clippy::unwrap_used, clippy::panic)]

mod common;

use cleanrules::format::{ValueContext, read_value, write_value};
use cleanrules::json::{Json, Limits, parse, to_string};
use common::{get, path, same, text, vectors};

const AT: &str = "/condition/right/value";

#[test]
fn value_vectors() {
    let all = vectors("format/values.json");
    let Json::Array(vectors) = &all else {
        panic!("no vectors")
    };
    let mut failures = Vec::new();
    for vector in vectors {
        let id = text(get(vector, "id")).unwrap();
        let given = get(vector, "given").unwrap();
        let document = match text(get(given, "text")) {
            Some(source) => parse(source, &Limits::default()).unwrap(),
            None => get(given, "document").unwrap().clone(),
        };
        let constant = path(&document, &["condition", "right", "value"]).unwrap();
        let result = read_value(constant, AT, ValueContext::Constant);
        let expect = get(vector, "expect").unwrap();
        let outcome = match (result, get(expect, "error")) {
            (Ok(value), None) => {
                let wanted = path(expect, &["document", "condition", "right", "value"]).unwrap();
                let written = write_value(&value);
                if same(&written, wanted) {
                    None
                } else {
                    Some(format!(
                        "wrote {}, expected {}",
                        to_string(&written),
                        to_string(wanted)
                    ))
                }
            }
            (Ok(value), Some(error)) => Some(format!(
                "read {}, expected {}",
                to_string(&write_value(&value)),
                to_string(error)
            )),
            (Err(error), None) => Some(format!(
                "{} at {:?}: {error}",
                error.code(),
                error.context().pointer
            )),
            (Err(error), Some(wanted)) => {
                let code = text(get(wanted, "code")).unwrap();
                let pointer = text(get(wanted, "pointer"));
                if error.code().as_str() == code && error.context().pointer.as_deref() == pointer {
                    None
                } else {
                    Some(format!(
                        "got {} at {:?} ({error}), expected {code} at {pointer:?}",
                        error.code(),
                        error.context().pointer
                    ))
                }
            }
        };
        if let Some(problem) = outcome {
            failures.push(format!("{id}: {problem}"));
        }
    }
    assert!(
        failures.is_empty(),
        "{} of {} failed:\n{}",
        failures.len(),
        vectors.len(),
        failures.join("\n")
    );
}
