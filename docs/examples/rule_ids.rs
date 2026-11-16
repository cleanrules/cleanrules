//! Rule identity (`spec/rules.md`, *Identity*): a rule's effective id is its
//! `id`, or its name when it has none. Renaming a rule without an id makes it
//! another rule when two versions of a rule set are compared; giving it an
//! explicit id, its old name, keeps it the same rule. Effective ids are unique
//! within a rule set.
//!
//! Run it with `cargo run -p cleanrules --example rule_ids`.

#![allow(clippy::print_stdout)]

use cleanrules::json::Limits;
use cleanrules::text;

// Version 1: the rules are identified by their names.
const VERSION_1: &str = "
HIT POLICY first

RULE vip
IF customer.years > 5
THEN discount = 0.15

RULE default
IF TRUE
THEN discount = 0
";

// Version 2: `vip` is renamed `gold_customer`, and keeps its identity with
// `ID vip`. A field named `id` needs no backticks: `ID` is a keyword only
// right after a rule's name.
const VERSION_2: &str = "
HIT POLICY first

RULE gold_customer ID vip
IF customer.years > 5 AND id IS NOT NULL
THEN discount = 0.15

RULE default
IF TRUE
THEN discount = 0
";

// `x` has no id, so its name is its effective id, which `b`'s id repeats.
const CLASH: &str = "
RULE x IF p
RULE b ID x IF q
";

fn main() -> cleanrules::Result<()> {
    let limits = Limits::default();
    let before = text::parse(VERSION_1, &limits)?.into_ruleset()?;
    let after = text::parse(VERSION_2, &limits)?.into_ruleset()?;
    for (rule, renamed) in before.rules().iter().zip(after.rules()) {
        println!(
            "{} -> {}: same rule: {}",
            rule.name(),
            renamed.name(),
            rule.effective_id() == renamed.effective_id()
        );
    }
    // vip -> gold_customer: same rule: true
    // default -> default: same rule: true

    if let Err(error) = text::parse(CLASH, &limits) {
        println!("{}: {error}", error.code());
    }
    // rule_definition_error: the id "x" of rule "b" repeats the name of rule
    // "x", which has no id: effective ids must be unique
    Ok(())
}
