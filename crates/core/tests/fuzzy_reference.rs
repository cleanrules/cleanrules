//! The fuzzy norms and complements against fuzzytool, their behavioural
//! reference (`spec/ast.md`, *Logics*): `data/fuzzytool_norms.json` holds
//! fuzzytool's values on a grid of degrees, made by `data/fuzzytool_norms.py`.
//!
//! - Inside `(0, 1)`, the norms use fuzzytool's formulas in the same order of
//!   operations, so they agree exactly wherever `pow` and `log` are not
//!   involved, and to a few units in the last place where they are (the core
//!   uses the `libm` crate, fuzzytool the platform's).
//! - On the boundary, the core keeps `T(1, b) = b`, `T(0, b) = 0`,
//!   `S(0, b) = b` and `S(1, b) = 1` exactly, which fuzzytool's floating-point
//!   formulas only approximate for some norms (Łukasiewicz, Dombi, Frank).
#![allow(clippy::unwrap_used, clippy::panic, clippy::float_cmp)]

use std::fs;
use std::path::Path;

use cleanrules::json::{Json, Limits, parse};
use cleanrules::logic::{Complement, Norm};

/// How far a degree computed with `pow` or `log` may be from fuzzytool's: the
/// core uses the `libm` crate on every platform, and fuzzytool (`NumPy`) the
/// platform's, so they may differ in the last places.
const POW_TOLERANCE: f64 = 1e-14;
/// How far fuzzytool may be from the exact boundary values: its Dombi s-norm
/// clips degrees to `[1e-12, 1 − 1e-12]`, which moves `S(0, b)` by up to
/// `7e-7` when `p` is 0.5.
const BOUNDARY_TOLERANCE: f64 = 1e-6;

/// The boundary condition of a norm at `(a, b)`, when one of them is 0 or 1.
fn boundary_law(kind: &str, a: f64, b: f64) -> Option<f64> {
    let tnorm = kind == "tnorm";
    if [a, b].contains(&(if tnorm { 0.0 } else { 1.0 })) {
        Some(if tnorm { 0.0 } else { 1.0 })
    } else if a == (if tnorm { 1.0 } else { 0.0 }) {
        Some(b)
    } else if b == (if tnorm { 1.0 } else { 0.0 }) {
        Some(a)
    } else {
        None
    }
}

fn get<'j>(json: &'j Json, key: &str) -> Option<&'j Json> {
    match json {
        Json::Object(members) => members.iter().find(|(k, _)| k == key).map(|(_, v)| v),
        _ => None,
    }
}

fn number(json: &Json) -> f64 {
    match json {
        Json::Number(n) => n.as_f64(),
        other => panic!("not a number: {other:?}"),
    }
}

fn items(json: &Json) -> &[Json] {
    match json {
        Json::Array(items) => items,
        other => panic!("not an array: {other:?}"),
    }
}

fn text(json: &Json) -> &str {
    match json {
        Json::String(s) => s,
        other => panic!("not a string: {other:?}"),
    }
}

fn norm(name: &str, parameter: Option<f64>) -> Norm {
    match (name, parameter) {
        ("min", None) => Norm::Min,
        ("prod", None) => Norm::Prod,
        ("lukasiewicz", None) => Norm::Lukasiewicz,
        ("einstein", None) => Norm::Einstein,
        // fuzzytool's Hamacher product is the family at γ = 0.
        ("hamacher", None) => Norm::hamacher(0.0).unwrap(),
        ("drastic", None) => Norm::Drastic,
        ("nilpotent", None) => Norm::Nilpotent,
        ("yager", Some(p)) => Norm::yager(p).unwrap(),
        ("dombi", Some(p)) => Norm::dombi(p).unwrap(),
        ("frank", Some(s)) => Norm::frank(s).unwrap(),
        other => panic!("no norm {other:?}"),
    }
}

