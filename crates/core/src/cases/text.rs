//! The text of a test run (`spec/tests.md`, *Test runs*, *Text*), as the
//! command line prints it. Fixed by snapshot tests.

use std::fmt;

use super::run::{CaseStatus, Mismatch, TestRun};
use crate::error::Error;
use crate::format::test_run::mismatch_values;
use crate::json::{Json, to_string};
use crate::rules::RulesSource;

/// The indentation of what follows a result's first line.
const DETAIL: &str = "          ";

fn plural(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

/// An error's code, location and message, then its hint.
fn write_error(f: &mut fmt::Formatter<'_>, error: &Error) -> fmt::Result {
    let context = error.context();
    write!(f, "{DETAIL}{}", error.code().as_str())?;
    if let Some(pointer) = &context.pointer {
        let shown = if pointer.is_empty() {
            "the root"
        } else {
            pointer
        };
        write!(f, " at {shown}")?;
    } else if let (Some(line), Some(column)) = (context.line, context.column) {
        write!(f, " at line {line}, column {column}")?;
    }
    writeln!(f, ": {}", error.message())?;
    if let Some(hint) = error.hint() {
        writeln!(f, "{DETAIL}hint: {hint}")?;
    }
    Ok(())
}

impl fmt::Display for TestRun {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let what = match self.source {
            RulesSource::RuleSet => "a rule set",
            RulesSource::DecisionTable => "a decision table",
        };
        write!(
            f,
            "Test run of {what}: {}, hit policy {}",
            plural(self.rule_count, "rule", "rules"),
            self.hit_policy.as_str()
        )?;
        if let Some(aggregate) = self.aggregate {
            write!(f, ", aggregate {}", aggregate.as_str())?;
        }
        write!(f, ", logic {}", self.logic.as_str())?;
        if self.strict {
            f.write_str(", strict")?;
        }
        writeln!(f)?;
        for result in &self.results {
            let status = match result.status {
                CaseStatus::Passed => continue,
                CaseStatus::Failed => "FAILED",
                CaseStatus::Error => "ERROR",
            };
            // The id quoted, as a JSON string: ids may hold spaces.
            let label: Vec<String> = result
                .path
                .iter()
                .cloned()
                .chain(
                    result
                        .id
                        .iter()
                        .map(|id| to_string(&Json::String(id.clone()))),
                )
                .collect();
            if label.is_empty() {
                writeln!(f, "{status:<8}(a case file without a path)")?;
            } else {
                writeln!(f, "{status:<8}{}", label.join("  "))?;
            }
            for mismatch in &result.mismatches {
                if let Mismatch::Error { expected, actual } = mismatch {
                    write!(f, "{DETAIL}error: expected {}, actual ", expected.as_str())?;
                    match actual {
                        Some(error) => writeln!(f, "{}: {}", error.code().as_str(), error)?,
                        None => writeln!(f, "no error")?,
                    }
                    continue;
                }
                let (expected, actual) = mismatch_values(mismatch);
                write!(
                    f,
                    "{DETAIL}{}: expected {}",
                    mismatch.key().as_str(),
                    to_string(&expected)
                )?;
                match actual {
                    Some(actual) => writeln!(f, ", actual {}", to_string(&actual))?,
                    // Nothing was given: a decision without a single output.
                    None => writeln!(f, "; {}", mismatch.reason().unwrap_or_default())?,
                }
            }
            if let Some(explanation) = &result.explanation_text {
                writeln!(f, "{DETAIL}explanation:")?;
                for line in explanation.lines() {
                    writeln!(f, "{DETAIL}  {line}")?;
                }
            }
            if let Some(error) = &result.error {
                write_error(f, error)?;
            }
        }
        let summary = &self.summary;
        writeln!(
            f,
            "{}: {} passed, {} failed, {}",
            plural(summary.total(), "result", "results"),
            summary.passed,
            summary.failed,
            plural(summary.error, "error", "errors"),
        )?;
        if let Some(interrupted) = summary.interrupted {
            writeln!(
                f,
                "Partial run: interrupted by {}, {} not run",
                interrupted.reason.as_str(),
                interrupted.remaining
            )?;
        }
        Ok(())
    }
}
