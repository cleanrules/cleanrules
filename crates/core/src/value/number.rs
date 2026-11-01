//! Numbers of the three kinds, compared exactly by value (`spec/ast.md`, *Types*).

use std::cmp::Ordering;

use super::bignum::BigUint;
use super::decimal::Decimal;

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

    #[test]
    fn to_f64() {
        assert_eq!(dec("0.1").to_f64(), 0.1);
        assert_eq!(Int(i64::MAX).to_f64(), 2_f64.powi(63));
    }
}
