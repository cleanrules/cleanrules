//! The workload shared by the suites: a loan screening rule set of 12 rules that
//! uses every family of node, and deterministic applicants, some with missing
//! or null fields.

use cleanrules::build::{constant, field};
use cleanrules::rules::{Config, HitPolicy, Rule, RuleSet};
use cleanrules::{Decimal, Map, NullCause, Value};

/// The rules, in order, as (name, condition).
fn rules() -> Vec<(&'static str, cleanrules::build::Build)> {
    vec![
        ("underage", field("age").lt(18)),
        ("sanctioned", field("country").is_in(["KP", "IR", "SY"])),
        (
            "bad_email",
            !field("email").matches(r"^[^@\s]+@[^@\s]+\.[a-z]+$"),
        ),
        ("high_debt", (field("debt") / field("income")).gt(0.6)),
        ("low_score", field("score").lt(500)),
        (
            "prime",
            field("score").ge(750)
                & field("income").gt(50_000)
                & field("employment").is_in(["employed", "self_employed"]),
        ),
        (
            "near_prime",
            field("score").between(650, 750) & field("income").gt(30_000),
        ),
        (
            "student",
            field("employment").eq("student") & field("age").lt(26),
        ),
        ("senior", field("age").ge(65) & field("income").gt(20_000)),
        ("partner", field("email").ends_with("@example.com")),
        (
            "manual_review",
            field("score").is_null() | field("income").is_null(),
        ),
        ("default", constant(true)),
    ]
}

/// The rule set under a hit policy; each rule outputs its name and a rate.
pub(crate) fn ruleset(hit_policy: HitPolicy) -> RuleSet {
    let rules = rules()
        .into_iter()
        .zip(1_i64..)
        .map(|((name, when), rate)| {
            let then = vec![
                ("decision".to_owned(), constant(name).build().unwrap()),
                (
                    "rate".to_owned(),
                    (field("base_rate") * rate).build().unwrap(),
                ),
            ];
            Rule::with_payload(name, when.build().unwrap(), then).unwrap()
        })
        .collect();
    let config = Config {
        hit_policy,
        ..Config::default()
    };
    RuleSet::new(rules, config).unwrap()
}

/// A linear congruential generator, so that every run sees the same inputs.
struct Lcg(u64);

impl Lcg {
    fn next(&mut self, bound: u64) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (self.0 >> 33) % bound
    }

    fn pick<'a>(&mut self, items: &[&'a str]) -> &'a str {
        items[usize::try_from(self.next(items.len() as u64)).unwrap()]
    }
}

/// `count` applicants. About one in ten lacks a score and one in twenty has a
/// null income.
pub(crate) fn inputs(count: usize) -> Vec<Value> {
    let mut rng = Lcg(42);
    (0..count)
        .map(|_| {
            let mut map = Map::new();
            map.insert("age", Value::Int(16 + rng.next(70).cast_signed()));
            let income = if rng.next(20) == 0 {
                Value::Null(NullCause::Null)
            } else {
                Value::float(5_000.0 + 1_000.0 * f64::from(u32::try_from(rng.next(120)).unwrap()))
            };
            map.insert("income", income);
            map.insert(
                "debt",
                Value::Decimal(Decimal::new(i128::from(rng.next(9_000_000)), 2).unwrap()),
            );
            if rng.next(10) != 0 {
                map.insert("score", Value::Int(300 + rng.next(550).cast_signed()));
            }
            let country = rng.pick(&["ES", "FR", "DE", "US", "KP", "MX", "IT"]);
            map.insert("country", Value::String(country.into()));
            let employment = rng.pick(&["employed", "self_employed", "student", "retired"]);
            map.insert("employment", Value::String(employment.into()));
            let email = rng.pick(&["ana@example.com", "luis@mail.es", "bad-address", "x@y.org"]);
            map.insert("email", Value::String(email.into()));
            map.insert("base_rate", Value::Decimal(Decimal::new(125, 2).unwrap()));
            Value::Map(map)
        })
        .collect()
}
