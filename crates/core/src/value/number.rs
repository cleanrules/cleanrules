//! Numbers of the three kinds, compared exactly by value (`spec/ast.md`, *Types*).

use std::cmp::Ordering;

use super::bignum::BigUint;
use super::decimal::{Decimal, fit};

/// An int, a float (never NaN) or a decimal.
#[derive(Clone, Copy, Debug)]
pub enum Number {
    /// A signed 64-bit integer.
    Int(i64),
    /// A binary64 float. Callers never pass NaN; it compares equal to nothing.
    Float(f64),
    /// An exact decimal.
    Decimal(Decimal),
}

/// A finite number as `sign × magnitude × 2^pow2 / 10^pow10`, or an infinity.
enum Exact {
    NegativeInfinity,
    Finite {
        sign: i8,
        magnitude: u128,
        pow2: i32,
        pow10: u32,
    },
    PositiveInfinity,
}

impl Number {
    fn exact(self) -> Exact {
        match self {
            Number::Int(i) => Exact::Finite {
                sign: sign_of(i == 0, i < 0),
                magnitude: u128::from(i.unsigned_abs()),
                pow2: 0,
                pow10: 0,
            },
            Number::Float(f) if f.is_infinite() => {
                if f < 0.0 {
                    Exact::NegativeInfinity
                } else {
                    Exact::PositiveInfinity
                }
            }
            Number::Float(f) => {
                let bits = f.to_bits();
                let biased = (bits >> 52) & 0x7ff;
                let fraction = bits & ((1 << 52) - 1);
                let (mantissa, pow2) = if biased == 0 {
                    (fraction, -1074)
                } else {
                    // biased ≤ 0x7fe here, so the subtraction fits an i32.
                    (
                        fraction | (1 << 52),
                        i32::try_from(biased).unwrap_or(0) - 1075,
                    )
                };
                Exact::Finite {
                    sign: sign_of(mantissa == 0, f.is_sign_negative()),
                    magnitude: u128::from(mantissa),
                    pow2,
                    pow10: 0,
                }
            }
            Number::Decimal(d) => match d.parts() {
                Some((coefficient, scale)) => Exact::Finite {
                    sign: sign_of(coefficient == 0, coefficient < 0),
                    magnitude: coefficient.unsigned_abs(),
                    pow2: 0,
                    pow10: scale,
                },
                None if d < Decimal::from(0) => Exact::NegativeInfinity,
                None => Exact::PositiveInfinity,
            },
        }
    }

    /// The exact order of two numbers of any kinds: `2^53 + 1` is greater than the
    /// float `2^53`, and the decimal `0.1` differs from the float `0.1`.
    #[must_use]
    pub fn compare(self, other: Self) -> Ordering {
        match (self.exact(), other.exact()) {
            (Exact::NegativeInfinity, Exact::NegativeInfinity)
            | (Exact::PositiveInfinity, Exact::PositiveInfinity) => Ordering::Equal,
            (Exact::NegativeInfinity, _) | (_, Exact::PositiveInfinity) => Ordering::Less,
            (Exact::PositiveInfinity, _) | (_, Exact::NegativeInfinity) => Ordering::Greater,
            (
                Exact::Finite {
                    sign: a_sign,
                    magnitude: a,
                    pow2: a2,
                    pow10: a10,
                },
                Exact::Finite {
                    sign: b_sign,
                    magnitude: b,
                    pow2: b2,
                    pow10: b10,
                },
            ) => {
                if a_sign != b_sign || a_sign == 0 {
                    return a_sign.cmp(&b_sign);
                }
                // a × 2^a2 / 10^a10  vs  b × 2^b2 / 10^b10, scaled to whole numbers.
                let low = a2.min(b2);
                let left = BigUint::from(a).shl(a2.abs_diff(low)).mul_pow10(b10);
                let right = BigUint::from(b).shl(b2.abs_diff(low)).mul_pow10(a10);
                let order = left.cmp(&right);
                if a_sign < 0 { order.reverse() } else { order }
            }
        }
    }

    /// The number as a float, as arithmetic needs it (`spec/ast.md`, *Node
    /// semantics*): decimals are correctly rounded.
    #[must_use]
    pub fn to_f64(self) -> f64 {
        match self {
            // Rounds to nearest, ties to even, like any int-to-float conversion.
            #[allow(clippy::cast_precision_loss)]
            Number::Int(i) => i as f64,
            Number::Float(f) => f,
            Number::Decimal(d) => d.to_f64(),
        }
    }
}

/// The exact sum of numbers of any kinds, with no limit on its size or digits,
/// rounded once (`spec/rules.md`, *`COLLECT` aggregates*): a float is the exact
/// binary fraction it holds, which is also a decimal fraction, since
/// `2^-k = 5^k × 10^-k`.
#[derive(Debug)]
pub(crate) struct ExactSum {
    /// The largest scale among the terms, decimal or binary.
    scale: u32,
    /// The positive and the negative terms, each at `scale`.
    positive: BigUint,
    negative: BigUint,
    /// The infinities met, positive and negative.
    infinities: (bool, bool),
    /// The widest kind among the terms, which the total takes.
    kind: SumKind,
}

