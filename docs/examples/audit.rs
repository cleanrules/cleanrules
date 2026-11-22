//! The static audit (`spec/audit.md`): what a rule set or a decision table
//! can and cannot do, found without any input. Each finding has a stable
//! code, a severity that depends on the hit policy, the rules involved and,
//! for overlaps and gaps, a witness: an input that shows it, which the core
//! has evaluated before reporting it. What the audit could not decide is
//! listed, never guessed.
//!
//! Run it with `cargo run -p cleanrules --example audit`.

#![allow(clippy::print_stdout)]

use cleanrules::audit::{AuditLimits, Severity, audit, audit_table};
use cleanrules::format::{DocumentFormat, write_audit_as};
use cleanrules::json::Limits;
use cleanrules::table::DecisionTable;
use cleanrules::text;

// Under `unique`, two rules that fire for one input make the evaluation fail.
const AGES: &str = "
HIT POLICY unique

RULE adult
IF age >= 18
THEN band = 'adult'

RULE senior ID \"AGE-2\"
IF age >= 65
THEN band = 'senior'
";

// A table where `ES` was meant as text: written without quotes, it is the
// field `ES`, which no input reads.
const COUNTRIES: &str = "\
HIT POLICY,first
RULE,country,age,THEN label
spain,ES,>= 18,'adult'
portugal,'PT',>= 18,'adult'
young,-,< 18,'young'
";

fn main() -> cleanrules::Result<()> {
    let limits = Limits::default();
    let ruleset = text::parse(AGES, &limits)?.into_ruleset()?;
    let report = audit(&ruleset, &AuditLimits::default())?;
    // The text the command line prints.
    print!("{report}");
    // Audit of a rule set: 2 rules, hit policy unique, logic boolean
    // error    overlap  adult, senior: rules adult and senior both fire for the witness, ...
    //            witness: {"age":65}
    // warning  gap  no rule fires for the witness, ...
    //            witness: {"age":0}
    // Checks: types complete, fields not applicable, unsatisfiable complete, ...
    // Witnesses are evaluated by the rule set itself, with its strictness:
    // {"age":65} raises hit_policy_error under unique.
    if report.max_severity() == Some(Severity::Error) {
        println!("an error: a CI job would fail here");
    }

    // The same report as a JSON document (`spec/audit.md`, *JSON*), with
    // the digest of the rule set audited, as a decision may carry it.
    let report = report.with_ruleset_digest(true);
    println!("{}", write_audit_as(&report, DocumentFormat::Json)?);

    let table = DecisionTable::from_csv(COUNTRIES, &limits)?;
    print!("{}", audit_table(&table, &AuditLimits::default())?);
    // warning  unlisted_field  spain: the cell of row spain under the input country reads
    //          the field ES, which no input reads: if it is text, write it between
    //          single quotes ('ES'), ...
    //            at /decision_table/rows/0/inputs/0
    //            field ES
    // The cell `country = ES` compares two fields, which the audit does not
    // model: it is listed under "Not modelled", and what it leaves open under
    // "Undecided".
    Ok(())
}
