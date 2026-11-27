//! Comparing two versions of rules on the same cases (`spec/comparison.md`):
//! the discount rules are compared with a version in which vip gives 20%, on
//! the case file of the discount rules. The change is unexpected until the
//! new version's cases state the new discount; then it is expected, which is
//! how a command line passes a change made on purpose.
//!
//! Run it with `cargo run -p cleanrules --example comparison`.

#![allow(clippy::print_stdout)]

use cleanrules::cases::{CaseFile, Comparator, Runner};
use cleanrules::format::{DocumentFormat, read_document, write_test_comparison_as};
use cleanrules::json::Limits;

const RULES: &str = include_str!("discounts.json");
const CASES: &str = include_str!("cases/discounts.json");

fn main() -> cleanrules::Result<()> {
    let limits = Limits::default();
    let old = read_document(RULES, &limits)?.into_ruleset()?;
    let new = read_document(&RULES.replace("\"0.15\"", "\"0.20\""), &limits)?.into_ruleset()?;
    // The core never opens files: the caller gives each case file's bytes.
    let files = [CaseFile::read(
        "cases/discounts.json",
        CASES.as_bytes(),
        &limits,
    )];

    // The same cases for both versions: vip-gets-15 changes, unexpectedly.
    let comparator = Comparator::new(Runner::new(&old), Runner::new(&new))?.with_max_explained(1);
    let comparison = comparator.compare(&files, &files);
    println!("{comparison}");
    assert!(comparison.has_unexpected_changes());

    // The new version's cases state the new discount: the change is expected.
    let updated = CASES.replace(
        r#""output": { "discount": { "$decimal": "0.15" } }"#,
        r#""output": { "discount": { "$decimal": "0.20" } }"#,
    );
    let files_b = [CaseFile::read(
        "cases/discounts.json",
        updated.as_bytes(),
        &limits,
    )];
    let comparison = comparator.with_max_explained(0).compare(&files, &files_b);
    println!("{comparison}");
    assert!(!comparison.has_unexpected_changes());
    println!(
        "{}",
        write_test_comparison_as(&comparison, DocumentFormat::Json)?
    );
    Ok(())
}
