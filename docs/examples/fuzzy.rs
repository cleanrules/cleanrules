//! Evaluating a condition under the fuzzy logics (`spec/ast.md`, *Truth
//! values*, *Logics*): degrees in the data, fuzzytool's norms, and unknown as
//! the interval `[0, 1]`.
//!
//! Run it with `cargo run -p cleanrules --example fuzzy`.

#![allow(clippy::print_stdout)]

use cleanrules::Value;
use cleanrules::eval::Options;
use cleanrules::format::{ValueContext, read_logic, read_value, write_any_result};
use cleanrules::json::{Limits, parse, to_string};
use cleanrules::logic::{AnyLogic, Complement, LogicName};
use cleanrules::text::parse_node;

fn data(text: &str) -> cleanrules::Result<Value> {
    read_value(&parse(text, &Limits::default())?, "", ValueContext::Data)
}

fn main() -> cleanrules::Result<()> {
    // Degrees in [0, 1] are read from the data; `tier > 2` is a crisp comparison.
    let condition = parse_node("(warm AND humid) OR NOT (tier > 2)", &Limits::default())?;
    let input = data(r#"{"warm": 0.8, "humid": 0.6, "tier": 3}"#)?;

    // A logic by name, with the standard complement:
    // (0.8 · 0.6) or not true = 0.48 or 0 = 0.48.
    let product = AnyLogic::from_name("prod")?;
    let tree = product.evaluate_tree(&condition, &input, Options::default())?;
    println!("prod: {}", to_string(&write_any_result(&tree)));

    // A parametric family, in the JSON form that APIs and vectors use.
    let yager = read_logic(
        &parse(r#"{"name": "yager", "p": 2.0}"#, &Limits::default())?,
        "",
    )?;
    let tree = yager.evaluate_tree(&condition, &input, Options::default())?;
    println!("yager p=2: {}", to_string(&write_any_result(&tree)));

    // The same family with Sugeno's complement, built in code.
    let sugeno = AnyLogic::with_parameter(LogicName::Yager, Some(2.0), Complement::sugeno(0.5)?)?;
    let tree = sugeno.evaluate_tree(&condition, &input, Options::default())?;
    println!("yager p=2, sugeno: {}", to_string(&write_any_result(&tree)));

    // A missing field is unknown, the interval [0, 1]: the result is the
    // interval of every degree it could have, with the reason:
    // [0.8 · 0, 0.8 · 1] = [0.0, 0.8], because `humid` is missing.
    let missing = data(r#"{"warm": 0.8, "tier": 3}"#)?;
    let tree = product.evaluate_tree(&condition, &missing, Options::default())?;
    println!(
        "prod, humid missing: {}",
        to_string(&write_any_result(&tree))
    );
    Ok(())
}
