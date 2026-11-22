//! Reproducible bundles (`spec/bundles.md`): a package of rules with its
//! identifier and revision, its rule set digest, the engine and limits that
//! made it, the contract of its input and output, and its test case files.
//! Comparing two bundles tells the same rules under another engine from
//! other rules under the same engine.
//!
//! Run it with `cargo run -p cleanrules --example bundles`.

#![allow(clippy::print_stdout)]

use cleanrules::bundle::{Bundle, BundleId, CaseFileRef, Difference, Relation, Revision};
use cleanrules::format::{Document, DocumentFormat, read_document, write_document_as};
use cleanrules::json::Limits;
use cleanrules::text;

const V1: &str = "RULE vip IF customer.years > 5 THEN discount = 0.15";
const V2: &str = "RULE vip IF customer.years > 3 THEN discount = 0.15";
// The bytes of a case file (`spec/tests.md`); the bundle records their digest.
const CASES: &[u8] = br#"{"version": 1, "cases": {"items": [{"id": "vip", "input": {"customer": {"years": 7}}, "expect": {"selected": ["vip"]}}]}}"#;

fn main() -> cleanrules::Result<()> {
    let limits = Limits::default();
    let cases = [CaseFileRef::new("cases/vip.json", CASES)?];
    // The author names the rules and their revision; the core never does.
    let id = BundleId::new("acme/discounts")?;
    let v1 = text::parse(V1, &limits)?.into_ruleset()?;
    let first = Bundle::new(v1, id.clone(), Revision::new("1")?)
        .with_cases(cases.clone())?
        .with_limits(&limits)?;
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
    assert_eq!(*read, first, "bundles are equal by digest");
    assert!(read.cases()[0].digest_matches(CASES));

    // A new revision of the rules, on the same engine.
    let v2 = text::parse(V2, &limits)?.into_ruleset()?;
    let second = Bundle::new(v2, id.clone(), Revision::new("2")?).with_cases(cases.clone())?;
    let comparison = first.compare(&second);
    println!("{}", cleanrules::json::to_string(&comparison.to_json()));
    assert_eq!(comparison.relation(), Relation::OtherRulesSameEngine);
    assert_eq!(
        comparison.differences(),
        [Difference::Revision, Difference::Rules]
    );

    // The same rules with other limits: another configuration of the engine.
    let mut tighter = limits;
    tighter.max_values = 1000;
    let rerun = Bundle::new(first.ruleset().clone(), id, Revision::new("1")?)
        .with_cases(cases)?
        .with_limits(&tighter)?;
    assert_eq!(
        first.compare(&rerun).relation(),
        Relation::SameRulesOtherEngine
    );
    Ok(())
}
