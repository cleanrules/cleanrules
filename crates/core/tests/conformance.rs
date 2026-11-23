//! The core's conformance runner (`conformance/README.md`).
//!
//! Runs every vector of the kinds this version of the core implements and
//! reports each failure with its id. Kinds still to come (rule sets, decisions,
//! explanations, summaries) are counted as pending.

// A test: a panic is the failure report.
#![allow(clippy::unwrap_used, clippy::panic)]

mod common;

use std::collections::BTreeMap;
use std::fs;

use cleanrules::Error;
use cleanrules::agents::Catalog;
use cleanrules::audit::{AuditLimits, audit_document};
use cleanrules::bundle::{self, Bundle, BundleId, BundleRules, CaseFileRef, Revision};
use cleanrules::cases::{CaseFile, Runner, TestRunLimits};
use cleanrules::eval::{Options, evaluate_value_tree};
use cleanrules::explain::{explain, summary};
use cleanrules::format::{
    Document, DocumentFormat, read_decision_document, read_document, read_document_as,
    read_parsed_document, write_document, write_document_as, write_explanation, write_summary,
};
use cleanrules::format::{
    ValueContext, read_node, read_ruleset, read_value, write_decision, write_path, write_result,
};
use cleanrules::format::{read_logic, write_any_result};
use cleanrules::json::{Json, Limits, parse, to_string};
use cleanrules::logic::{AnyLogic, Kleene};
use cleanrules::visual::{DiagramFormat, draw_document, draw_explanation};
use common::{get, on_small_stack, path, same, text, vector_files};

/// Remove `detail` from reasons, which is never compared.
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

/// The compared fields of an error read from a document: those of
/// [`error_json`] and its pointer.
fn read_error_json(error: &Error) -> Json {
    let mut json = error_json(error);
    if let (Json::Object(members), Some(pointer)) = (&mut json, &error.context().pointer) {
        members.push(("pointer".into(), Json::String(pointer.clone())));
    }
    json
}

/// The compared fields of an error (`spec/errors.md`, *Comparing errors*).
fn error_json(error: &Error) -> Json {
    let context = error.context();
    let mut members = vec![(
        "code".to_owned(),
        Json::String(error.code().as_str().into()),
    )];
    if let Some(rule) = &context.rule {
        members.push(("rule".into(), Json::String(rule.clone())));
    }
    if let Some(path) = &context.path {
        members.push(("path".into(), write_path(path)));
    }
    if !context.types.is_empty() {
        let types = context
            .types
            .iter()
            .map(|g| Json::String(g.as_str().into()))
            .collect();
        members.push(("types".into(), Json::Array(types)));
    }
    if !context.rules.is_empty() {
        let rules = context
            .rules
            .iter()
            .map(|r| Json::String(r.clone()))
            .collect();
        members.push(("rules".into(), Json::Array(rules)));
    }
    if let Some(name) = &context.name {
        members.push(("name".into(), Json::String(name.clone())));
    }
    if let Some(row) = context.row {
        members.push((
            "row".into(),
            Json::Number(cleanrules::json::JsonNumber::from_literal(row.to_string())),
        ));
    }
    Json::Object(members)
}

/// Compare error objects without regard to member order.
fn same_error(actual: &Json, expected: &Json) -> bool {
    let (Json::Object(a), Json::Object(b)) = (actual, expected) else {
        return false;
    };
    a.len() == b.len()
        && b.iter()
            .all(|(k, v)| a.iter().any(|(l, w)| l == k && same(w, v)))
}

fn compare(actual: Result<Json, Error>, expect: &Json, key: &str) -> Result<(), String> {
    compare_with(actual, expect, key, error_json)
}

fn compare_with(
    actual: Result<Json, Error>,
    expect: &Json,
    key: &str,
    error_json: fn(&Error) -> Json,
) -> Result<(), String> {
    match (actual, get(expect, "error")) {
        (Ok(json), None) => {
            let actual = strip_detail(&json);
            let wanted = strip_detail(get(expect, key).unwrap());
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
            "got {}, expected the error {}",
            to_string(&json),
            to_string(error)
        )),
        (Err(error), None) => Err(format!(
            "unexpected error {}: {error}",
            to_string(&error_json(&error))
        )),
        (Err(error), Some(wanted)) => {
            let actual = error_json(&error);
            if same_error(&actual, wanted) {
                Ok(())
            } else {
                Err(format!(
                    "got the error {}, expected {}",
                    to_string(&actual),
                    to_string(wanted)
                ))
            }
        }
    }
}

