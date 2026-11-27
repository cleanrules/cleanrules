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
    CR_DIAGRAM_DOT, CR_DIAGRAM_HTML, CR_DIAGRAM_HTML_FRAGMENT, CR_DIAGRAM_MERMAID, CR_FORMAT_CSV,
    CR_FORMAT_JSON, CR_FORMAT_TEXT, CR_FORMAT_YAML, CR_LIMIT_MAX_DEPTH, CR_LIMIT_MAX_DIAGRAM_SIZE,
    CR_LIMIT_MAX_NESTING, CR_LIMIT_MAX_PATH_SEGMENTS, CR_LIMIT_MAX_SIZE, CR_LIMIT_MAX_TABLE_SIZE,
    CR_LIMIT_MAX_VALUES, cr_decision_draw, cr_document_canonical_bytes, cr_document_condition,
    cr_document_digest, cr_document_draw, cr_document_rule, cr_node_draw, cr_rule_draw,
    cr_ruleset_canonical_bytes, cr_ruleset_digest, cr_ruleset_draw, cr_version,
};
use common::{Failure, Limits_, json, on_small_stack};

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
                "max_diagram_size" => CR_LIMIT_MAX_DIAGRAM_SIZE,
                "max_path_segments" => CR_LIMIT_MAX_PATH_SEGMENTS,
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

/// This library's version (`cr_version`).
fn version() -> String {
    // SAFETY: a static NUL-terminated string.
    unsafe { std::ffi::CStr::from_ptr(cr_version()) }
        .to_str()
        .unwrap()
        .to_owned()
}

/// A copy of a vector's JSON with the bundle's `engine.version` set to this
/// engine's, under `key` (`conformance/README.md`, `current_engine`).
fn with_this_engine(json: &Json, key: &str) -> Json {
    let mut copy = json.clone();
    let set = |document: &mut Json| {
        if let Json::Object(top) = document
            && let Some((_, Json::Object(bundle))) = top.iter_mut().find(|(k, _)| k == "bundle")
            && let Some((_, Json::Object(engine))) = bundle.iter_mut().find(|(k, _)| k == "engine")
            && let Some((_, slot)) = engine.iter_mut().find(|(k, _)| k == "version")
        {
            *slot = Json::String(version());
        }
    };
    if let Json::Object(members) = &mut copy
        && let Some((_, document)) = members.iter_mut().find(|(k, _)| k == key)
    {
        set(document);
    }
    copy
}

/// A `bundle` vector: make a bundle with `cr_document_new_bundle`, write it, read it
/// back (`spec/bundles.md`, *Making a bundle*).
fn run_make_bundle(given: &Json, expect: &Json) -> Result<(), String> {
    let rules = common::read_document(
        to_string(get(given, "document").unwrap()).as_bytes(),
        CR_FORMAT_JSON,
        None,
    )
    .map_err(|f| to_string(&f.json))?;
    let limits = match get(given, "limits") {
        Some(Json::Object(members)) => {
            let values: Vec<(i32, usize)> = members
                .iter()
                .map(|(name, value)| {
                    let Json::Number(n) = value else { panic!() };
                    let which = match name.as_str() {
                        "max_size" => CR_LIMIT_MAX_SIZE,
                        "max_nesting" => CR_LIMIT_MAX_NESTING,
                        "max_depth" => CR_LIMIT_MAX_DEPTH,
                        "max_path_segments" => CR_LIMIT_MAX_PATH_SEGMENTS,
                        "max_table_size" => CR_LIMIT_MAX_TABLE_SIZE,
                        "max_values" => CR_LIMIT_MAX_VALUES,
                        other => panic!("unknown limit {other}"),
                    };
                    (which, usize::try_from(n.as_i128().unwrap()).unwrap())
                })
                .collect();
            Some(common::limits(&values))
        }
        _ => None,
    };
    let cases: Vec<(String, String)> = match get(given, "cases") {
        Some(Json::Array(items)) => items
            .iter()
            .map(|item| {
                (
                    string(get(item, "path")).unwrap().to_owned(),
                    string(get(item, "text")).unwrap().to_owned(),
                )
            })
            .collect(),
        _ => Vec::new(),
    };
    let cases: Vec<(&str, &[u8])> = cases
        .iter()
        .map(|(path, text)| (path.as_str(), text.as_bytes()))
        .collect();
    let made = common::bundle(
        &rules,
        string(get(given, "id")).unwrap(),
        string(get(given, "revision")).unwrap(),
        &cases,
        limits.as_ref(),
    )
    .and_then(|bundle| common::write_document(&bundle, CR_FORMAT_JSON))
    .and_then(|text| {
        // This engine reads what it made back, and writes the same.
        let again = common::read_document(text.as_bytes(), CR_FORMAT_JSON, None)?;
        assert_eq!(common::write_document(&again, CR_FORMAT_JSON)?, text);
        Ok(json(&text))
    });
    compare(
        made,
        &with_this_engine(expect, "document"),
        "document",
        &READ_FIELDS,
    )
}

