//! Reproducible bundles (`spec/bundles.md`): a package of rules with its
//! identifier and revision, its rule set digest, the engine and limits that
//! made it, the contract of its input and output, and its test case files.
//! Comparing two bundles tells the same rules under another engine from
//! other rules under the same engine.
//!
//! Run it with `cargo run -p cleanrules --example bundles`.

#![allow(clippy::print_stdout)]

use cleanrules::bundle::{Bundle, CaseFile, Relation};
use cleanrules::format::{Document, DocumentFormat, read_document, write_document_as};
use cleanrules::json::Limits;
use cleanrules::text;

const V1: &str = "RULE vip IF customer.years > 5 THEN discount = 0.15";
const V2: &str = "RULE vip IF customer.years > 3 THEN discount = 0.15";
// The bytes of a case file; until task T.1 their format is opaque.
const CASES: &[u8] = br#"[{"input": {"customer": {"years": 7}}}]"#;

fn main() -> cleanrules::Result<()> {
    let limits = Limits::default();
    let cases = || CaseFile::new("cases/vip.json", CASES).map(|case| vec![case]);
    // The author names the rules and their revision; the core never does.
    let v1 = text::parse(V1, &limits)?.into_ruleset()?;
    let first = Bundle::new(v1, "acme/discounts", "1", cases()?, &limits)?;
    let written = write_document_as(&first.clone().into_document(), DocumentFormat::Json)?;
    println!("{written}");
    println!(
        "bundle {}, rules {}",
        first.digest(),
        first.ruleset_digest()
    );

    // Read back: the reader verifies the rule set digest and, for a bundle of
    // this engine, the contract.
    let Document::Bundle(read) = read_document(&written, &limits)? else {
        unreachable!("a bundle document gives a bundle")
    };
    assert_eq!(read.digest(), first.digest());
    assert!(read.cases()[0].matches(CASES));

    // A new revision of the rules, on the same engine.
    let v2 = text::parse(V2, &limits)?.into_ruleset()?;
    let second = Bundle::new(v2, "acme/discounts", "2", cases()?, &limits)?;
    let comparison = first.compare(&second);
    println!("{}", cleanrules::json::to_string(&comparison.to_json()));
    assert_eq!(comparison.relation(), Relation::OtherRulesSameEngine);

    // The same rules with other limits: another configuration of the engine.
    let mut tighter = limits;
    tighter.max_values = 1000;
    let rerun = Bundle::new(
        first.ruleset().clone(),
        "acme/discounts",
        "1",
        cases()?,
        &tighter,
    )?;
    assert_eq!(
        first.compare(&rerun).relation(),
        Relation::SameRulesOtherEngine
    );
    Ok(())
}
