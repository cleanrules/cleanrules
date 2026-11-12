//! Compact explanations (`spec/agents.md`, *Compact explanations*): the text of
//! an explanation, cut to a budget for a model's context.

use std::fmt;

use crate::error::{Error, ErrorCode, Result};
use crate::explain::{Explanation, Status, header};

/// The default budget, in Unicode code points.
pub const DEFAULT_BUDGET: usize = 2000;
/// The smallest budget: room for the header and the last line.
pub const MIN_BUDGET: usize = 256;
/// The largest budget.
pub const MAX_BUDGET: usize = 1 << 20;
/// The longest line kept whole, in code points.
pub const MAX_LINE: usize = 160;

/// An explanation's text cut to a budget.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct CompactExplanation {
    /// The text: at most the budget, in Unicode code points.
    pub text: String,
    /// Whether it is the explanation's whole text.
    pub complete: bool,
}

/// The text.
impl fmt::Display for CompactExplanation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.text)
    }
}

/// The error of a budget out of its range: a `format_error` at `/budget`, as
/// the `explain` tool reports it (`spec/agents.md`, *Arguments*).
pub(super) fn budget_error() -> Error {
    Error::new(
        ErrorCode::FormatError,
        format!("the argument \"budget\" is an integer from {MIN_BUDGET} to {MAX_BUDGET}"),
    )
    .with_hint(format!(
        "give a budget of at least {MIN_BUDGET} code points, or leave it out for {DEFAULT_BUDGET}"
    ))
    .at_pointer("/budget")
}

fn width(text: &str) -> usize {
    text.chars().count()
}

/// A line of at most [`MAX_LINE`] code points, and whether it was cut.
fn bounded(line: &str) -> (String, bool) {
    if width(line) <= MAX_LINE {
        (line.to_owned(), false)
    } else {
        let mut cut: String = line.chars().take(MAX_LINE - 3).collect();
        cut.push_str("...");
        (cut, true)
    }
}

/// The order in which rules are taken: what decided first, then what the
/// input lacked, then what another input could change.
fn rank(status: Status, achievable: Option<bool>) -> u8 {
    match (status, achievable) {
        (Status::Selected, _) => 0,
        (Status::Unknown, _) => 1,
        (Status::Fired, _) => 2,
        (Status::NotFired, Some(false)) => 4,
        (Status::NotFired, _) => 3,
    }
}

fn trailer(shortened: usize, rules: usize) -> String {
    format!("... {shortened} of {rules} rules shortened or omitted to fit the budget")
}

/// The compact explanation of `explanation` within `budget` code points
/// (`spec/agents.md`, *Compact explanations*).
///
/// ```
/// use cleanrules::agents::compact_explanation;
/// use cleanrules::explain::explain;
/// use cleanrules::json::Limits;
/// use cleanrules::{Map, Value};
///
/// let rules = cleanrules::text::parse("RULE adult IF age >= 18", &Limits::default())?
///     .into_ruleset()?;
/// let decision = rules.evaluate(&Value::Map(Map::from_iter([("age".to_owned(), Value::Int(20))])))?;
/// let compact = compact_explanation(&explain(&decision), 2000)?;
/// assert!(compact.complete);
/// assert_eq!(compact.to_string(), explain(&decision).to_string());
/// assert!(compact_explanation(&explain(&decision), 100).is_err());
/// # Ok::<(), cleanrules::Error>(())
/// ```
///
/// # Errors
///
/// `format_error` at `/budget` for a budget outside
/// [`MIN_BUDGET`]..=[`MAX_BUDGET`], as the `explain` tool reports it.
pub fn compact_explanation(
    explanation: &Explanation<'_, '_>,
    budget: usize,
) -> Result<CompactExplanation> {
    if !(MIN_BUDGET..=MAX_BUDGET).contains(&budget) {
        return Err(budget_error());
    }
    let (header, mut cut) = bounded(&header(explanation));
    let mut blocks: Vec<Vec<String>> = Vec::with_capacity(explanation.rules.len());
    for rule in &explanation.rules {
        let text = rule.to_string();
        let lines = text
            .split('\n')
            .map(|line| {
                let (line, was_cut) = bounded(line);
                cut |= was_cut;
                line
            })
            .collect();
        blocks.push(lines);
    }
    // A block's size, with the line break before each of its lines.
    let size = |lines: &[String]| lines.iter().map(|l| width(l) + 1).sum::<usize>();
    let whole = width(&header) + blocks.iter().map(|b| size(b)).sum::<usize>();
    let mut text = header;
    if whole <= budget {
        for line in blocks.iter().flatten() {
            text.push('\n');
            text.push_str(line);
        }
        return Ok(CompactExplanation {
            text,
            complete: !cut,
        });
    }
    let rules = blocks.len();
    let reserve = 1 + width(&trailer(rules, rules));
    let mut order: Vec<usize> = (0..rules).collect();
    order.sort_by_key(|&i| {
        let rule = &explanation.rules[i];
        rank(rule.status, rule.achievable)
    });
    // How many lines of each block are taken: all, one or none.
    let mut taken = vec![0; rules];
    let mut used = width(&text);
    for i in order {
        let block = &blocks[i];
        let all = size(block);
        let first = block.first().map_or(0, |l| width(l) + 1);
        if used + all + reserve <= budget {
            taken[i] = block.len();
            used += all;
        } else if used + first + reserve <= budget {
            taken[i] = 1;
            used += first;
        }
    }
    let shortened = blocks
        .iter()
        .zip(&taken)
        .filter(|(block, taken)| **taken < block.len())
        .count();
    for (block, taken) in blocks.iter().zip(taken) {
        for line in &block[..taken] {
            text.push('\n');
            text.push_str(line);
        }
    }
    text.push('\n');
    text.push_str(&trailer(shortened, rules));
    Ok(CompactExplanation {
        text,
        complete: false,
    })
}
