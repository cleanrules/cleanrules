//! Exact decimal arithmetic (`spec/ast.md`, *Arithmetic*, *Decimal arithmetic*
//! and *Rounding*): discounted prices stay exact decimals, `ROUND` gives them
//! cents, a division keeps 38 significant digits, a float anywhere makes the
//! result a float, and a `sum` adds exactly, whatever the rule order.
//!
//! Run it with `cargo run -p cleanrules --example decimals`.

#![allow(clippy::print_stdout)]

use cleanrules::Value;
use cleanrules::format::{ValueContext, read_value, write_value};
use cleanrules::json::{Limits, parse, to_string};
use cleanrules::rules::Aggregated;
use cleanrules::text;

// In the rule syntax, `0.15` is a decimal and `0.15e0` a float. A chained
// discount (15 %, then 10 % more) rounded to cents once, at the end.
const RULES: &str = "
RULE vip IF customer.years > 5
THEN saving = order.total * 0.15,
     price = ROUND(order.total * (1 - 0.15) * (1 - 0.10), 2),
     exact_price = order.total * (1 - 0.15) * (1 - 0.10),
     instalment = ROUND(order.total / 3, 2, HALF_UP),
     float_saving = order.total * 0.15e0
";

// Three surcharges summed exactly: the same total in any rule order.
const SURCHARGES: &str = "
HIT POLICY collect AGGREGATE sum
RULE a IF TRUE THEN fee = 0.10
RULE b IF TRUE THEN fee = 0.20
RULE c IF TRUE THEN fee = 0.05
";

fn data(text: &str) -> cleanrules::Result<Value> {
    read_value(&parse(text, &Limits::default())?, "", ValueContext::Data)
}

fn main() -> cleanrules::Result<()> {
    let rules = text::parse(RULES, &Limits::default())?.into_ruleset()?;
    // A price in cents, as a decimal: `{"$decimal": "19.99"}` in JSON, and
    // `Decimal("19.99")` in Python or `BigDecimal` in Java.
    let order = data(r#"{"customer": {"years": 7}, "order": {"total": {"$decimal": "19.99"}}}"#)?;
    let decision = rules.evaluate(&order)?;
    for (key, value) in decision.outputs().into_iter().flatten() {
        println!("{key}: {}", to_string(&write_value(&value)));
    }
    // saving: {"$decimal":"2.9985"}            19.99 × 0.15, exact
    // price: {"$decimal":"15.29"}              19.99 × 0.85 × 0.90, in cents
    // exact_price: {"$decimal":"15.292350"}    the same, before rounding
    // instalment: {"$decimal":"6.66"}          19.99 / 3, in cents
    // float_saving: 2.9984999999999995         a float operand gives a float

    let surcharges = text::parse(SURCHARGES, &Limits::default())?.into_ruleset()?;
    let decision = surcharges.evaluate(&data("{}")?)?;
    if let Some(Aggregated::Values(totals)) = decision.aggregated() {
        for (key, value) in totals.iter() {
            println!("{key}: {}", to_string(&write_value(value)));
        }
    }
    // fee: {"$decimal":"0.35"}
    Ok(())
}
