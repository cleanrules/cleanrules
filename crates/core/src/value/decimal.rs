//! Exact decimals of up to 38 significant digits (`spec/data-model.md`, *Decimals*).

use std::cmp::Ordering;
use std::fmt;
use std::hash::{Hash, Hasher};

use super::bignum::BigUint;

/// The largest number of digits a coefficient or a scale may have.
pub const MAX_DIGITS: u32 = 38;

/// `10^38`: coefficients are strictly smaller in absolute value.
const COEFFICIENT_LIMIT: i128 = 10_i128.pow(MAX_DIGITS);

/// An exact decimal: a coefficient `c` with `|c| < 10^38` and a scale `s` from 0
/// to 38, whose value is `c × 10^-s`, or one of the two infinities.
///
/// Equality and ordering are by value, so `1.0` equals `1.00`; the scale is kept
/// for display. A zero has no sign.
#[derive(Clone, Copy, Debug)]
pub struct Decimal {
    repr: Repr,
}

#[derive(Clone, Copy, Debug)]
enum Repr {
    Finite { coefficient: i128, scale: u8 },
    Infinite { negative: bool },
}

/// Why a decimal could not be built.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum DecimalError {
    /// The text is not a decimal literal.
    #[error("not a decimal literal")]
    Malformed,
    /// The value needs more than 38 significant digits, or more than 38 digits
    /// after the point.
    #[error("the decimal does not fit in 38 digits")]
    DoesNotFit,
}

impl Decimal {
    /// Positive infinity.
    pub const INFINITY: Self = Self {
        repr: Repr::Infinite { negative: false },
    };
    /// Negative infinity.
    pub const NEG_INFINITY: Self = Self {
        repr: Repr::Infinite { negative: true },
    };

    /// Build `coefficient × 10^-scale`.
    ///
    /// # Errors
    ///
    /// [`DecimalError::DoesNotFit`] when `|coefficient| ≥ 10^38` or `scale > 38`.
    pub fn new(coefficient: i128, scale: u32) -> Result<Self, DecimalError> {
        if coefficient.unsigned_abs() >= COEFFICIENT_LIMIT.unsigned_abs() || scale > MAX_DIGITS {
            return Err(DecimalError::DoesNotFit);
        }
        // The scale fits in a u8: it is at most 38.
        let scale = u8::try_from(scale).map_err(|_| DecimalError::DoesNotFit)?;
        Ok(Self {
            repr: Repr::Finite { coefficient, scale },
        })
    }

