//! The JSON reader against the conformance vectors.
//!
//! Every vector file must be readable by the reader itself. The `read` vectors
//! whose expected error is located by line and column (syntax errors, and limits
//! checked while parsing) are run here; the others need the document readers that
//! later tasks add.

// A test: a panic is the failure report.
#![allow(clippy::unwrap_used, clippy::panic)]

use std::fs;
use std::path::{Path, PathBuf};

use cleanrules::json::{Json, Limits, parse};

fn vectors_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../conformance/vectors")
}

fn vector_files() -> Vec<PathBuf> {
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

fn get<'a>(value: &'a Json, key: &str) -> Option<&'a Json> {
    match value {
        Json::Object(members) => members.iter().find(|(k, _)| k == key).map(|(_, v)| v),
        _ => None,
    }
}

fn text(value: Option<&Json>) -> Option<&str> {
    match value {
        Some(Json::String(s)) => Some(s),
        _ => None,
    }
}

fn int(value: Option<&Json>) -> Option<u64> {
    match value {
        Some(Json::Number(n)) => n.as_i128().and_then(|i| u64::try_from(i).ok()),
        _ => None,
    }
}

fn limits_of(given: &Json) -> Limits {
    let mut limits = Limits::default();
    if let Some(custom) = get(given, "limits") {
        if let Some(size) = int(get(custom, "max_size")) {
            limits.max_size = usize::try_from(size).unwrap();
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

#[test]
fn every_vector_file_is_readable() {
    let files = vector_files();
    assert!(
        files.len() > 20,
        "vectors not found in {}",
        vectors_dir().display()
    );
    for path in files {
        let content = fs::read_to_string(&path).unwrap();
        if let Err(error) = parse(&content, &Limits::default()) {
            panic!("{}: {error} at {:?}", path.display(), error.context());
        }
    }
}

#[test]
fn lexical_read_vectors() {
    let mut run = 0;
    let mut failures = Vec::new();
    for path in vector_files() {
        let document = parse(&fs::read_to_string(&path).unwrap(), &Limits::default()).unwrap();
        let Some(Json::Array(vectors)) = get(&document, "vectors") else {
            continue;
        };
        for vector in vectors {
            let id = text(get(vector, "id")).unwrap_or("?");
            let Some(given) = get(vector, "given") else {
                continue;
            };
            let Some(source) = text(get(given, "text")) else {
                continue;
            };
            let Some(expected) = get(vector, "expect").and_then(|e| get(e, "error")) else {
                continue;
            };
            let code = text(get(expected, "code")).unwrap_or_default();
            let lexical = int(get(expected, "line")).is_some()
                || (code == "limit_exceeded" && get(expected, "pointer").is_none());
            if !lexical {
                continue;
            }
            run += 1;
            let wanted = (
                code.to_owned(),
                int(get(expected, "line")),
                int(get(expected, "column")),
            );
            let actual = match parse(source, &limits_of(given)) {
                Ok(_) => ("<no error>".to_owned(), None, None),
                Err(error) => (
                    error.code().as_str().to_owned(),
                    error.context().line,
                    error.context().column,
                ),
            };
            if actual != wanted {
                failures.push(format!("{id}: expected {wanted:?}, got {actual:?}"));
            }
        }
    }
    assert!(run >= 10, "only {run} lexical vectors found");
    println!("{run} lexical read vectors");
    assert!(
        failures.is_empty(),
        "{} of {run} failed:\n{}",
        failures.len(),
        failures.join("\n")
    );
}
