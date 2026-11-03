//! The conformance vectors, through the C ABI: documents read and written in
//! every format, grids, decisions read with their rule set, single conditions
//! and values, evaluations from JSON and from native values, batches,
//! explanations and summaries. Every vector runs.

// A test: a panic is the failure report.
#![allow(clippy::unwrap_used, clippy::panic)]

mod common;

use std::fs;
use std::path::{Path, PathBuf};

use cleanrules::json::{Json, same, to_string};
use cleanrules_ffi::{
    CR_FORMAT_CSV, CR_FORMAT_JSON, CR_FORMAT_TEXT, CR_FORMAT_YAML, CR_LIMIT_MAX_DEPTH,
    CR_LIMIT_MAX_NESTING, CR_LIMIT_MAX_SIZE, CR_LIMIT_MAX_TABLE_SIZE,
};
use common::{Failure, Limits_, json};

fn files() -> Vec<PathBuf> {
    let mut files = Vec::new();
    let mut dirs = vec![Path::new(env!("CARGO_MANIFEST_DIR")).join("../../conformance/vectors")];
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

fn string(value: Option<&Json>) -> Option<&str> {
    match value {
        Some(Json::String(s)) => Some(s),
        _ => None,
    }
}

fn strip_detail(json: &Json) -> Json {
    match json {
        Json::Object(members) => Json::Object(
            members
                .iter()
                .filter(|(k, _)| k != "detail")
                .map(|(k, v)| (k.clone(), strip_detail(v)))
                .collect(),
        ),
        Json::Array(items) => Json::Array(items.iter().map(strip_detail).collect()),
        other => other.clone(),
    }
}

/// An object with only these members.
fn only(json: &Json, keys: &[&str]) -> Json {
    match json {
        Json::Object(members) => Json::Object(
            members
                .iter()
                .filter(|(k, _)| keys.contains(&k.as_str()))
                .cloned()
                .collect(),
        ),
        other => other.clone(),
    }
}

/// The compared fields of an error, as the core's runners compare them.
const READ_FIELDS: [&str; 5] = ["code", "pointer", "line", "column", "name"];
const EVALUATE_FIELDS: [&str; 6] = ["code", "rule", "path", "types", "rules", "row"];

fn same_members(a: &Json, b: &Json) -> bool {
    let (Json::Object(a), Json::Object(b)) = (a, b) else {
        return false;
    };
    a.len() == b.len()
        && b.iter()
            .all(|(k, v)| a.iter().any(|(l, w)| l == k && same(w, v)))
}

/// Compare an error: on the compared fields for evaluations, and for reading
/// on the fields the vector lists, as the core's and Python's runners do.
fn compare_error(actual: &Failure, wanted: &Json, fields: &[&str]) -> Result<(), String> {
    let keys: Vec<&str> = if fields == READ_FIELDS {
        match wanted {
            Json::Object(members) => members.iter().map(|(k, _)| k.as_str()).collect(),
            _ => panic!("an error that is not an object"),
        }
    } else {
        fields.to_vec()
    };
    let actual = only(&actual.json, &keys);
    let wanted = only(wanted, &keys);
    if same_members(&actual, &wanted) {
        Ok(())
    } else {
        Err(format!(
            "error {}, expected {}",
            to_string(&actual),
            to_string(&wanted)
        ))
    }
}

/// Compare a result with the vector's expectation of `key`, or its error.
fn compare(
    actual: Result<Json, Failure>,
    expect: &Json,
    key: &str,
    fields: &[&str],
) -> Result<(), String> {
    match (actual, get(expect, "error")) {
        (Ok(json), None) => {
            let wanted = strip_detail(get(expect, key).unwrap());
            let actual = strip_detail(&json);
            if same(&actual, &wanted) {
                Ok(())
            } else {
                Err(format!(
                    "got {}\nwant {}",
                    to_string(&actual),
                    to_string(&wanted)
                ))
            }
        }
        (Ok(json), Some(error)) => Err(format!(
            "got {}, expected {}",
            to_string(&json),
            to_string(error)
        )),
        (Err(failure), None) => Err(format!("unexpected error {}", to_string(&failure.json))),
        (Err(failure), Some(error)) => compare_error(&failure, error, fields),
    }
}

/// The content of a document written by the ABI.
fn content(text: &str, key: &str) -> Json {
    get(&json(text), key).unwrap().clone()
}

fn limits_of(given: &Json) -> Option<Limits_> {
    let Some(Json::Object(members)) = get(given, "limits") else {
        return None;
    };
    let values: Vec<(i32, usize)> = members
        .iter()
        .map(|(k, v)| {
            let which = match k.as_str() {
                "max_size" => CR_LIMIT_MAX_SIZE,
                "max_nesting" => CR_LIMIT_MAX_NESTING,
                "max_depth" => CR_LIMIT_MAX_DEPTH,
                "max_table_size" => CR_LIMIT_MAX_TABLE_SIZE,
                other => panic!("unknown limit {other}"),
            };
            let Json::Number(n) = v else { panic!() };
            (which, usize::try_from(n.as_i128().unwrap()).unwrap())
        })
        .collect();
    Some(common::limits(&values))
}

fn ruleset(given: &Json) -> common::RuleSet {
    let document = format!(
        r#"{{"version":1,"ruleset":{}}}"#,
        to_string(get(given, "ruleset").unwrap())
    );
    common::read_ruleset(document.as_bytes(), CR_FORMAT_JSON).unwrap()
}

fn run_read(given: &Json, expect: &Json) -> Result<(), String> {
    let (source, format) = if let Some(text) = string(get(given, "text")) {
        (text.to_owned(), CR_FORMAT_JSON)
    } else if let Some(document) = get(given, "document") {
        (to_string(document), CR_FORMAT_JSON)
    } else if let Some(yaml) = string(get(given, "yaml")) {
        (yaml.to_owned(), CR_FORMAT_YAML)
    } else if let Some(text) = string(get(given, "rules_text")) {
        (text.to_owned(), CR_FORMAT_TEXT)
    } else if let Some(csv) = string(get(given, "table_csv")) {
        (csv.to_owned(), CR_FORMAT_CSV)
    } else if get(given, "table_grid").is_some() {
        (String::new(), 0)
    } else {
        panic!("nothing given")
    };
    let limits = limits_of(given);
    if get(given, "ruleset").is_some() {
        let ruleset = ruleset(given);
        let read = common::read_decision(&ruleset, source.as_bytes(), format, limits.as_ref())
            .and_then(|d| common::write_decision(&d, CR_FORMAT_JSON))
            .map(|text| json(&text));
        return compare(read, expect, "document", &READ_FIELDS);
    }
    let read = match get(given, "table_grid") {
        Some(grid) => read_grid(grid, limits.as_ref()),
        None => common::read_document(source.as_bytes(), format, limits.as_ref()),
    };
    let document = match read {
        Ok(document) => document,
        Err(failure) => return compare(Err(failure), expect, "document", &READ_FIELDS),
    };
    if let Some(error) = get(expect, "error") {
        let write = match string(get(given, "write")) {
            Some("text") => CR_FORMAT_TEXT,
            Some("csv") => CR_FORMAT_CSV,
            _ => return Err(format!("read a document, expected {}", to_string(error))),
        };
        return match common::write_document(&document, write) {
            Ok(text) => Err(format!("wrote {text:?}, expected {}", to_string(error))),
            Err(failure) => compare_error(&failure, error, &READ_FIELDS),
        };
    }
    if get(expect, "document").is_some() {
        let written = common::write_document(&document, CR_FORMAT_JSON).map(|t| json(&t));
        compare(written, expect, "document", &READ_FIELDS)?;
    }
    if let Some(wanted) = string(get(expect, "printed")) {
        let printed =
            common::write_document(&document, CR_FORMAT_TEXT).map_err(|f| to_string(&f.json))?;
        if printed != wanted {
            return Err(format!("printed {printed:?}, expected {wanted:?}"));
        }
    }
    if let Some(wanted) = string(get(expect, "csv")) {
        let csv =
            common::write_document(&document, CR_FORMAT_CSV).map_err(|f| to_string(&f.json))?;
        if csv != wanted {
            return Err(format!("wrote CSV {csv:?}, expected {wanted:?}"));
        }
    }
    if get(expect, "compiled").is_some() {
        let compiled = common::ruleset_of(&document)
            .and_then(|r| common::write_ruleset(&r, CR_FORMAT_JSON))
            .map(|t| content(&t, "ruleset"));
        compare(compiled, expect, "compiled", &READ_FIELDS)?;
    }
    Ok(())
}

/// A table from a vector's grid, built with `cr_grid_*`.
fn read_grid(grid: &Json, limits: Option<&Limits_>) -> Result<common::Document, Failure> {
    let settings = ["hit_policy", "aggregate", "logic"].map(|key| string(get(grid, key)));
    let strict = matches!(get(grid, "strict"), Some(Json::Bool(true)));
    let header = to_string(get(grid, "header").unwrap());
    let Some(Json::Array(rows)) = get(grid, "rows") else {
        panic!("a grid has rows")
    };
    let rows: Vec<String> = rows.iter().map(to_string).collect();
    common::grid(settings, strict, &header, &rows, limits)
}

/// A condition or value vector, through `cr_node_*`.
fn run_node(given: &Json, expect: &Json, as_value: bool) -> Result<(), String> {
    let key = if as_value { "value" } else { "condition" };
    let node = common::read_node(&to_string(get(given, key).unwrap()), CR_FORMAT_JSON)
        .map_err(|f| to_string(&f.json))?;
    let input = common::value_from_json(&to_string(get(given, "input").unwrap()))
        .map_err(|f| to_string(&f.json))?;
    let logic = string(get(given, "logic"));
    let strict = matches!(get(given, "strict"), Some(Json::Bool(true)));
    let result = common::evaluate_node(&node, &input, logic, strict, as_value).map(|t| json(&t));
    compare(result, expect, "result", &EVALUATE_FIELDS)
}

/// A batch vector, through `cr_ruleset_evaluate_many`.
fn run_batch(given: &Json, expect: &Json) -> Result<(), String> {
    let ruleset = ruleset(given);
    let inputs = common::value_from_json(&to_string(get(given, "inputs").unwrap()))
        .map_err(|f| to_string(&f.json))?;
    let decisions = common::evaluate_many(&ruleset, &inputs).map(|batch| {
        Json::Array(
            common::batch_documents(&batch)
                .iter()
                .map(|text| content(text, "decision"))
                .collect(),
        )
    });
    compare(decisions, expect, "decisions", &EVALUATE_FIELDS)
}

fn run_evaluate(given: &Json, expect: &Json, explain: bool) -> Result<(), String> {
    let ruleset = ruleset(given);
    let input = to_string(get(given, "input").unwrap());
    let decision = common::evaluate(&ruleset, input.as_bytes());
    let written = decision
        .as_ref()
        .map_err(|f| Failure {
            status: f.status,
            json: f.json.clone(),
        })
        .and_then(|d| common::write_decision(d, CR_FORMAT_JSON))
        .map(|text| content(&text, "decision"));
    compare(written, expect, "decision", &EVALUATE_FIELDS)?;
    // Again from a native value, read and rebuilt piece by piece.
    let read = common::value_from_json(&input).map_err(|f| to_string(&f.json))?;
    let native = common::Value(common::copy(read.0));
    if common::value_to_json(native.0) != common::value_to_json(read.0) {
        return Err(format!("the copy of {input} differs"));
    }
    let from_value = common::evaluate_value(&ruleset, &native)
        .and_then(|d| common::write_decision(&d, CR_FORMAT_JSON))
        .map(|text| content(&text, "decision"));
    compare(from_value, expect, "decision", &EVALUATE_FIELDS)?;
    if explain && let Ok(decision) = &decision {
        let explanation =
            common::explain(decision, CR_FORMAT_JSON).map(|text| content(&text, "explanation"));
        compare(explanation, expect, "explanation", &EVALUATE_FIELDS)?;
    }
    Ok(())
}

fn run(vector: &Json) -> Result<(), String> {
    let given = get(vector, "given").unwrap();
    let expect = get(vector, "expect").unwrap();
    match string(get(given, "kind")).unwrap() {
        "read" => run_read(given, expect),
        "condition" => run_node(given, expect, false),
        "value" => run_node(given, expect, true),
        "evaluate" if get(given, "inputs").is_some() => run_batch(given, expect),
        "evaluate" => run_evaluate(given, expect, false),
        "explain" => run_evaluate(given, expect, true),
        "summary" => {
            let summary = common::summary(&ruleset(given), CR_FORMAT_JSON)
                .map(|text| content(&text, "summary"));
            compare(summary, expect, "summary", &EVALUATE_FIELDS)
        }
        other => panic!("unknown kind {other}"),
    }
}

#[test]
fn the_vectors_pass_through_the_c_abi() {
    let (mut ran, mut failures) = (0, Vec::new());
    for file in files() {
        let document = json(&fs::read_to_string(&file).unwrap());
        let Some(Json::Array(vectors)) = get(&document, "vectors") else {
            panic!()
        };
        for vector in vectors {
            ran += 1;
            if let Err(problem) = run(vector) {
                failures.push(format!("{}: {problem}", string(get(vector, "id")).unwrap()));
            }
        }
    }
    assert!(
        failures.is_empty(),
        "{} of {ran} failed:\n{}",
        failures.len(),
        failures.join("\n")
    );
    assert!(ran >= 2508, "only {ran} vectors ran");
}