fn run_evaluate(vector: &Json) -> Result<(), String> {
    let given = get(vector, "given").unwrap();
    let limits = Limits::default();
    let ruleset = read_ruleset(get(given, "ruleset").unwrap(), "/ruleset", &limits)
        .map_err(|e| format!("reading the rule set: {e} at {:?}", e.context().pointer))?;
    let expect = get(vector, "expect").unwrap();
    if let Some(Json::Array(items)) = get(given, "inputs") {
        let inputs = items
            .iter()
            .map(|i| read_value(i, "/inputs", ValueContext::Data))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| format!("reading the inputs: {e}"))?;
        let actual = ruleset
            .evaluate_many(&inputs)
            .map(|decisions| Json::Array(decisions.iter().map(write_decision).collect()));
        return compare(actual, expect, "decisions");
    }
    let input = read_value(get(given, "input").unwrap(), "/input", ValueContext::Data)
        .map_err(|e| format!("reading the input: {e}"))?;
    let decision = ruleset.evaluate(&input);
    if let Ok(decision) = &decision {
        // Every decision the core produces reads back, passes the checks of
        // *Reading a decision*, and is written the same way.
        let written = write_decision(decision);
        let document = Json::Object(vec![
            (
                "version".into(),
                Json::Number(cleanrules::json::JsonNumber::from_literal("1")),
            ),
            ("decision".into(), written.clone()),
        ]);
        let read =
            read_decision_document(&to_string(&document), &ruleset, &limits).map_err(|e| {
                format!(
                    "the decision does not read back: {e} at {:?}",
                    e.context().pointer
                )
            })?;
        if !same(&write_decision(&read), &written) {
            return Err("the decision read back is written differently".into());
        }
    }
    compare(decision.map(|d| write_decision(&d)), expect, "decision")
}

fn run_explain(vector: &Json) -> Result<(), String> {
    let given = get(vector, "given").unwrap();
    let ruleset = read_ruleset(
        get(given, "ruleset").unwrap(),
        "/ruleset",
        &Limits::default(),
    )
    .map_err(|e| format!("reading the rule set: {e}"))?;
    let expect = get(vector, "expect").unwrap();
    if text(get(given, "kind")) == Some("summary") {
        return compare(Ok(write_summary(&summary(&ruleset))), expect, "summary");
    }
    let input = read_value(get(given, "input").unwrap(), "/input", ValueContext::Data)
        .map_err(|e| format!("reading the input: {e}"))?;
    let decision = ruleset.evaluate(&input);
    if let Ok(d) = &decision {
        // A decision read back from JSON explains exactly as the evaluated one.
        let document = Json::Object(vec![
            (
                "version".into(),
                Json::Number(cleanrules::json::JsonNumber::from_literal("1")),
            ),
            ("decision".into(), write_decision(d)),
        ]);
        let read = read_decision_document(&to_string(&document), &ruleset, &Limits::default())
            .map_err(|e| format!("the decision does not read back: {e}"))?;
        if !same(
            &write_explanation(&explain(&read)),
            &write_explanation(&explain(d)),
        ) {
            return Err("the decision read back explains differently".into());
        }
    }
    let (decision_json, explanation_json) = match &decision {
        Ok(d) => (Ok(write_decision(d)), Ok(write_explanation(&explain(d)))),
        Err(e) => (
            Err(cleanrules::Error::new(e.code(), e.message())),
            Err(cleanrules::Error::new(e.code(), e.message())),
        ),
    };
    compare(decision_json, expect, "decision")?;
    compare(explanation_json, expect, "explanation")
}

/// Members of an agent error that are free text or not compared
/// (`conformance/README.md`, *Comparing*).
const AGENT_ERROR_FREE: [&str; 6] = ["message", "hint", "cause", "node", "operation", "reason"];