fn complement(name: &str, parameter: Option<f64>) -> Complement {
    match (name, parameter) {
        ("standard", None) => Complement::Standard,
        ("sugeno", Some(lam)) => Complement::sugeno(lam).unwrap(),
        ("yager", Some(w)) => Complement::yager(w).unwrap(),
        other => panic!("no complement {other:?}"),
    }
}

/// Whether the core computes this connective with `pow` or `log` (the `libm`
/// crate): the exponents 1, -1, 2 and 0.5 use exact operations instead.
fn uses_libm(name: &str, kind: &str, parameter: Option<f64>) -> bool {
    let exact = |e: f64| [1.0, -1.0, 2.0, 0.5].contains(&e);
    match (name, kind, parameter) {
        ("frank", _, _) => true,
        ("dombi", "snorm", Some(p)) => !(exact(-p) && exact(-1.0 / p)),
        ("yager" | "dombi", _, Some(p)) => !(exact(p) && exact(1.0 / p)),
        _ => false,
    }
}

#[test]
fn norms_and_complements_match_fuzzytool() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/fuzzytool_norms.json");
    let document = parse(&fs::read_to_string(path).unwrap(), &Limits::default()).unwrap();
    assert!(text(get(&document, "source").unwrap()).starts_with("fuzzytool "));
    let mut failures = Vec::new();
    let mut checked = 0;
    let mut exact = 0;
    let mut approximate_boundary = 0;
    let (mut libm_differs, mut most_ulps) = (0, 0_u64);
    for case in items(get(&document, "cases").unwrap()) {
        let kind = text(get(case, "kind").unwrap());
        let name = text(get(case, "name").unwrap());
        let parameter = get(case, "parameter").map(number);
        let libm = uses_libm(name, kind, parameter);
        for row in items(get(case, "values").unwrap()) {
            let row: Vec<f64> = items(row).iter().map(number).collect();
            let (got, reference, law) = match kind {
                "tnorm" | "snorm" => {
                    let (a, b) = (row[0], row[1]);
                    let n = norm(name, parameter);
                    let got = if kind == "tnorm" {
                        n.tnorm(a, b)
                    } else {
                        n.snorm(a, b)
                    };
                    (got, row[2], boundary_law(kind, a, b))
                }
                "complement" => (complement(name, parameter).apply(row[0]), row[1], None),
                other => panic!("unknown kind {other}"),
            };
            checked += 1;
            if let Some(law) = law {
                // The core keeps the boundary condition exactly; fuzzytool
                // approximates it.
                if got != law || (reference - law).abs() > BOUNDARY_TOLERANCE {
                    failures.push(format!(
                        "{kind} {name} {parameter:?} at {:?}: core {got:?}, exactly {law:?}, \
                         fuzzytool {reference:?}",
                        &row[..2]
                    ));
                }
                if reference == law {
                    exact += 1;
                } else {
                    approximate_boundary += 1;
                }
                continue;
            }
            let tolerance = if libm { POW_TOLERANCE } else { 0.0 };
            if got == reference {
                exact += 1;
            } else if libm {
                libm_differs += 1;
                most_ulps = most_ulps.max(got.to_bits().abs_diff(reference.to_bits()));
            }
            if got != reference && (got - reference).abs() > tolerance {
                failures.push(format!(
                    "{kind} {name} {parameter:?} at {:?}: core {got:?}, fuzzytool {reference:?}",
                    &row[..row.len() - 1]
                ));
            }
        }
    }
    println!(
        "{checked} values checked, {exact} exactly equal to fuzzytool's; fuzzytool \
         approximates {approximate_boundary} boundary values; {libm_differs} values that use \
         pow or log differ from the platform's, by at most {most_ulps} units in the last place"
    );
    assert!(checked > 5000, "only {checked} values");
    assert!(
        failures.is_empty(),
        "{} differ:\n{}",
        failures.len(),
        failures.join("\n")
    );
}
