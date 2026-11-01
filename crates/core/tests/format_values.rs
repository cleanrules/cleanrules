//! Reading and writing values against the `format/values` conformance vectors.
//!
//! Each of these vectors compares `a` with one constant; the constant is read at
//! `/condition/right/value` and written back in canonical form. The documents
//! around it are read by later tasks.

// A test: a panic is the failure report.
#![allow(clippy::unwrap_used, clippy::panic)]

use std::fs;
use std::path::Path;

use cleanrules::format::{ValueContext, read_value, write_value};
use cleanrules::json::{Json, Limits, parse, to_string};

const AT: &str = "/condition/right/value";

fn get<'a>(value: &'a Json, key: &str) -> Option<&'a Json> {
    match value {
        Json::Object(members) => members.iter().find(|(k, _)| k == key).map(|(_, v)| v),
        _ => None,
    }
}

fn path<'a>(value: &'a Json, keys: &[&str]) -> Option<&'a Json> {
    keys.iter().try_fold(value, |v, k| get(v, k))
}

fn text(value: Option<&Json>) -> Option<&str> {
    match value {
        Some(Json::String(s)) => Some(s),
        _ => None,
    }
}

/// Typed JSON equality (`spec/json-format.md`, *Comparing documents*): members in
/// order, numbers by lexical class and exact value, and the sign of a float zero.
fn same(a: &Json, b: &Json) -> bool {
    match (a, b) {
        (Json::Number(x), Json::Number(y)) => {
            if x.is_integer_literal() != y.is_integer_literal() {
                return false;
            }
            if x.is_integer_literal() {
                x.as_i128().is_some() && x.as_i128() == y.as_i128()
            } else {
                x.as_f64().to_bits() == y.as_f64().to_bits()
            }
        }
        (Json::Array(x), Json::Array(y)) => {
            x.len() == y.len() && x.iter().zip(y).all(|(p, q)| same(p, q))
        }
        (Json::Object(x), Json::Object(y)) => {
            x.len() == y.len() && x.iter().zip(y).all(|((k, p), (l, q))| k == l && same(p, q))
        }
        _ => a == b,
    }
}

#[test]
fn value_vectors() {
    let file =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../conformance/vectors/format/values.json");
    let vectors = parse(&fs::read_to_string(file).unwrap(), &Limits::default()).unwrap();
    let Some(Json::Array(vectors)) = get(&vectors, "vectors") else {
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