/// An agent error without its free text, after checking that it has a hint
/// (`spec/agents.md`, *Errors*).
fn agent_error_compared(error: &Json) -> Result<Json, String> {
    match get(error, "hint") {
        Some(Json::String(hint)) if !hint.is_empty() => {}
        _ => return Err(format!("the agent error {} has no hint", to_string(error))),
    }
    let Json::Object(members) = error else {
        return Err(format!(
            "the agent error {} is not an object",
            to_string(error)
        ));
    };
    Ok(Json::Object(
        members
            .iter()
            .filter(|(k, _)| !AGENT_ERROR_FREE.contains(&k.as_str()))
            .cloned()
            .collect(),
    ))
}

/// A `tool` vector (`spec/agents.md`): a call on a catalog of the given rule
/// sets, compared with `output` or with `error`.
fn run_tool(vector: &Json) -> Result<(), String> {
    let given = get(vector, "given").unwrap();
    let expect = get(vector, "expect").unwrap();
    let mut catalog = Catalog::new();
    if let Some(Json::Object(rulesets)) = get(given, "rulesets") {
        for (name, content) in rulesets {
            let pointer = format!("/rulesets/{}", cleanrules::json::pointer_token(name));
            let ruleset = read_ruleset(content, &pointer, &Limits::default())
                .map_err(|e| format!("reading the rule set {name}: {e}"))?;
            catalog.insert(name.clone(), ruleset);
        }
    }
    let tool = text(get(given, "tool")).unwrap();
    let result = match get(given, "arguments_text") {
        Some(Json::String(arguments)) => catalog.call_text(tool, arguments),
        _ => catalog.call(tool, get(given, "arguments").unwrap()),
    };
    match (result.is_error, get(expect, "error")) {
        (true, Some(wanted)) => {
            let actual = agent_error_compared(get(&result.content, "error").unwrap())?;
            if same_error(&actual, wanted) {
                Ok(())
            } else {
                Err(format!(
                    "got the error {}, expected {}",
                    to_string(&actual),
                    to_string(wanted)
                ))
            }
        }
        (false, None) => {
            let mut actual = result.content.clone();
            // A `validate` result's error is compared without its free text.
            if let Json::Object(members) = &mut actual {
                for (key, value) in members.iter_mut() {
                    if key == "error" {
                        *value = agent_error_compared(value)?;
                    }
                }
            }
            let wanted = get(expect, "output").unwrap();
            if same(&actual, wanted) {
                Ok(())
            } else {
                Err(format!(
                    "got {}\nwant {}",
                    to_string(&actual),
                    to_string(wanted)
                ))
            }
        }
        (true, None) => Err(format!(
            "unexpected tool error {}",
            to_string(&result.content)
        )),
        (false, Some(wanted)) => Err(format!(
            "got {}, expected the error {}",
            to_string(&result.content),
            to_string(wanted)
        )),
    }
}

/// A diagram (`spec/visual.md`): of the document, or, with an input, of the
/// explanation of a rule set's decision or of a table's grid with its statuses.
fn run_draw(vector: &Json) -> Result<(), String> {
    let given = get(vector, "given").unwrap();
    let mut limits = Limits::default();
    if let Some(Json::Number(n)) = path(given, &["limits", "max_diagram_size"]) {
        limits.max_diagram_size = usize::try_from(n.as_i128().unwrap()).unwrap();
    }
    let format = DiagramFormat::from_name(text(get(given, "format")).unwrap()).unwrap();
    let document = read_parsed_document(get(given, "document").unwrap(), &Limits::default())
        .map_err(|e| format!("reading the document: {e}"))?;
    let drawn = match (get(given, "input"), &document) {
        (None, document) => draw_document(document, None, format, &limits),
        (Some(input), Document::RuleSet(ruleset)) => {
            let input = read_value(input, "/input", ValueContext::Data)
                .map_err(|e| format!("reading the input: {e}"))?;
            let decision = ruleset.evaluate(&input).map_err(|e| e.to_string())?;
            draw_explanation(&explain(&decision), format, &limits)
        }
        (Some(input), Document::DecisionTable(table)) => {
            let input = read_value(input, "/input", ValueContext::Data)
                .map_err(|e| format!("reading the input: {e}"))?;
            let decision = table
                .ruleset()
                .evaluate(&input)
                .map_err(|e| e.to_string())?;
            draw_document(&document, Some(&decision), format, &limits)
        }
        (Some(_), _) => return Err("an input is given with a rule set or a table only".into()),
    };
    let expect = get(vector, "expect").unwrap();
    match (drawn, get(expect, "error")) {
        (Ok(diagram), None) => {
            let wanted = text(get(expect, "diagram")).unwrap();
            if diagram == wanted {
                Ok(())
            } else {
                Err(format!("got\n{diagram}\nwant\n{wanted}"))
            }
        }
        (Ok(diagram), Some(error)) => Err(format!(
            "got\n{diagram}\nexpected the error {}",
            to_string(error)
        )),
        (Err(error), wanted) => compare(
            Err(error),
            &Json::Object(
                wanted
                    .map(|w| vec![("error".to_owned(), w.clone())])
                    .unwrap_or_default(),
            ),
            "diagram",
        ),
    }
}