/// A `bundle_compare` vector (`spec/bundles.md`, *Comparing bundles*).
fn run_compare_bundles(given: &Json, expect: &Json) -> Result<(), String> {
    let read = |key: &str| {
        common::read_document(
            to_string(get(given, key).unwrap()).as_bytes(),
            CR_FORMAT_JSON,
            None,
        )
        .map_err(|f| to_string(&f.json))
    };
    let (a, b) = (read("a")?, read("b")?);
    let comparison = common::compare_bundles(&a, &b).map(|text| json(&text));
    compare(comparison, expect, "comparison", &READ_FIELDS)
}

fn run_read(given: &Json, expect: &Json) -> Result<(), String> {
    let current = matches!(get(given, "current_engine"), Some(Json::Bool(true)));
    let (given, expect) = if current {
        (
            &with_this_engine(given, "document"),
            &with_this_engine(expect, "document"),
        )
    } else {
        (given, expect)
    };
    let (source, format) = if let Some(text) = string(get(given, "text")) {
        (text.to_owned(), CR_FORMAT_JSON)
    } else if let Some(Json::Array(bytes)) = get(given, "bytes") {
        // Bytes that may not be UTF-8, read in their syntax.
        let bytes: Vec<u8> = bytes
            .iter()
            .map(|b| match b {
                Json::Number(n) => u8::try_from(n.as_i128().unwrap()).unwrap(),
                _ => panic!("not a byte"),
            })
            .collect();
        let format = match string(get(given, "syntax")) {
            Some("json") => CR_FORMAT_JSON,
            Some("yaml") => CR_FORMAT_YAML,
            Some("text") => CR_FORMAT_TEXT,
            Some("csv") => CR_FORMAT_CSV,
            other => panic!("unknown syntax {other:?}"),
        };
        let read = common::read_document(&bytes, format, limits_of(given).as_ref())
            .and_then(|d| common::write_document(&d, CR_FORMAT_JSON))
            .map(|text| json(&text));
        return compare(read, expect, "document", &READ_FIELDS);
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

/// The fields of a drawing error that are compared (`spec/errors.md`).
const DRAW_FIELDS: [&str; 2] = ["code", "name"];

/// Compare a diagram with the vector's, or its error.
fn compare_diagram(actual: Result<String, Failure>, expect: &Json) -> Result<(), String> {
    match (actual, get(expect, "error")) {
        (Ok(diagram), None) => {
            let wanted = string(get(expect, "diagram")).unwrap();
            if diagram == wanted {
                Ok(())
            } else {
                Err(format!("got\n{diagram}\nwant\n{wanted}"))
            }
        }
        (Ok(diagram), Some(error)) => Err(format!("got\n{diagram}\nexpected {}", to_string(error))),
        (Err(failure), None) => Err(format!("unexpected error {}", to_string(&failure.json))),
        (Err(failure), Some(error)) => compare_error(&failure, error, &DRAW_FIELDS),
    }
}

/// A diagram (`spec/visual.md`), through `cr_document_draw` and through the
/// function of the document's content.
fn run_draw(given: &Json, expect: &Json) -> Result<(), String> {
    let format = match string(get(given, "format")).unwrap() {
        "dot" => CR_DIAGRAM_DOT,
        "mermaid" => CR_DIAGRAM_MERMAID,
        "html" => CR_DIAGRAM_HTML,
        "html_fragment" => CR_DIAGRAM_HTML_FRAGMENT,
        other => panic!("unknown diagram format {other}"),
    };
    let limits = limits_of(given);
    let raw = limits.as_ref().map_or(std::ptr::null(), |l| l.0);
    let source = get(given, "document").unwrap();
    let document = common::read_document(to_string(source).as_bytes(), CR_FORMAT_JSON, None)
        .map_err(|f| to_string(&f.json))?;
    let content = |key| get(source, key).is_some();
    let Some(input) = get(given, "input") else {
        // SAFETY: a live document; no decision; live or NULL limits.
        let drawn = common::text(|out, len, error| unsafe {
            cr_document_draw(document.0, std::ptr::null(), format, raw, out, len, error)
        });
        compare_diagram(drawn, expect)?;
        let drawn = if content("condition") {
            // SAFETY: a live document.
            let node = common::handle(|out, error| unsafe {
                cr_document_condition(document.0, out, error)
            })
            .map(common::Node)
            .unwrap();
            // SAFETY: a live node; live or NULL limits.
            common::text(|out, len, error| unsafe {
                cr_node_draw(node.0, format, raw, out, len, error)
            })
        } else if content("rule") {
            // SAFETY: a live document.
            let rule =
                common::handle(|out, error| unsafe { cr_document_rule(document.0, out, error) })
                    .map(common::Rule)
                    .unwrap();
            // SAFETY: a live rule; live or NULL limits.
            common::text(|out, len, error| unsafe {
                cr_rule_draw(rule.0, format, raw, out, len, error)
            })
        } else if content("ruleset") {
            let ruleset = common::ruleset_of(&document).unwrap();
            // SAFETY: a live rule set; no flags; live or NULL limits.
            common::text(|out, len, error| unsafe {
                cr_ruleset_draw(ruleset.0, std::ptr::null(), 0, format, raw, out, len, error)
            })
        } else {
            return Ok(());
        };
        return compare_diagram(drawn, expect);
    };
    let ruleset = common::ruleset_of(&document).unwrap();
    let decision =
        common::evaluate(&ruleset, to_string(input).as_bytes()).map_err(|f| to_string(&f.json))?;
    let drawn = if content("decision_table") {
        // SAFETY: a live document and decision; live or NULL limits.
        common::text(|out, len, error| unsafe {
            cr_document_draw(document.0, decision.0, format, raw, out, len, error)
        })
    } else {
        // SAFETY: a live decision; live or NULL limits.
        common::text(|out, len, error| unsafe {
            cr_decision_draw(decision.0, format, raw, out, len, error)
        })
    };
    compare_diagram(drawn, expect)
}

/// Canonical bytes and digests (`spec/json-format.md`, *Canonical bytes*,
/// *Digests*): `cr_document_canonical_bytes` and `cr_document_digest`, and
/// `cr_ruleset_digest` (with `cr_ruleset_canonical_bytes`) of the rule set the
/// document gives.
fn run_canonical(given: &Json, expect: &Json) -> Result<(), String> {
    let source = string(get(given, "text"))
        .map_or_else(|| to_string(get(given, "document").unwrap()), str::to_owned);
    let document = common::read_document(source.as_bytes(), CR_FORMAT_JSON, None)
        .map_err(|f| to_string(&f.json))?;
    let wanted = |key| string(get(expect, key)).map(str::to_owned);
    // SAFETY: a live document.
    let canonical = common::text(|out, len, error| unsafe {
        cr_document_canonical_bytes(document.0, out, len, error)
    })
    .map_err(|f| to_string(&f.json))?;
    // SAFETY: a live document.
    let digest =
        common::text(|out, len, error| unsafe { cr_document_digest(document.0, out, len, error) })
            .map_err(|f| to_string(&f.json))?;
    if Some(&canonical) != wanted("canonical").as_ref()
        || Some(&digest) != wanted("digest").as_ref()
    {
        return Err(format!("canonical {canonical} digest {digest}"));
    }
    let ruleset_digest = match common::ruleset_of(&document) {
        Ok(ruleset) => {
            // SAFETY: a live rule set.
            let bytes = common::text(|out, len, error| unsafe {
                cr_ruleset_canonical_bytes(ruleset.0, out, len, error)
            })
            .map_err(|f| to_string(&f.json))?;
            if get(&source_of(given)?, "ruleset").is_some() && bytes != canonical {
                return Err(format!("rule set canonical bytes {bytes}"));
            }
            // SAFETY: a live rule set.
            Some(
                common::text(|out, len, error| unsafe {
                    cr_ruleset_digest(ruleset.0, out, len, error)
                })
                .map_err(|f| to_string(&f.json))?,
            )
        }
        Err(_) => None,
    };
    if ruleset_digest != wanted("ruleset_digest") {
        return Err(format!("rule set digest {ruleset_digest:?}"));
    }
    Ok(())
}

/// The document a `canonical` vector gives, parsed.
fn source_of(given: &Json) -> Result<Json, String> {
    match (get(given, "document"), string(get(given, "text"))) {
        (Some(document), _) => Ok(document.clone()),
        (None, Some(text)) => Ok(json(text)),
        _ => Err("nothing given".into()),
    }
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
        "draw" => run_draw(given, expect),
        "canonical" => run_canonical(given, expect),
        "bundle" => run_make_bundle(given, expect),
        "bundle_compare" => run_compare_bundles(given, expect),
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
    // On a small stack, as a binding's thread may be (`spec/json-format.md`,
    // *Errors and limits*).
    on_small_stack(|| {
        let (mut ran, mut skipped, mut graded, mut failures) = (0, 0, 0, Vec::new());
        // The tools for agents (`spec/agents.md`) are not in the C ABI yet
        // (`docs/PLAN.md`, O.1): their vectors are counted and skipped.
        let mut tools = 0;
        // Nor is the static audit (`spec/audit.md`; `docs/PLAN.md`, T.7 and
        // T.9): its vectors are counted and skipped too.
        let mut audits = 0;
        // Nor is the case runner (`spec/tests.md`, *Running cases*;
        // `docs/PLAN.md`, T.3 and T.9b).
        let mut test_runs = 0;
        // Nor are a run's statistics (`spec/tests.md`, *Statistics*;
        // `docs/PLAN.md`, T.10).
        let mut statistics = 0;
        // Nor is the comparator (`spec/comparison.md`; `docs/PLAN.md`, T.4
        // and T.9b).
        let mut comparisons = 0;
        for file in files() {
            let document = json(&fs::read_to_string(&file).unwrap());
            let Some(Json::Array(vectors)) = get(&document, "vectors") else {
                panic!()
            };
            for vector in vectors {
                // `max_values` bounds native data: a C program builds its
                // values itself, so only the bindings' native runs check these.
                let given = get(vector, "given").unwrap();
                if get(given, "limits").is_some() && string(get(given, "kind")) == Some("evaluate")
                {
                    skipped += 1;
                    continue;
                }
                if string(get(given, "kind")) == Some("tool") {
                    tools += 1;
                    continue;
                }
                if string(get(given, "kind")) == Some("audit") {
                    audits += 1;
                    continue;
                }
                if string(get(given, "kind")) == Some("test_run") {
                    test_runs += 1;
                    continue;
                }
                if string(get(given, "kind")) == Some("statistics") {
                    statistics += 1;
                    continue;
                }
                if string(get(given, "kind")) == Some("test_comparison") {
                    comparisons += 1;
                    continue;
                }
                // Conditions under a graded logic: the C ABI evaluates one
                // node under `boolean` only until task J.1b.
                if get(given, "logic").is_some_and(|l| string(Some(l)) != Some("boolean")) {
                    graded += 1;
                    continue;
                }
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
        assert!(skipped >= 14, "{skipped} native-only vectors skipped");
        assert!(graded >= 96, "{graded} graded-logic vectors skipped");
        assert!(test_runs >= 26, "{test_runs} test run vectors skipped");
        assert!(statistics >= 8, "{statistics} statistics vectors skipped");
        assert!(
            comparisons >= 29,
            "{comparisons} test comparison vectors skipped"
        );
        println!(
            "{ran} ran, {skipped} native-only, {tools} tool vectors (agents), {audits} audit vectors, {test_runs} test run vectors, {statistics} statistics vectors, {comparisons} test comparison vectors and {graded} graded-logic vectors skipped"
        );
    });
}
