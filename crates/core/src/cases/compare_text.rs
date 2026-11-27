//! The text of a test comparison (`spec/comparison.md`, *Text*), as the
//! command line prints it. Fixed by snapshot tests.

use std::fmt;

use super::compare::{
    CaseOutcome, ComparedItem, ComparedRun, SingleOutput, TestComparison, Unstated,
};
use crate::bundle::Relation;
use crate::error::Error;
use crate::format::test_run::{strings, write_output};
use crate::json::{Json, to_string};
use crate::rules::RulesSource;

/// The indentation of what follows an item's first line.
const DETAIL: &str = "            ";

fn plural(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

/// The relation in words.
fn relation_words(relation: Relation) -> &'static str {
    match relation {
        Relation::Conflict => "a conflict",
        Relation::SameRulesSameEngine => "same rules, same engine",
        Relation::SameRulesOtherEngine => "same rules, other engine",
        Relation::OtherRulesSameEngine => "other rules, same engine",
        Relation::OtherRulesOtherEngine => "other rules, other engine",
    }
}

/// A side's header line, after `a: ` or `b: `.
fn write_side(f: &mut fmt::Formatter<'_>, name: &str, run: &ComparedRun) -> fmt::Result {
    write!(f, "{name}: ")?;
    if let Some((id, revision)) = &run.bundle {
        write!(f, "bundle {} revision {}, ", id.as_str(), revision.as_str())?;
    }
    let what = match run.source {
        RulesSource::RuleSet => "a rule set",
        RulesSource::DecisionTable => "a decision table",
    };
    write!(
        f,
        "{what}, {}, hit policy {}",
        plural(run.rule_count, "rule", "rules"),
        run.hit_policy.as_str()
    )?;
    if let Some(aggregate) = run.aggregate {
        write!(f, ", aggregate {}", aggregate.as_str())?;
    }
    write!(f, ", logic {}", run.logic.as_str())?;
    if run.strict {
        f.write_str(", strict")?;
    }
    writeln!(f, ", {}", run.ruleset_digest)
}

/// An error's code, location, rules and message.
fn error_text(error: &Error) -> String {
    let context = error.context();
    let location = if let Some(pointer) = &context.pointer {
        if pointer.is_empty() {
            " at the root".to_owned()
        } else {
            format!(" at {pointer}")
        }
    } else if let (Some(line), Some(column)) = (context.line, context.column) {
        format!(" at line {line}, column {column}")
    } else {
        String::new()
    };
    // The rules, which two errors of the same code may differ in.
    let rules = if let Some(rule) = &context.rule {
        format!(" in rule {}", to_string(&Json::String(rule.clone())))
    } else if context.rules.is_empty() {
        String::new()
    } else {
        format!(" in rules {}", to_string(&strings(&context.rules)))
    };
    format!(
        "{}{location}{rules}: {}",
        error.code().as_str(),
        error.message()
    )
}

/// What a side gave, on one line.
fn outcome_text(outcome: &CaseOutcome) -> String {
    let mut parts = vec![outcome.status.as_str().to_owned()];
    if let Some(decision) = &outcome.decision {
        parts.push(format!(
            "selected {}",
            to_string(&strings(&decision.selected))
        ));
        match &decision.output {
            SingleOutput::Output(map) => {
                parts.push(format!("output {}", to_string(&write_output(map))));
            }
            SingleOutput::NothingSelected => parts.push("output null".to_owned()),
            SingleOutput::NoSingleOutput => {
                let outputs = Json::Array(decision.outputs.iter().map(write_output).collect());
                parts.push(format!("outputs {}", to_string(&outputs)));
            }
        }
        if !decision.unknown.is_empty() {
            parts.push(format!(
                "unknown {}",
                to_string(&strings(&decision.unknown))
            ));
        }
    }
    if let Some(error) = &outcome.error {
        parts.push(error_text(error));
    }
    parts.join(", ")
}