/// A `canonical` vector: the document's canonical bytes, its digest and, for
/// the documents that give a rule set, its rule set digest
/// (`spec/json-format.md`, *Canonical bytes*, *Digests*).
fn run_canonical(vector: &Json) -> Result<(), String> {
    let given = get(vector, "given").unwrap();
    let expect = get(vector, "expect").unwrap();
    let source = text(get(given, "text"))
        .map_or_else(|| to_string(get(given, "document").unwrap()), str::to_owned);
    let document = read_document(&source, &Limits::default())
        .map_err(|e| format!("reading the document: {e}"))?;
    let mut problems = Vec::new();
    let bytes = String::from_utf8(document.canonical_bytes()).unwrap();
    if Some(bytes.as_str()) != text(get(expect, "canonical")) {
        problems.push(format!("canonical bytes {bytes}"));
    }
    let digest = document.digest().to_string();
    if Some(digest.as_str()) != text(get(expect, "digest")) {
        problems.push(format!("digest {digest}"));
    }
    let ruleset_digest = document.into_ruleset().ok().map(|r| r.digest().to_string());
    if ruleset_digest.as_deref() != text(get(expect, "ruleset_digest")) {
        problems.push(format!("rule set digest {ruleset_digest:?}"));
    }
    if problems.is_empty() {
        Ok(())
    } else {
        Err(problems.join("; "))
    }
}

/// A copy of a bundle document with this engine's version, for the vectors
/// whose bundle this engine made (`conformance/README.md`, `current_engine`).
fn with_this_engine(document: &Json) -> Json {
    let mut copy = document.clone();
    if let Json::Object(top) = &mut copy
        && let Some((_, Json::Object(bundle))) = top.iter_mut().find(|(k, _)| k == "bundle")
        && let Some((_, Json::Object(engine))) = bundle.iter_mut().find(|(k, _)| k == "engine")
        && let Some((_, version)) = engine.iter_mut().find(|(k, _)| k == "version")
    {
        *version = Json::String(bundle::ENGINE_VERSION.into());
    }
    copy
}

/// A `read` vector of a bundle (`spec/bundles.md`) or a case file
/// (`spec/tests.md`): read, then written back, or written in the format of
/// `write`.
fn run_read_bundle(vector: &Json) -> Result<(), String> {
    let given = get(vector, "given").unwrap();
    let current = matches!(get(given, "current_engine"), Some(Json::Bool(true)));
    let fix = |json: &Json| {
        if current {
            with_this_engine(json)
        } else {
            json.clone()
        }
    };
    let limits = Limits::default();
    let read = match (get(given, "document"), text(get(given, "yaml"))) {
        (Some(document), _) => read_document(&to_string(&fix(document)), &limits),
        (None, Some(yaml)) => read_document_as(yaml.as_bytes(), DocumentFormat::Yaml, &limits),
        _ => return Err("nothing given".into()),
    };
    let mut expect = get(vector, "expect").unwrap().clone();
    if let Json::Object(members) = &mut expect
        && let Some((_, document)) = members.iter_mut().find(|(k, _)| k == "document")
    {
        *document = fix(document);
    }
    if let Some(name) = text(get(given, "write")) {
        let format = DocumentFormat::from_name(name).unwrap();
        let written = read
            .and_then(|document| write_document_as(&document, format))
            .map(Json::String);
        return compare_with(written, &expect, "printed", read_error_json);
    }
    let written = read.map(|document| {
        let json = write_document(&document);
        // Writing in YAML gives the same document back.
        if cfg!(feature = "yaml") {
            let yaml = write_document_as(&document, DocumentFormat::Yaml).unwrap();
            let again = read_document_as(yaml.as_bytes(), DocumentFormat::Yaml, &limits).unwrap();
            assert_eq!(write_document(&again), json, "YAML round trip");
        }
        json
    });
    compare_with(written, &expect, "document", read_error_json)
}

