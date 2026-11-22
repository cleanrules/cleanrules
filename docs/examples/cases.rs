//! Test cases (`spec/tests.md`): a case file holds identified inputs and what
//! evaluating each one must give: the selected rules (by effective id), the
//! output, the rules left unknown, or the error. This example reads the case
//! file of the discount rules, shows what each case expects, builds the same
//! kind of case in code, and writes a case file in YAML. Running the cases
//! against rules, and comparing two versions on them, build on this format.
//!
//! Run it with `cargo run -p cleanrules --features yaml --example cases`.

#![allow(clippy::print_stdout)]

use cleanrules::cases::{Case, Cases, Expectation, ExpectedOutput};
use cleanrules::format::{Document, DocumentFormat, read_document, write_document_as};
use cleanrules::json::Limits;
use cleanrules::{Map, Value};

const CASES: &str = include_str!("cases/discounts.json");

fn main() -> cleanrules::Result<()> {
    let Document::Cases(cases) = read_document(CASES, &Limits::default())? else {
        unreachable!("a case file holds cases")
    };
    for case in &cases {
        let expect = case.expect();
        let selected = expect.selected().map(|ids| ids.join(", "));
        let output = match expect.output() {
            Some(ExpectedOutput::NothingSelected) => "no output".to_owned(),
            Some(ExpectedOutput::Includes(keys)) => {
                let keys: Vec<&str> = keys.iter().map(|(key, _)| key).collect();
                format!("an output with {}", keys.join(", "))
            }
            _ => "any output".to_owned(),
        };
        println!(
            "{}: selects [{}], {output}",
            case.id(),
            selected.unwrap_or_default()
        );
    }

    // The same kind of case, built in code: an expected error is given alone.
    let mut input = Map::new();
    input.insert("order", Value::Map(Map::new()));
    let strict = Case::new("total-missing", Value::Map(input))?
        .with_description("strict rules refuse an order without a total")
        .with_expect(Expectation::new().with_error(cleanrules::ErrorCode::MissingField)?);
    let more = Cases::new([strict])?.with_description("cases written in code");
    println!(
        "{}",
        write_document_as(&more.into_document(), DocumentFormat::Yaml)?
    );
    Ok(())
}