/// The kind of a sum: int until a decimal is met, decimal until a float is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum SumKind {
    Int,
    Decimal,
    Float,
}

/// A finite float as `(negative, mantissa, exponent)`, its value
/// `mantissa × 2^exponent`.
fn binary(f: f64) -> (bool, u128, i32) {
    let bits = f.to_bits();
    let biased = (bits >> 52) & 0x7ff;
    let fraction = bits & ((1 << 52) - 1);
    let (mantissa, exponent) = if biased == 0 {
        (fraction, -1074)
    } else {
        // biased ≤ 0x7fe for a finite float, so this fits an i32.
        (
            fraction | (1 << 52),
            i32::try_from(biased).unwrap_or(0) - 1075,
        )
    };
    (f.is_sign_negative(), u128::from(mantissa), exponent)
}

impl ExactSum {
    /// The sum of `terms`.
    pub(crate) fn of(terms: &[Number]) -> Self {
        let scale = terms
            .iter()
            .filter_map(|n| match n {
                Number::Decimal(d) => d.parts().map(|(_, s)| s),
                Number::Float(f) if f.is_finite() => Some(binary(*f).2.min(0).unsigned_abs()),
                _ => None,
            })
            .max()
            .unwrap_or(0);
        let mut sum = Self {
            scale,
            positive: BigUint::from(0),
            negative: BigUint::from(0),
            infinities: (false, false),
            kind: SumKind::Int,
        };
        for term in terms {
            let (negative, magnitude) = match *term {
                Number::Int(i) => (
                    i < 0,
                    BigUint::from(u128::from(i.unsigned_abs())).mul_pow10(scale),
                ),
                Number::Decimal(d) => {
                    sum.kind = sum.kind.max(SumKind::Decimal);
                    let Some((c, s)) = d.parts() else {
                        sum.infinity(d < Decimal::from(0));
                        continue;
                    };
                    (c < 0, BigUint::from(c.unsigned_abs()).mul_pow10(scale - s))
                }
                Number::Float(f) => {
                    sum.kind = SumKind::Float;
                    if f.is_infinite() {
                        sum.infinity(f < 0.0);
                        continue;
                    }
                    let (negative, mantissa, exponent) = binary(f);
                    let magnitude = BigUint::from(mantissa);
                    let magnitude = if exponent >= 0 {
                        magnitude.shl(exponent.unsigned_abs()).mul_pow10(scale)
                    } else {
                        // m × 2^e × 10^scale = m × 5^-e × 10^(scale + e), and
                        // scale ≥ -e.
                        let k = exponent.unsigned_abs();
                        magnitude.mul_pow5(k).mul_pow10(scale - k)
                    };
                    (negative, magnitude)
                }
            };
            if negative {
                sum.negative = sum.negative.add(&magnitude);
            } else {
                sum.positive = sum.positive.add(&magnitude);
            }
        }
        sum
    }

    fn infinity(&mut self, negative: bool) {
        if negative {
            self.infinities.1 = true;
        } else {
            self.infinities.0 = true;
        }
    }

    /// The total, rounded once: a float when a float was among the terms, a
    /// decimal when a decimal was, and an int otherwise; `None` for a null
    /// (both infinities, an int outside 64 bits, a decimal that does not fit).
    pub(crate) fn total(&self) -> Option<Number> {
        match self.infinities {
            (true, true) => return None,
            (true, false) | (false, true) => {
                let negative = self.infinities.1;
                return Some(if self.kind == SumKind::Float {
                    Number::Float(if negative {
                        f64::NEG_INFINITY
                    } else {
                        f64::INFINITY
                    })
                } else if negative {
                    Number::Decimal(Decimal::NEG_INFINITY)
                } else {
                    Number::Decimal(Decimal::INFINITY)
                });
            }
            (false, false) => {}
        }
        let (magnitude, negative) = self.positive.abs_diff(&self.negative);
        let digits = magnitude.digits();
        if self.kind == SumKind::Float {
            if digits.is_empty() {
                return Some(Number::Float(0.0));
            }
            let digits: String = digits.iter().map(|&d| char::from(b'0' + d)).collect();
            let sign = if negative { "-" } else { "" };
            // Rust's float parsing is correctly rounded for any number of digits,
            // and this text is always a valid literal; beyond the largest float
            // it gives an infinity.
            return format!("{sign}{digits}e-{}", self.scale)
                .parse()
                .ok()
                .map(Number::Float);
        }
        if self.kind == SumKind::Decimal {
            return fit(negative, digits, self.scale, false, self.scale)
                .ok()
                .map(Number::Decimal);
        }
        // Only ints: scale 0.
        let mut total: i128 = 0;
        for d in digits {
            total = total.checked_mul(10)?.checked_add(i128::from(d))?;
        }
        let total = if negative { -total } else { total };
        i64::try_from(total).ok().map(Number::Int)
    }
}

fn sign_of(zero: bool, negative: bool) -> i8 {
    if zero {
        0
    } else if negative {
        -1
    } else {
        1
    }
}