/// A `bundle` vector: make a bundle, write it, read it back
/// (`spec/bundles.md`, *Making a bundle*).
fn run_make_bundle(vector: &Json) -> Result<(), String> {
    let given = get(vector, "given").unwrap();
    let mut limits = Limits::default();
    if let Some(Json::Object(members)) = get(given, "limits") {
        for (name, value) in members {
            let Json::Number(n) = value else { panic!() };
            let n = usize::try_from(n.as_i128().unwrap()).unwrap();
            match name.as_str() {
                "max_size" => limits.max_size = n,
                "max_nesting" => limits.max_nesting = n,
                "max_depth" => limits.max_depth = n,
                "max_path_segments" => limits.max_path_segments = n,
                "max_table_size" => limits.max_table_size = n,
                "max_values" => limits.max_values = n,
                other => panic!("unknown limit {other}"),
            }
        }
    }
    let rules: BundleRules = match read_document(
        &to_string(get(given, "document").unwrap()),
        &Limits::default(),
    )
    .map_err(|e| format!("reading the rules: {e}"))?
    {
        Document::RuleSet(ruleset) => ruleset.into(),
        Document::DecisionTable(table) => table.into(),
        _ => return Err("the rules are a rule set or a table".into()),
    };
    let cases = match get(given, "cases") {
        Some(Json::Array(items)) => items
            .iter()
            .map(|item| {
                CaseFileRef::new(
                    text(get(item, "path")).unwrap(),
                    text(get(item, "text")).unwrap().as_bytes(),
                )
            })
            .collect::<Result<Vec<_>, Error>>(),
        _ => Ok(Vec::new()),
    };
    let made = cases.and_then(|cases| {
        Bundle::new(
            rules,
            BundleId::new(text(get(given, "id")).unwrap())?,
            Revision::new(text(get(given, "revision")).unwrap())?,
        )
        .with_cases(cases)?
        .with_limits(&limits)
    });
    let mut expect = get(vector, "expect").unwrap().clone();
    if let Json::Object(members) = &mut expect
        && let Some((_, document)) = members.iter_mut().find(|(k, _)| k == "document")
    {
        *document = with_this_engine(document);
    }
    let written = made.map(|bundle| {
        let json = write_document(&bundle.into_document());
        // This engine reads what it made, verifying its digest and contract.
        let again = read_document(&to_string(&json), &Limits::default()).unwrap();
        assert_eq!(write_document(&again), json, "read back");
        json
    });
    compare(written, &expect, "document")
}

/// A `bundle_compare` vector (`spec/bundles.md`, *Comparing bundles*).
fn run_compare_bundles(vector: &Json) -> Result<(), String> {
    let given = get(vector, "given").unwrap();
    let read =
        |key: &str| match read_document(&to_string(get(given, key).unwrap()), &Limits::default()) {
            Ok(Document::Bundle(bundle)) => Ok(bundle),
            Ok(_) => Err(format!("{key} is not a bundle")),
            Err(e) => Err(format!("reading {key}: {e}")),
        };
    let (a, b) = (read("a")?, read("b")?);
    let comparison = a.compare(&b);
    // Symmetric.
    let back = b.compare(&a);
    if back.relation() != comparison.relation() || back.differences() != comparison.differences() {
        return Err("not symmetric".into());
    }
    compare(
        Ok(comparison.to_json()),
        get(vector, "expect").unwrap(),
        "comparison",
    )
}

