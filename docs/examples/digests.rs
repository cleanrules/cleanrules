//! Canonical bytes and digests (`spec/json-format.md`, *Canonical bytes* and
//! *Digests*): the same rule set written as JSON with another layout, as YAML
//! or as text has one digest; a decision can carry it, and a reader refuses
//! the decision with any other rule set. A digest identifies an artifact: the
//! same rules written differently (`5 < years`, or `0.150` instead of
//! `0.15`) have another digest.
//!
//! Run it with `cargo run -p cleanrules --example digests`.

#![allow(clippy::print_stdout)]

use cleanrules::format::{DocumentFormat, ValueContext, read_value};
use cleanrules::format::{read_decision_document, read_document, write_decision_as};
use cleanrules::json::{Limits, parse};
use cleanrules::text;

const RULES: &str = "RULE vip IF customer.years > 5 THEN discount = 0.15";

// The same rule set as a JSON document, with its own layout and member order.
const JSON: &str = r#"{
  "ruleset": {
    "rules": [{
      "when": { "op": "gt", "right": { "op": "const", "value": 5 },
                "left": { "op": "field", "path": ["customer", "years"] } },
      "name": "vip",
      "then": { "discount": { "op": "const", "value": { "$decimal": "0.15" } } }
    }],
    "logic": "boolean",
    "hit_policy": "first"
  },
  "version": 1
}"#;

fn main() -> cleanrules::Result<()> {
    let limits = Limits::default();
    let rules = text::parse(RULES, &limits)?.into_ruleset()?;
    let same = read_document(JSON, &limits)?.into_ruleset()?;
    println!("{}", String::from_utf8_lossy(&rules.canonical_bytes()));
    println!("text: {}", rules.digest());
    println!("JSON: {}", same.digest());
    assert_eq!(rules.digest(), same.digest());

    // Written differently, decided alike: another artifact, another digest.
    for other in [
        "RULE vip IF 5 < customer.years THEN discount = 0.15",
        "RULE vip IF customer.years > 5 THEN discount = 0.150",
    ] {
        let other = text::parse(other, &limits)?.into_ruleset()?;
        println!("{}", other.digest());
        assert_ne!(other.digest(), rules.digest());
    }

    // A decision that carries its rule set's digest, for an audit log.
    let input = read_value(
        &parse(r#"{"customer": {"years": 7}}"#, &limits)?,
        "",
        ValueContext::Data,
    )?;
    let decision = rules.evaluate(&input)?.with_ruleset_digest(true);
    let stored = write_decision_as(&decision, DocumentFormat::Json)?;
    println!("{stored}");
    // Read back with the same rules, it is accepted and keeps its digest.
    assert!(read_decision_document(&stored, &rules, &limits)?.carries_ruleset_digest());
    // A comment in the rule text is not part of the document...
    let commented = text::parse(&format!("{RULES} # reviewed"), &limits)?.into_ruleset()?;
    assert_eq!(commented.digest(), rules.digest());
    // ...but a description is, so the decision is refused with rules that
    // differ only there, which the other checks of a decision cannot see.
    let described = read_document(
        &JSON.replace(
            r#""name": "vip","#,
            r#""name": "vip", "description": "edited","#,
        ),
        &limits,
    )?
    .into_ruleset()?;
    let refused = read_decision_document(&stored, &described, &limits)
        .err()
        .map(|e| e.to_string());
    println!("{}", refused.unwrap_or_default());
    Ok(())
}