#[cfg(test)]
// Exact float equality is what these tests check.
#[allow(clippy::float_cmp)]
mod tests {
    use std::cmp::Ordering::{Equal, Greater, Less};

    use super::Number::{self, Decimal as D, Float, Int};
    use crate::value::Decimal;

    fn dec(text: &str) -> Number {
        D(Decimal::parse(text).unwrap())
    }

    #[test]
    fn ints_and_floats_compare_exactly() {
        assert_eq!(Int(1).compare(Float(1.0)), Equal);
        assert_eq!(
            Int((1 << 53) + 1).compare(Float(9_007_199_254_740_992.0)),
            Greater
        );
        assert_eq!(Int(i64::MAX).compare(Float(2_f64.powi(63))), Less);
        assert_eq!(Int(-3).compare(Float(-2.5)), Less);
        assert_eq!(Int(0).compare(Float(-0.0)), Equal);
        assert_eq!(Float(-0.0).compare(Float(0.0)), Equal);
        assert_eq!(Int(i64::MIN).compare(Float(f64::NEG_INFINITY)), Greater);
        assert_eq!(Float(5e-324).compare(Int(0)), Greater);
    }

    #[test]
    fn decimals_compare_exactly_with_floats_and_ints() {
        assert_ne!(dec("0.1").compare(Float(0.1)), Equal);
        assert_eq!(dec("0.1").compare(Float(0.1)), Less);
        assert_eq!(dec("0.5").compare(Float(0.5)), Equal);
        assert_eq!(dec("2.00").compare(Int(2)), Equal);
        assert_eq!(dec("-1.5").compare(Int(-1)), Less);
        assert_eq!(dec("inf").compare(Float(f64::INFINITY)), Equal);
        assert_eq!(dec("-inf").compare(Float(-1e308)), Less);
        // The float 1e38 is 99999999999999997748809823456034029568.
        assert_eq!(dec(&"9".repeat(38)).compare(Float(1e38)), Greater);
        assert_eq!(
            dec("0.000000000000000000000000000000000001").compare(Float(1e-36)),
            Greater
        );
        assert_eq!(dec("0").compare(Float(-0.0)), Equal);
    }

    fn sum(terms: &[Number]) -> String {
        match super::ExactSum::of(terms).total() {
            None => "null".into(),
            Some(Int(i)) => format!("int {i}"),
            Some(Float(f)) => format!("float {f:?}"),
            Some(D(d)) => format!("decimal {d}"),
        }
    }

    #[test]
    fn exact_sums_round_once() {
        assert_eq!(sum(&[dec("0.1"), dec("0.2")]), "decimal 0.3");
        assert_eq!(sum(&[]), "int 0");
        assert_eq!(
            sum(&[Int(i64::MAX), Int(1), Int(-1)]),
            format!("int {}", i64::MAX)
        );
        assert_eq!(sum(&[Int(i64::MAX), Int(1)]), "null");
        let max = "9".repeat(38);
        assert_eq!(sum(&[dec(&max), Int(1), Int(-1)]), format!("decimal {max}"));
        assert_eq!(sum(&[dec(&max), dec("1")]), "null");
        assert_eq!(sum(&[dec("-1.5"), dec("0.25")]), "decimal -1.25");
        // Floats are exact too, and the order never matters.
        assert_eq!(
            sum(&[Int(1), Int(1), Float(1e16)]),
            "float 1.0000000000000002e16"
        );
        assert_eq!(
            sum(&[Float(1e16), Int(1), Int(1)]),
            "float 1.0000000000000002e16"
        );
        assert_eq!(sum(&[Float(0.1), Float(0.2)]), "float 0.30000000000000004");
        assert_eq!(sum(&[Float(0.1), dec("0.2"), Float(-0.1)]), "float 0.2");
        assert_eq!(
            sum(&[Float(1e308), Float(1e308), Float(-1e308)]),
            "float 1e308"
        );
        assert_eq!(sum(&[Float(1e308), Float(1e308)]), "float inf");
        assert_eq!(sum(&[Float(5e-324), Float(-5e-324)]), "float 0.0");
        assert_eq!(sum(&[Float(5e-324), dec("1")]), "float 1.0");
        assert_eq!(sum(&[dec(&max), Int(1), Float(0.0)]), "float 1e38");
        // Infinities.
        assert_eq!(sum(&[dec("inf"), Int(1)]), "decimal inf");
        assert_eq!(sum(&[dec("-inf"), Int(1)]), "decimal -inf");
        assert_eq!(sum(&[dec("inf"), Float(1.0)]), "float inf");
        assert_eq!(sum(&[Float(f64::NEG_INFINITY)]), "float -inf");
        assert_eq!(sum(&[dec("inf"), Float(f64::NEG_INFINITY)]), "null");
    }

    #[test]
    fn to_f64() {
        assert_eq!(dec("0.1").to_f64(), 0.1);
        assert_eq!(Int(i64::MAX).to_f64(), 2_f64.powi(63));
    }
}
