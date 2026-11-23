//! The JSON form of a test run (`spec/tests.md`, *Test runs*), output only.

use crate::bundle::{ENGINE_NAME, ENGINE_VERSION};
use crate::cases::{CaseResult, ExpectedOutput, Mismatch, Summary, TestRun};
use crate::format::{write_error, write_value};
use crate::json::{Json, JsonNumber};
use crate::value::Map;

fn string(text: &str) -> Json {
    Json::String(text.to_owned())
}

fn number(n: usize) -> Json {
    Json::Number(JsonNumber::from_literal(n.to_string()))
}

fn strings<S: AsRef<str>>(items: impl IntoIterator<Item = S>) -> Json {
    Json::Array(
        items
            .into_iter()
            .map(|s| Json::String(s.as_ref().to_owned()))
            .collect(),
    )
}

/// An output: payload keys taken literally, values in the JSON encoding.
pub(crate) fn write_output(map: &Map) -> Json {
    Json::Object(
        map.iter()
            .map(|(key, value)| (key.to_owned(), write_value(value)))
            .collect(),
    )
}

pub(crate) fn write_expected_output(expected: &ExpectedOutput) -> Json {
    match expected {
        ExpectedOutput::Includes(map) => write_output(map),
        ExpectedOutput::NothingSelected => Json::Null,
    }
}

/// The content of a test run document: the object under `"test_run"`.
#[must_use]
pub fn write_test_run(run: &TestRun) -> Json {
    let mut members = vec![
        ("source".to_owned(), string(run.source.as_str())),
        ("hit_policy".to_owned(), string(run.hit_policy.as_str())),
    ];
    if let Some(aggregate) = run.aggregate {
        members.push(("aggregate".into(), string(aggregate.as_str())));
    }
    members.push(("logic".into(), string(run.logic.as_str())));
    if run.strict {
        members.push(("strict".into(), Json::Bool(true)));
    }
    if let Some(digest) = run.ruleset_digest {
        members.push(("ruleset_digest".into(), string(&digest.to_string())));
    }
    members.push((
        "engine".into(),
        Json::Object(vec![
            ("name".into(), string(ENGINE_NAME)),
            ("version".into(), string(ENGINE_VERSION)),
            ("limits".into(), run.engine_limits.to_json()),
        ]),
    ));
    if let Some((id, revision)) = &run.bundle {
        members.push((
            "bundle".into(),
            Json::Object(vec![
                ("id".into(), string(id.as_str())),
                ("revision".into(), string(revision.as_str())),
            ]),
        ));
    }
    let files = run
        .files
        .iter()
        .map(|file| {
            let mut entry = Vec::new();
            if let Some(path) = &file.path {
                entry.push(("path".to_owned(), string(path)));
            }
            if let Some(digest) = file.digest {
                entry.push(("digest".to_owned(), string(&digest.to_string())));
            }
            Json::Object(entry)
        })
        .collect();
    members.push(("files".into(), Json::Array(files)));
    let mut limits = vec![("max_explained".to_owned(), number(run.limits.max_explained))];
    if let Some(max) = run.limits.max_results {
        limits.push(("max_results".into(), number(max)));
    }
    if let Some(max) = run.limits.max_steps {
        limits.push(("max_steps".into(), number(max)));
    }
    members.push(("limits".into(), Json::Object(limits)));
    members.push(("summary".into(), write_summary(&run.summary)));
    members.push((
        "results".into(),
        Json::Array(run.results.iter().map(write_case_result).collect()),
    ));
    Json::Object(members)
}

fn write_summary(summary: &Summary) -> Json {
    let mut members = vec![
        ("total".to_owned(), number(summary.total())),
        ("passed".to_owned(), number(summary.passed)),
        ("failed".to_owned(), number(summary.failed)),
        ("error".to_owned(), number(summary.error)),
        ("partial".to_owned(), Json::Bool(summary.is_partial())),
    ];
    if let Some(interrupted) = summary.interrupted {
        members.push((
            "interrupted".into(),
            Json::Object(vec![
                ("reason".into(), string(interrupted.reason.as_str())),
                ("remaining".into(), number(interrupted.remaining)),
            ]),
        ));
    }
    Json::Object(members)
}

/// One result of a test run (`spec/tests.md`, *Test runs*), as a test run
/// document writes it.
#[must_use]
pub fn write_case_result(result: &CaseResult) -> Json {
    let mut members = Vec::new();
    if let Some(path) = &result.path {
        members.push(("path".to_owned(), string(path)));
    }
    if let Some(id) = &result.id {
        members.push(("id".to_owned(), string(id)));
    }
    members.push(("status".into(), string(result.status.as_str())));
    if !result.mismatches.is_empty() {
        members.push((
            "mismatches".into(),
            Json::Array(result.mismatches.iter().map(write_mismatch).collect()),
        ));
    }
    if let Some(error) = &result.error {
        members.push(("error".into(), write_error(error)));
    }
    if let Some(text) = &result.explanation_text {
        members.push(("explanation_text".into(), string(text)));
    }
    Json::Object(members)
}

/// What a mismatch expected and, unless nothing was given, what was given.
pub(crate) fn mismatch_values(mismatch: &Mismatch) -> (Json, Option<Json>) {
    match mismatch {
        Mismatch::Selected { expected, actual } | Mismatch::Unknown { expected, actual } => {
            (strings(expected), Some(strings(actual)))
        }
        Mismatch::Output { expected, actual } => (
            write_expected_output(expected),
            Some(actual.as_ref().map_or(Json::Null, write_output)),
        ),
        Mismatch::NoSingleOutput { expected, .. } => (write_expected_output(expected), None),
        Mismatch::Outputs { expected, actual } => (
            Json::Array(expected.iter().map(write_output).collect()),
            Some(Json::Array(actual.iter().map(write_output).collect())),
        ),
        Mismatch::Error { expected, actual } => (
            Json::Object(vec![("code".into(), string(expected.as_str()))]),
            Some(actual.as_ref().map_or(Json::Null, write_error)),
        ),
    }
}

fn write_mismatch(mismatch: &Mismatch) -> Json {
    let (expected, actual) = mismatch_values(mismatch);
    let mut members = vec![
        ("key".to_owned(), string(mismatch.key().as_str())),
        ("expected".to_owned(), expected),
    ];
    if let Some(actual) = actual {
        members.push(("actual".into(), actual));
    }
    if let Some(reason) = mismatch.reason() {
        members.push(("reason".into(), string(&reason)));
    }
    Json::Object(members)
}