/// An `audit` vector (`spec/audit.md`): the report, with each witness checked
/// by the rule set's own evaluation (`conformance/README.md`, *Comparing*),
/// then compared without the messages and witnesses of its diagnostics.
fn run_audit(vector: &Json) -> Result<(), String> {
    let given = get(vector, "given").unwrap();
    let mut limits = AuditLimits::default();
    if let Some(Json::Object(members)) = get(given, "limits") {
        for (key, value) in members {
            let Json::Number(n) = value else {
                return Err(format!("limit {key} is not a number"));
            };
            let n = usize::try_from(n.as_i128().unwrap()).unwrap();
            limits = match key.as_str() {
                "max_terms" => limits.with_max_terms(n),
                "max_steps" => limits.with_max_steps(n),
                other => return Err(format!("unknown limit {other}")),
            };
        }
    }
    let document = read_document(
        &to_string(get(given, "document").unwrap()),
        &Limits::default(),
    )
    .map_err(|e| format!("reading the document: {e}"))?;
    // Any document, through `audit_document` (`spec/audit.md`, *Documents*);
    // a document without rules is an expected error.
    let report = match audit_document(&document, &limits) {
        Ok(report) => report,
        Err(error) => return compare(Err(error), get(vector, "expect").unwrap(), "audit"),
    };
    let ruleset = &document
        .into_ruleset()
        .map_err(|e| format!("the rule set: {e}"))?;
    let report = report.with_ruleset_digest(matches!(
        get(given, "ruleset_digest"),
        Some(Json::Bool(true))
    ));
    for d in &report.diagnostics {
        let evaluated = d.witness.as_ref().map(|w| ruleset.evaluate(w));
        let fired = |decision: &cleanrules::rules::Decision<'_>, name: &str| {
            decision
                .results()
                .iter()
                .any(|r| r.fired && r.rule.name() == name)
        };
        let shown = match (d.code.as_str(), d.severity.as_str(), evaluated) {
            ("overlap", "error", Some(Err(e))) => {
                e.code().as_str() == "hit_policy_error"
                    && d.rules.iter().all(|r| e.context().rules.contains(r))
            }
            ("overlap", _, Some(Ok(decision))) => d.rules.iter().all(|r| fired(&decision, r)),
            ("gap", _, Some(Ok(decision))) => decision.results().iter().all(|r| !r.fired),
            ("overlap" | "gap", _, _) => false,
            (_, _, evaluated) => evaluated.is_none(),
        };
        if !shown {
            return Err(format!("the witness does not show {d:?}"));
        }
    }
    let mut written = cleanrules::format::write_audit(&report);
    if let Json::Object(members) = &mut written {
        for (key, value) in members.iter_mut() {
            if key == "diagnostics"
                && let Json::Array(items) = value
            {
                for item in items {
                    if let Json::Object(fields) = item {
                        fields.retain(|(k, _)| k != "message" && k != "witness");
                    }
                }
            }
        }
    }
    compare(Ok(written), get(vector, "expect").unwrap(), "audit")
}

/// The compared fields of an error written in its JSON form
/// (`spec/errors.md`, *Comparing errors*): the others are dropped.
fn compared_error(json: &mut Json) {
    const COMPARED: [&str; 11] = [
        "code", "pointer", "line", "column", "rule", "rules", "path", "types", "row", "name",
        "executed",
    ];
    if let Json::Object(members) = json {
        members.retain(|(k, _)| COMPARED.contains(&k.as_str()));
    }
}

/// Reduce every error of a written test run to its compared fields: a
/// result's `error`, and the `actual` of a mismatch of the key `error`.
fn compared_errors(written: &mut Json) {
    if let Json::Object(members) = written
        && let Some((_, Json::Array(results))) = members.iter_mut().find(|(k, _)| k == "results")
    {
        for result in results {
            let Json::Object(fields) = result else {
                continue;
            };
            for (key, value) in fields.iter_mut() {
                if key == "error" {
                    compared_error(value);
                }
                if key == "mismatches"
                    && let Json::Array(mismatches) = value
                {
                    for mismatch in mismatches {
                        let Json::Object(parts) = mismatch else {
                            continue;
                        };
                        let is_error = parts.iter().any(|(k, v)| {
                            k == "key" && matches!(v, Json::String(s) if s == "error")
                        });
                        for (k, v) in parts.iter_mut() {
                            if is_error && k == "actual" {
                                compared_error(v);
                            }
                        }
                    }
                }
            }
        }
    }
}

