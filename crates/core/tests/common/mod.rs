//! Helpers shared by the tests that read conformance vectors.

// Each test binary uses a different subset.
#![allow(dead_code, unreachable_pub, clippy::unwrap_used, clippy::panic)]

use std::fs;
use std::path::{Path, PathBuf};

use cleanrules::json::{Json, Limits, parse};

/// The conformance vectors directory.
pub fn vectors_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../conformance/vectors")
}

/// Every vector file, sorted.
pub fn vector_files() -> Vec<PathBuf> {
    let mut files = Vec::new();
    let mut dirs = vec![vectors_dir()];
    while let Some(dir) = dirs.pop() {
        for entry in fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                dirs.push(path);
            } else if path.extension().is_some_and(|e| e == "json") {
                files.push(path);
            }
        }
    }
    files.sort();
    files
}

/// The vectors of one file, such as `format/values.json`.
pub fn vectors(file: &str) -> Json {
    let text = fs::read_to_string(vectors_dir().join(file)).unwrap();
    let document = parse(&text, &Limits::default()).unwrap();
    get(&document, "vectors").unwrap().clone()
}

/// A member of an object.
pub fn get<'a>(value: &'a Json, key: &str) -> Option<&'a Json> {
    match value {
        Json::Object(members) => members.iter().find(|(k, _)| k == key).map(|(_, v)| v),
        _ => None,
    }
}

/// A member several levels down.
pub fn path<'a>(value: &'a Json, keys: &[&str]) -> Option<&'a Json> {
    keys.iter().try_fold(value, |v, k| get(v, k))
}

/// A string member.
pub fn text(value: Option<&Json>) -> Option<&str> {
    match value {
        Some(Json::String(s)) => Some(s),
        _ => None,
    }
}

/// A non-negative integer member.
pub fn int(value: Option<&Json>) -> Option<u64> {
    match value {
        Some(Json::Number(n)) => n.as_i128().and_then(|i| u64::try_from(i).ok()),
        _ => None,
    }
}

/// The limits of a `read` vector's `given`.
pub fn limits_of(given: &Json) -> Limits {
    let mut limits = Limits::default();
    if let Some(custom) = get(given, "limits") {
        if let Some(size) = int(get(custom, "max_size")) {
            // A vector's size limit applies to its text, JSON or YAML.
            limits.max_size = usize::try_from(size).unwrap();
            limits.max_yaml_size = limits.max_size;
        }
        if let Some(nesting) = int(get(custom, "max_nesting")) {
            limits.max_nesting = usize::try_from(nesting).unwrap();
        }
        if let Some(depth) = int(get(custom, "max_depth")) {
            limits.max_depth = usize::try_from(depth).unwrap();
        }
    }
    limits
}

/// Typed JSON equality (`spec/json-format.md`, *Comparing documents*): members in
/// order, numbers by lexical class and exact value, and the sign of a float zero.
pub fn same(a: &Json, b: &Json) -> bool {
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
