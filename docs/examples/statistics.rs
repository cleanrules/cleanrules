//! The statistics and coverage of a run (`spec/tests.md`, *Statistics*): the
//! case file of the discount rules is run against them, gathering how many
//! times each rule was selected and fired and each condition was `TRUE`,
//! `FALSE` or `UNKNOWN`, and which ones the cases never reached.
//!
//! Run it with `cargo run -p cleanrules --example statistics`.

#![allow(clippy::print_stdout)]

use cleanrules::cases::{CaseFile, CoverageMeasure, Missed, Runner};
use cleanrules::format::write_statistics;
use cleanrules::json::{Limits, to_string};

const RULES: &str = include_str!("discounts.json");
const CASES: &str = include_str!("cases/discounts.json");

fn main() -> cleanrules::Result<()> {
    let limits = Limits::default();
    let rules = cleanrules::format::read_document(RULES, &limits)?.into_ruleset()?;
    let files = [CaseFile::read(
        "cases/discounts.json",
        CASES.as_bytes(),
        &limits,
    )];

    // Counted while the cases run, from each decision: no result tree is kept.
    let run = Runner::new(&rules).with_statistics(true).run(&files);
    // The text of the run ends with its statistics and coverage.
    println!("{run}");

    let Some(statistics) = &run.statistics else {
        unreachable!("the runner gathers statistics")
    };
    for measure in statistics.coverage() {
        if measure.measure == CoverageMeasure::ConditionUnknown {
            for missed in &measure.missed {
                if let Missed::Condition { rule, pointer } = missed {
                    println!("never unknown: {rule} at {pointer}: no case leaves it without data");
                }
            }
        }
    }
    // The `statistics` member of the test run document.
    println!("{}", to_string(&write_statistics(statistics)));
    Ok(())
}
