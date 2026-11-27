//! The JSON form of a test comparison (`spec/comparison.md`, *JSON*),
//! output only.

use super::test_run::{
    number, string, strings, write_engine_and_files, write_output, write_summary,
};
use crate::cases::{CaseOutcome, ComparedItem, ComparedRun, SingleOutput, TestComparison};
use crate::format::{write_error, write_statistics};
use crate::json::Json;

/// The content of a test comparison document: the object under
/// `"test_comparison"`.
#[must_use]
pub fn write_test_comparison(comparison: &TestComparison) -> Json {
    let mut members = vec![("relation".to_owned(), string(comparison.relation.as_str()))];
    if let Some(bundles) = &comparison.bundles {
        members.push(("bundles".into(), bundles.to_json()));
    }
    members.push(("a".into(), write_side(&comparison.a)));
    members.push(("b".into(), write_side(&comparison.b)));
    members.push((
        "limits".into(),
        Json::Object(vec![(
            "max_explained".into(),
            number(comparison.max_explained),
        )]),
    ));
    let summary = &comparison.summary;
    members.push((
        "summary".into(),
        Json::Object(vec![
            ("total".into(), number(summary.total())),
            ("unchanged".into(), number(summary.unchanged)),
            ("changed".into(), number(summary.changed())),
            ("expected".into(), number(summary.expected)),
            ("unexpected".into(), number(summary.unexpected)),
            ("added".into(), number(summary.added)),
            ("removed".into(), number(summary.removed)),
            ("not_compared".into(), number(summary.not_compared)),
            ("partial".into(), Json::Bool(summary.partial)),
            (
                "by_change".into(),
                Json::Object(
                    summary
                        .by_change
                        .iter()
                        .map(|(change, count)| (change.as_str().to_owned(), number(*count)))
                        .collect(),
                ),
            ),
        ]),
    ));
    members.push((
        "items".into(),
        Json::Array(comparison.items.iter().map(write_compared_item).collect()),
    ));
    Json::Object(members)
}

/// One side: its run's header and summary, and the counts of its outcomes.
fn write_side(run: &ComparedRun) -> Json {
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
    members.push((
        "ruleset_digest".into(),
        string(&run.ruleset_digest.to_string()),
    ));
    write_engine_and_files(
        &mut members,
        &run.engine_limits,
        run.bundle.as_ref(),
        &run.files,
    );
    let mut limits = Vec::new();
    if let Some(max) = run.limits.max_results {
        limits.push(("max_results".to_owned(), number(max)));
    }
    if let Some(max) = run.limits.max_steps {
        limits.push(("max_steps".to_owned(), number(max)));
    }
    members.push(("limits".into(), Json::Object(limits)));
    members.push(("summary".into(), write_summary(&run.summary)));
    members.push((
        "outcomes".into(),
        Json::Object(vec![
            ("decisions".into(), number(run.outcomes.decisions)),
            (
                "nothing_selected".into(),
                number(run.outcomes.nothing_selected),
            ),
            ("with_unknown".into(), number(run.outcomes.with_unknown)),
            ("error".into(), number(run.outcomes.error)),
        ]),
    ));
    if let Some(statistics) = &run.statistics {
        members.push(("statistics".into(), write_statistics(statistics)));
    }
    Json::Object(members)
}

/// One listed item of a comparison (`spec/comparison.md`, *JSON*).
#[must_use]
pub fn write_compared_item(item: &ComparedItem) -> Json {
    let mut members = Vec::new();
    if let Some(path) = &item.path {
        members.push(("path".to_owned(), string(path)));
    }
    if let Some(id) = &item.id {
        members.push(("id".to_owned(), string(id)));
    }
    members.push(("change".into(), string(item.kind.as_str())));
    if let Some(expected) = item.expected {
        members.push(("expected".into(), Json::Bool(expected)));
        members.push((
            "differences".into(),
            strings(item.differences.iter().map(|part| part.as_str())),
        ));
        if !item.unstated.is_empty() {
            members.push((
                "unstated".into(),
                strings(item.unstated.iter().map(|why| why.as_str())),
            ));
        }
    }
    for (side, outcome) in [("a", &item.a), ("b", &item.b)] {
        if let Some(outcome) = outcome {
            members.push((side.into(), write_outcome(outcome)));
        }
    }
    Json::Object(members)
}

fn write_outcome(outcome: &CaseOutcome) -> Json {
    let mut members = vec![("status".to_owned(), string(outcome.status.as_str()))];
    if let Some(decision) = &outcome.decision {
        members.push(("selected".into(), strings(&decision.selected)));
        match &decision.output {
            SingleOutput::Output(map) => members.push(("output".into(), write_output(map))),
            SingleOutput::NothingSelected => members.push(("output".into(), Json::Null)),
            SingleOutput::NoSingleOutput => {}
        }
        members.push((
            "outputs".into(),
            Json::Array(decision.outputs.iter().map(write_output).collect()),
        ));
        members.push(("unknown".into(), strings(&decision.unknown)));
    }
    if let Some(error) = &outcome.error {
        members.push(("error".into(), write_error(error)));
    }
    if let Some(text) = &outcome.explanation_text {
        members.push(("explanation_text".into(), string(text)));
    }
    Json::Object(members)
}
