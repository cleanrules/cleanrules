//! Reading documents against the `read` conformance vectors.
//!
//! Vectors whose document holds a rule, a rule set or a decision wait for the
//! readers of later tasks; every other vector of these files runs.

// A test: a panic is the failure report.
#![allow(clippy::unwrap_used, clippy::panic)]

mod common;

use cleanrules::format::{read_document, write_document};
use cleanrules::json::{Json, Limits, parse, to_string};
use common::{get, int, limits_of, same, text, vectors};

const NOT_YET: [&str; 3] = ["rule", "ruleset", "decision"];

#[test]
fn read_vectors() {
    let (mut run, mut skipped, mut failures) = (0, 0, Vec::new());
    for file in [
        "format/documents.json",
        "format/values.json",
        "format/conditions.json",
        "format/limits.json",
    ] {
        let all = vectors(file);
        let Json::Array(list) = &all else { panic!() };
        for vector in list {
            let id = text(get(vector, "id")).unwrap();
            let given = get(vector, "given").unwrap();
            let source = match (text(get(given, "text")), get(given, "document")) {
                (Some(source), _) => source.to_owned(),
                (None, Some(document)) => to_string(document),
                _ => panic!("{id}: nothing given"),
            };
            if let Ok(parsed) = parse(&source, &Limits::default())
                && let Json::Object(members) = &parsed
            {
                let keys: Vec<&str> = members.iter().map(|(k, _)| k.as_str()).collect();
                let contents: Vec<&&str> = keys
                    .iter()
                    .filter(|k| NOT_YET.contains(k) || **k == "condition")
                    .collect();
                if contents.len() == 1 && NOT_YET.contains(contents[0]) {
                    skipped += 1;
                    continue;
                }
            }
            run += 1;
            let expect = get(vector, "expect").unwrap();
            let problem = match (
                read_document(&source, &limits_of(given)),
                get(expect, "error"),
            ) {
                (Ok(document), None) => {
                    let wanted = get(expect, "document").unwrap();
                    let written = write_document(&document);
                    (!same(&written, wanted)).then(|| {
                        format!(
                            "wrote {}, expected {}",
                            to_string(&written),
                            to_string(wanted)
                        )
                    })
                }
                (Ok(document), Some(error)) => Some(format!(
                    "read {}, expected {}",
                    to_string(&write_document(&document)),
                    to_string(error)
                )),
                (Err(error), None) => Some(format!(
                    "{} at {:?}: {error}",
                    error.code(),
                    error.context()
                )),
                (Err(error), Some(wanted)) => {
                    let context = error.context();
                    let ok = error.code().as_str() == text(get(wanted, "code")).unwrap()
                        && (get(wanted, "pointer").is_none()
                            || context.pointer.as_deref() == text(get(wanted, "pointer")))
                        && (get(wanted, "line").is_none()
                            || context.line == int(get(wanted, "line")))
                        && (get(wanted, "column").is_none()
                            || context.column == int(get(wanted, "column")));
                    (!ok).then(|| {
                        format!(
                            "got {} pointer={:?} line={:?} column={:?} ({error}), expected {}",
                            error.code(),
                            context.pointer,
                            context.line,
                            context.column,
                            to_string(wanted)
                        )
                    })
                }
            };
            if let Some(problem) = problem {
                failures.push(format!("{id}: {problem}"));
            }
        }
    }
    println!("{run} read vectors run, {skipped} waiting for later readers");
    assert!(
        failures.is_empty(),
        "{} of {run} failed:\n{}",
        failures.len(),
        failures.join("\n")
    );
}