    /// Read a decimal literal: `-?[0-9]+(\.[0-9]+)?([eE][+-]?[0-9]+)?`, `inf` or `-inf`.
    ///
    /// The scale is the number of digits after the point, minus the exponent, never
    /// negative. When the value does not fit, trailing zeros are dropped only as far
    /// as needed (`1.` followed by 40 zeros is `1` with scale 37); the value is never
    /// rounded.
    ///
    /// # Errors
    ///
    /// [`DecimalError::Malformed`] for text outside the grammar, and
    /// [`DecimalError::DoesNotFit`] for a value that needs more than 38 digits.
    pub fn parse(text: &str) -> Result<Self, DecimalError> {
        match text {
            "inf" => return Ok(Self::INFINITY),
            "-inf" => return Ok(Self::NEG_INFINITY),
            _ => {}
        }
        let (negative, body) = match text.strip_prefix('-') {
            Some(rest) => (true, rest),
            None => (false, text),
        };
        let (mantissa, exponent) = match body.find(['e', 'E']) {
            Some(at) => (&body[..at], parse_exponent(&body[at + 1..])?),
            None => (body, 0),
        };
        let (integer, fraction) = match mantissa.split_once('.') {
            Some((integer, fraction)) => {
                if fraction.is_empty() {
                    return Err(DecimalError::Malformed);
                }
                (integer, fraction)
            }
            None => (mantissa, ""),
        };
        if integer.is_empty() || !all_digits(integer) || !all_digits(fraction) {
            return Err(DecimalError::Malformed);
        }
        // digits × 10^(exponent - fraction digits)
        let mut digits: Vec<u8> = integer
            .bytes()
            .chain(fraction.bytes())
            .map(|b| b - b'0')
            .collect();
        let fraction_len = i64::try_from(fraction.len()).map_err(|_| DecimalError::DoesNotFit)?;
        let mut scale = fraction_len
            .checked_sub(exponent)
            .ok_or(DecimalError::DoesNotFit)?;
        if scale < 0 {
            // A positive exponent: append zeros, scale 0.
            let zeros = usize::try_from(-scale).map_err(|_| DecimalError::DoesNotFit)?;
            if zeros > MAX_DIGITS as usize + 1 && digits.iter().any(|&d| d != 0) {
                return Err(DecimalError::DoesNotFit);
            }
            digits.extend(std::iter::repeat_n(0, zeros.min(MAX_DIGITS as usize + 1)));
            scale = 0;
        }
        // Leading zeros carry no digits of the coefficient.
        let first = digits.iter().position(|&d| d != 0).unwrap_or(digits.len());
        digits.drain(..first);
        // Drop trailing zeros only while the value does not fit.
        while (scale > i64::from(MAX_DIGITS) || digits.len() > MAX_DIGITS as usize)
            && scale > 0
            && digits.last() == Some(&0)
        {
            digits.pop();
            scale -= 1;
        }
        if digits.is_empty() {
            // Zero: keep the scale, clamped to what fits.
            let scale = u32::try_from(scale.min(i64::from(MAX_DIGITS)))
                .map_err(|_| DecimalError::DoesNotFit)?;
            return Self::new(0, scale);
        }
        if scale > i64::from(MAX_DIGITS) || digits.len() > MAX_DIGITS as usize {
            return Err(DecimalError::DoesNotFit);
        }
        let mut coefficient: i128 = 0;
        for d in digits {
            coefficient = coefficient * 10 + i128::from(d);
        }
        if negative {
            coefficient = -coefficient;
        }
        let scale = u32::try_from(scale).map_err(|_| DecimalError::DoesNotFit)?;
        Self::new(coefficient, scale)
    }

    /// Whether the decimal is one of the infinities.
    #[must_use]
    pub fn is_infinite(self) -> bool {
        matches!(self.repr, Repr::Infinite { .. })
    }

    /// The coefficient and the scale, or `None` for an infinity.
    #[must_use]
    pub fn parts(self) -> Option<(i128, u32)> {
        match self.repr {
            Repr::Finite { coefficient, scale } => Some((coefficient, u32::from(scale))),
            Repr::Infinite { .. } => None,
        }
    }

    /// The nearest binary64, ties to even (`spec/data-model.md`, *Decimals*).
    #[must_use]
    pub fn to_f64(self) -> f64 {
        match self.repr {
            Repr::Infinite { negative } => {
                if negative {
                    f64::NEG_INFINITY
                } else {
                    f64::INFINITY
                }
            }
            // Rust's float parsing is correctly rounded, and the canonical text is a
            // plain decimal literal, so this is exactly the nearest float.
            Repr::Finite { .. } => self.to_string().parse().unwrap_or(f64::NAN),
        }
    }

    /// The exact difference `self − other`, with the larger of the two scales, or
    /// `None` when it does not fit in 38 digits or an operand is infinite
    /// (`spec/explain.md`, *Counterfactuals*).
    #[must_use]
    pub fn checked_sub(self, other: Self) -> Option<Self> {
        let ((a, sa), (b, sb)) = (self.parts()?, other.parts()?);
        let scale = sa.max(sb);
        let a = a.checked_mul(10_i128.checked_pow(scale - sa)?)?;
        let b = b.checked_mul(10_i128.checked_pow(scale - sb)?)?;
        Self::new(a.checked_sub(b)?, scale).ok()
    }

    /// The exact value as `(negative, magnitude, scale)`, for comparisons.
    fn exact(self) -> Option<(bool, u128, u32)> {
        self.parts().map(|(c, s)| (c < 0, c.unsigned_abs(), s))
    }
}

