//! Running test cases (`spec/tests.md`, *Running cases*, *Test runs*): the
//! case file of the discount rules is run against them, then against a
//! changed version, whose failures are listed with what was expected, what
//! was given and why; then the same run is taken one result at a time and
//! stopped after two, which marks it partial.
//!
//! Run it with `cargo run -p cleanrules --example test_run`.

#![allow(clippy::print_stdout)]

use cleanrules::cases::{CaseFile, Runner, TestRunLimits};
use cleanrules::format::{Document, DocumentFormat, read_document, write_test_run_as};
use cleanrules::json::Limits;

const RULES: &str = include_str!("discounts.json");
const CASES: &str = include_str!("cases/discounts.json");

fn main() -> cleanrules::Result<()> {
    let limits = Limits::default();
    let rules = read_document(RULES, &limits)?.into_ruleset()?;
    // The core never opens files: the caller gives each case file's bytes.
    let files = [CaseFile::read(
        "cases/discounts.json",
        CASES.as_bytes(),
        &limits,
    )];

    let run = Runner::new(&rules).run(&files);
    println!("{run}");
    assert!(run.all_passed());

    // A changed version: vip now gives 20%.
    let changed = RULES.replace("\"0.15\"", "\"0.20\"");
    let Document::RuleSet(changed) = read_document(&changed, &limits)? else {
        unreachable!("a rule set document")
    };
    let run = Runner::new(&changed)
        .with_limits(TestRunLimits::default().with_max_explained(1))
        .run(&files);
    println!("{run}");
    println!("{}", write_test_run_as(&run, DocumentFormat::Json)?);

    // One result at a time: the caller stops after two.
    let runner = Runner::new(&rules);
    let mut results = runner.iter(&files);
    for result in results.by_ref().take(2) {
        println!(
            "{} {}",
            result.status.as_str(),
            result.id.unwrap_or_default()
        );
    }
    results.cancel();
    let summary = results.summary();
    println!(
        "partial: {}, {} not run",
        summary.is_partial(),
        summary.interrupted.map_or(0, |i| i.remaining)
    );
    Ok(())
}