/// A `test_run` vector (`spec/tests.md`, *Running cases*): run the cases of
/// `cases` or `files` against the rules of `document`, then compare the test
/// run with its errors reduced to their compared fields.
fn run_test_run(vector: &Json) -> Result<(), String> {
    let given = get(vector, "given").unwrap();
    let limits = Limits::default();
    let document = read_document(&to_string(get(given, "document").unwrap()), &limits)
        .map_err(|e| format!("reading the rules: {e}"))?;
    let texts: Vec<(String, String)> = match get(given, "files") {
        Some(Json::Array(items)) => items
            .iter()
            .map(|item| {
                (
                    text(get(item, "path")).unwrap().to_owned(),
                    text(get(item, "text")).unwrap().to_owned(),
                )
            })
            .collect(),
        _ => Vec::new(),
    };
    let files: Vec<CaseFile> = match (&document, get(given, "cases")) {
        (_, Some(cases)) => match read_document(&to_string(cases), &limits) {
            Ok(Document::Cases(cases)) => vec![CaseFile::new(cases)],
            other => return Err(format!("not a case file: {other:?}")),
        },
        (Document::Bundle(bundle), None) => CaseFile::bundle_files(bundle, |r| {
            texts
                .iter()
                .find(|(path, _)| path == r.path())
                .map(|(_, text)| text.as_bytes().to_vec())
                .ok_or_else(|| format!("no file {}", r.path()))
        })?,
        (_, None) => texts
            .iter()
            .map(|(path, text)| CaseFile::read(path, text.as_bytes(), &limits))
            .collect(),
    };
    let mut run_limits = TestRunLimits::default();
    if let Some(Json::Object(members)) = get(given, "limits") {
        for (key, value) in members {
            let Json::Number(n) = value else {
                return Err(format!("limit {key} is not a number"));
            };
            let n = usize::try_from(n.as_i128().unwrap()).unwrap();
            run_limits = match key.as_str() {
                "max_results" => run_limits.with_max_results(n),
                "max_steps" => run_limits.with_max_steps(n),
                "max_explained" => run_limits.with_max_explained(n),
                other => return Err(format!("unknown limit {other}")),
            };
        }
    }
    let runner = match &document {
        Document::RuleSet(ruleset) => Runner::new(ruleset),
        Document::DecisionTable(table) => Runner::for_table(table),
        Document::Bundle(bundle) => Runner::for_bundle(bundle),
        _ => return Err("not rules".into()),
    }
    .with_limits(run_limits);
    let digest = matches!(get(given, "ruleset_digest"), Some(Json::Bool(true)));
    let report = match get(given, "cancel_after") {
        Some(Json::Number(n)) => {
            let after = usize::try_from(n.as_i128().unwrap()).unwrap();
            let mut asked = 0;
            runner.run_until(&files, || {
                asked += 1;
                asked > after
            })
        }
        _ => runner.run(&files),
    }
    .with_ruleset_digest(digest);
    let mut written = cleanrules::format::write_test_run(&report);
    compared_errors(&mut written);
    // The expected `engine.version` is this engine's, whichever it is
    // (`conformance/README.md`, `test_run`).
    let mut expect = get(vector, "expect").unwrap().clone();
    if let Json::Object(members) = &mut expect
        && let Some((_, Json::Object(run))) = members.iter_mut().find(|(k, _)| k == "test_run")
        && let Some((_, Json::Object(engine))) = run.iter_mut().find(|(k, _)| k == "engine")
        && let Some((_, version)) = engine.iter_mut().find(|(k, _)| k == "version")
    {
        *version = Json::String(cleanrules::bundle::ENGINE_VERSION.to_owned());
    }
    compare(Ok(written), &expect, "test_run")
}