fn parse_exponent(text: &str) -> Result<i64, DecimalError> {
    let digits = text.strip_prefix(['+', '-']).unwrap_or(text);
    if digits.is_empty() || !all_digits(digits) {
        return Err(DecimalError::Malformed);
    }
    // An exponent too large for an i64 cannot give a value that fits.
    text.parse().map_err(|_| DecimalError::DoesNotFit)
}

fn all_digits(text: &str) -> bool {
    text.bytes().all(|b| b.is_ascii_digit())
}

impl From<i64> for Decimal {
    fn from(value: i64) -> Self {
        Self {
            repr: Repr::Finite {
                coefficient: i128::from(value),
                scale: 0,
            },
        }
    }
}

/// The canonical text: plain notation, no exponent, no leading zeros, as many
/// fraction digits as the scale, and no sign on zero (`spec/json-format.md`).
impl fmt::Display for Decimal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (coefficient, scale) = match self.repr {
            Repr::Infinite { negative } => {
                return f.write_str(if negative { "-inf" } else { "inf" });
            }
            Repr::Finite { coefficient, scale } => (coefficient, usize::from(scale)),
        };
        let digits = coefficient.unsigned_abs().to_string();
        let digits = format!("{digits:0>width$}", width = scale + 1);
        let (integer, fraction) = digits.split_at(digits.len() - scale);
        if coefficient < 0 {
            f.write_str("-")?;
        }
        f.write_str(integer)?;
        if scale > 0 {
            write!(f, ".{fraction}")?;
        }
        Ok(())
    }
}

impl PartialEq for Decimal {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}

impl Eq for Decimal {}

impl PartialOrd for Decimal {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Decimal {
    fn cmp(&self, other: &Self) -> Ordering {
        match (self.repr, other.repr) {
            (Repr::Infinite { negative: a }, Repr::Infinite { negative: b }) => b.cmp(&a),
            (Repr::Infinite { negative }, Repr::Finite { .. }) => {
                if negative {
                    Ordering::Less
                } else {
                    Ordering::Greater
                }
            }
            (Repr::Finite { .. }, Repr::Infinite { negative }) => {
                if negative {
                    Ordering::Greater
                } else {
                    Ordering::Less
                }
            }
            (Repr::Finite { .. }, Repr::Finite { .. }) => {
                let (Some(a), Some(b)) = (self.exact(), other.exact()) else {
                    return Ordering::Equal;
                };
                compare_scaled(a, b)
            }
        }
    }
}

/// Hash the value, not the representation: equal decimals hash alike.
impl Hash for Decimal {
    fn hash<H: Hasher>(&self, state: &mut H) {
        match self.repr {
            Repr::Infinite { negative } => (1_u8, negative).hash(state),
            Repr::Finite {
                mut coefficient,
                mut scale,
            } => {
                while scale > 0 && coefficient % 10 == 0 {
                    coefficient /= 10;
                    scale -= 1;
                }
                (0_u8, coefficient, scale).hash(state);
            }
        }
    }
}

/// Compare two exact values `(negative, magnitude, scale)`.
pub(super) fn compare_scaled(a: (bool, u128, u32), b: (bool, u128, u32)) -> Ordering {
    let (a_negative, a_magnitude, a_scale) = a;
    let (b_negative, b_magnitude, b_scale) = b;
    let a_zero = a_magnitude == 0;
    let b_zero = b_magnitude == 0;
    if a_zero && b_zero {
        return Ordering::Equal;
    }
    let a_sign = if a_zero {
        0
    } else if a_negative {
        -1
    } else {
        1
    };
    let b_sign = if b_zero {
        0
    } else if b_negative {
        -1
    } else {
        1
    };
    if a_sign != b_sign {
        return a_sign.cmp(&b_sign);
    }
    // Same sign: compare a_magnitude × 10^b_scale with b_magnitude × 10^a_scale.
    let left = BigUint::from(a_magnitude).mul_pow10(b_scale);
    let right = BigUint::from(b_magnitude).mul_pow10(a_scale);
    let order = left.cmp(&right);
    if a_sign < 0 { order.reverse() } else { order }
}

#[cfg(test)]
// Exact float equality is what these tests check.
#[allow(clippy::float_cmp)]
mod tests {
    use super::{Decimal, DecimalError};