fn write_item(f: &mut fmt::Formatter<'_>, item: &ComparedItem) -> fmt::Result {
    let label = match (item.kind.is_change(), item.expected) {
        (true, Some(true)) => "EXPECTED".to_owned(),
        (true, _) => "UNEXPECTED".to_owned(),
        (false, _) => item.kind.as_str().to_ascii_uppercase(),
    };
    // The id quoted, as a JSON string: ids may hold spaces.
    let names: Vec<String> = item
        .path
        .iter()
        .cloned()
        .chain(
            item.id
                .iter()
                .map(|id| to_string(&Json::String(id.clone()))),
        )
        .collect();
    if names.is_empty() {
        writeln!(f, "{label:<12}(a case file without a path)")?;
    } else {
        writeln!(f, "{label:<12}{}", names.join("  "))?;
    }
    if item.kind.is_change() {
        let parts: Vec<&str> = item.differences.iter().map(|part| part.as_str()).collect();
        writeln!(f, "{DETAIL}{} ({})", item.kind.as_str(), parts.join(", "))?;
        if item.id.is_none() {
            writeln!(f, "{DETAIL}a case file has no expectation")?;
        } else if item.unstated.contains(&Unstated::Status) {
            writeln!(f, "{DETAIL}b did not pass its case")?;
        } else if !item.unstated.is_empty() {
            let parts: Vec<&str> = item.unstated.iter().map(|why| why.as_str()).collect();
            writeln!(
                f,
                "{DETAIL}b's expectation does not check: {}",
                parts.join(", ")
            )?;
        }
    }
    for (name, outcome) in [("a", &item.a), ("b", &item.b)] {
        if let Some(outcome) = outcome {
            writeln!(f, "{DETAIL}{name}: {}", outcome_text(outcome))?;
        }
    }
    for (name, outcome) in [("a", &item.a), ("b", &item.b)] {
        if let Some(text) = outcome.as_ref().and_then(|o| o.explanation_text.as_ref()) {
            writeln!(f, "{DETAIL}{name} explanation:")?;
            for line in text.lines() {
                writeln!(f, "{DETAIL}  {line}")?;
            }
        }
    }
    Ok(())
}

impl fmt::Display for TestComparison {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(
            f,
            "Comparison of two test runs: {} ({})",
            relation_words(self.relation),
            self.relation.as_str()
        )?;
        write_side(f, "a", &self.a)?;
        write_side(f, "b", &self.b)?;
        if let Some(bundles) = &self.bundles {
            write!(f, "bundles: {}", bundles.relation().as_str())?;
            if bundles.is_identical() {
                writeln!(f, ", identical")?;
            } else {
                let differences: Vec<&str> =
                    bundles.differences().iter().map(|d| d.as_str()).collect();
                writeln!(f, ", differences {}", differences.join(", "))?;
            }
        }
        let limits: Vec<String> = self
            .engine_limit_differences()
            .into_iter()
            .map(|(limit, a, b)| format!("{} a {a}, b {b}", limit.as_str()))
            .collect();
        if !limits.is_empty() {
            writeln!(f, "engine limits differ: {}", limits.join("; "))?;
        }
        if self.relation == Relation::OtherRulesOtherEngine {
            writeln!(
                f,
                "note: both the rules and the engine differ, so a change may come from either"
            )?;
        }
        for item in &self.items {
            write_item(f, item)?;
        }
        let summary = &self.summary;
        writeln!(
            f,
            "{}: {} unchanged, {} changed ({} expected, {} unexpected), {} added, {} removed, {} not compared",
            plural(summary.total(), "item", "items"),
            summary.unchanged,
            summary.changed(),
            summary.expected,
            summary.unexpected,
            summary.added,
            summary.removed,
            summary.not_compared,
        )?;
        for (name, run) in [("a", &self.a), ("b", &self.b)] {
            if let Some(interrupted) = run.summary.interrupted {
                writeln!(
                    f,
                    "Partial run of {name}: interrupted by {}, {} not run",
                    interrupted.reason.as_str(),
                    interrupted.remaining
                )?;
            }
        }
        Ok(())
    }
}