fn run(vector: &Json) -> Result<(), String> {
    let given = get(vector, "given").unwrap();
    let kind = text(get(given, "kind")).unwrap();
    if kind == "read" {
        return run_read_bundle(vector);
    }
    if kind == "bundle" {
        return run_make_bundle(vector);
    }
    if kind == "bundle_compare" {
        return run_compare_bundles(vector);
    }
    if kind == "canonical" {
        return run_canonical(vector);
    }
    if kind == "draw" {
        return run_draw(vector);
    }
    if kind == "evaluate" {
        return run_evaluate(vector);
    }
    if kind == "explain" || kind == "summary" {
        return run_explain(vector);
    }
    if kind == "tool" {
        return run_tool(vector);
    }
    if kind == "audit" {
        return run_audit(vector);
    }
    if kind == "test_run" {
        return run_test_run(vector);
    }
    let limits = Limits::default();
    let node_key = if kind == "condition" {
        "condition"
    } else {
        "value"
    };
    let node = read_node(get(given, node_key).unwrap(), "", &limits)
        .map_err(|e| format!("reading the node: {e}"))?;
    let input = read_value(get(given, "input").unwrap(), "/input", ValueContext::Data)
        .map_err(|e| format!("reading the input: {e}"))?;
    let options = Options {
        strict: matches!(get(given, "strict"), Some(Json::Bool(true))),
        rule: None,
    };
    let expect = get(vector, "expect").unwrap();
    if kind == "value" {
        let result = evaluate_value_tree(&node, &input, &Kleene, options);
        return compare(result.map(|tree| write_result(&tree)), expect, "result");
    }
    // The logic in its JSON form (`spec/json-format.md`, *Logics*), boolean
    // when absent; an error reading it is the vector's result.
    let logic = get(given, "logic").map_or(Ok(AnyLogic::Boolean(Kleene)), |json| {
        read_logic(json, "/logic")
    });
    let result = logic
        .and_then(|logic| logic.evaluate_tree(&node, &input, options))
        .map(|tree| write_any_result(&tree));
    compare(result, expect, "result")
}

#[test]
fn conformance_vectors() {
    // On a small stack, as a binding's thread may be (`spec/json-format.md`,
    // *Errors and limits*).
    on_small_stack(|| {
        let mut passed = 0;
        let mut pending: BTreeMap<String, usize> = BTreeMap::new();
        let mut failures = Vec::new();
        for path in vector_files() {
            let file = parse(&fs::read_to_string(&path).unwrap(), &Limits::default()).unwrap();
            let Some(Json::Array(vectors)) = get(&file, "vectors") else {
                continue;
            };
            for vector in vectors {
                let given = get(vector, "given").unwrap();
                let kind = text(get(given, "kind")).unwrap();
                // `max_values` bounds native data, which the core never reads:
                // only the bindings' native runs check these vectors.
                if get(given, "limits").is_some() && kind == "evaluate" {
                    *pending.entry("native-only limits".to_owned()).or_default() += 1;
                    continue;
                }
                // The `read` vectors run through the C ABI (`crates/ffi`), but
                // for bundles, which only this runner makes and compares too,
                // and case files.
                let bundle_file = text(get(vector, "id")).is_some_and(|id| {
                    id.starts_with("format/bundles/") || id.starts_with("format/cases/")
                });
                if kind == "read"
                    && bundle_file
                    && !cfg!(feature = "yaml")
                    && get(given, "yaml").is_some()
                {
                    *pending
                        .entry("yaml without its feature".to_owned())
                        .or_default() += 1;
                    continue;
                }
                if kind == "read" && !bundle_file {
                    *pending.entry(kind.to_owned()).or_default() += 1;
                    continue;
                }
                if !matches!(
                    kind,
                    "read"
                        | "bundle"
                        | "bundle_compare"
                        | "condition"
                        | "value"
                        | "evaluate"
                        | "explain"
                        | "summary"
                        | "tool"
                        | "draw"
                        | "canonical"
                        | "audit"
                        | "test_run"
                ) {
                    *pending.entry(kind.to_owned()).or_default() += 1;
                    continue;
                }
                match run(vector) {
                    Ok(()) => passed += 1,
                    Err(problem) => {
                        failures.push(format!("{}: {problem}", text(get(vector, "id")).unwrap()));
                    }
                }
            }
        }
        println!(
            "{passed} passed, {} failed; pending kinds: {pending:?}",
            failures.len()
        );
        let shown: Vec<&String> = failures.iter().take(15).collect();
        assert!(
            failures.is_empty(),
            "{} failed, first ones:\n{}",
            failures.len(),
            shown
                .iter()
                .map(|s| s.as_str())
                .collect::<Vec<_>>()
                .join("\n")
        );
    });
}