    fn d(text: &str) -> Decimal {
        Decimal::parse(text).unwrap()
    }

    #[test]
    fn canonical_text() {
        for (raw, canonical) in [
            ("007.50", "7.50"),
            ("1.5e-3", "0.0015"),
            ("1e2", "100"),
            ("1.5e3", "1500"),
            ("-0.00", "0.00"),
            ("1.00", "1.00"),
            ("-inf", "-inf"),
            ("inf", "inf"),
            ("0", "0"),
            ("-12.345", "-12.345"),
            ("1E+2", "100"),
        ] {
            assert_eq!(d(raw).to_string(), canonical, "{raw}");
        }
    }

    #[test]
    fn the_38_digit_limits() {
        assert_eq!(d(&"9".repeat(38)).to_string(), "9".repeat(38));
        assert_eq!(
            Decimal::parse(&format!("1{}", "0".repeat(38))),
            Err(DecimalError::DoesNotFit)
        );
        assert_eq!(d("1e37").to_string(), format!("1{}", "0".repeat(37)));
        assert_eq!(Decimal::parse("1e38"), Err(DecimalError::DoesNotFit));
        assert_eq!(
            Decimal::parse(&format!("0.{}1", "0".repeat(38))),
            Err(DecimalError::DoesNotFit)
        );
        // Trailing zeros are dropped only as far as needed: 1 fits with scale 37.
        assert_eq!(
            d(&format!("1.{}", "0".repeat(40))).to_string(),
            format!("1.{}", "0".repeat(37))
        );
        assert_eq!(
            Decimal::parse("1e999999999999999999999"),
            Err(DecimalError::DoesNotFit)
        );
        assert_eq!(d("0e999999").to_string(), "0");
    }

    #[test]
    fn malformed_text() {
        for raw in [
            "", "1,5", ".5", "5.", "+5", "+inf", "1e", "1e+", "--1", "nan", "1.5.2", " 1",
        ] {
            assert_eq!(Decimal::parse(raw), Err(DecimalError::Malformed), "{raw:?}");
        }
    }

    #[test]
    fn equality_and_order_by_value() {
        assert_eq!(d("1.0"), d("1.00"));
        assert_eq!(d("-0.00"), d("0"));
        assert!(d("0.1") < d(&format!("0.1{}1", "0".repeat(36))));
        assert!(d("-2") < d("-1.5"));
        assert!(d("-inf") < d(&format!("-{}", "9".repeat(38))));
        assert!(d("inf") > d(&"9".repeat(38)));
        assert_eq!(d("inf"), d("inf"));
    }

    #[test]
    fn equal_values_hash_alike() {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        let hash = |x: Decimal| {
            let mut h = DefaultHasher::new();
            x.hash(&mut h);
            h.finish()
        };
        assert_eq!(hash(d("1.0")), hash(d("1.000")));
        assert_eq!(hash(d("0")), hash(d("-0.00")));
    }

    #[test]
    fn exact_differences() {
        assert_eq!(
            d("10.05").checked_sub(d("9.9")).unwrap().to_string(),
            "0.15"
        );
        assert_eq!(d("1").checked_sub(d("0.001")).unwrap().to_string(), "0.999");
        assert!(d(&"9".repeat(38)).checked_sub(d("-1")).is_none());
        assert!(d("inf").checked_sub(d("1")).is_none());
    }

    #[test]
    fn to_f64_is_correctly_rounded() {
        assert_eq!(d("0.1").to_f64(), 0.1);
        assert_eq!(d("9007199254740993").to_f64(), 9_007_199_254_740_992.0);
        assert_eq!(d("9007199254740995").to_f64(), 9_007_199_254_740_996.0);
        assert_eq!(d("-inf").to_f64(), f64::NEG_INFINITY);
        assert_eq!(d(&"9".repeat(38)).to_f64(), 1e38);
    }
}
