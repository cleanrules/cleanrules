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
#[non_exhaustive]
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
    /// (`spec/explain.md`, *Counterfactuals*). It never rounds, unlike
    /// [`Decimal::try_sub`], which rounds a difference that does not fit.
    #[must_use]
    pub fn exact_sub(self, other: Self) -> Option<Self> {
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

    /// The sign: -1, 0 or 1.
    fn signum(self) -> i8 {
        match self.repr {
            Repr::Infinite { negative } => {
                if negative {
                    -1
                } else {
                    1
                }
            }
            Repr::Finite { coefficient, .. } => match coefficient.cmp(&0) {
                Ordering::Less => -1,
                Ordering::Equal => 0,
                Ordering::Greater => 1,
            },
        }
    }

    /// The infinity of a sign.
    fn infinity(negative: bool) -> Self {
        Self {
            repr: Repr::Infinite { negative },
        }
    }

    /// `self + other`, exact when it fits and otherwise **rounded** half to even
    /// to 38 significant digits (`spec/ast.md`, *Decimal arithmetic*): the
    /// `try_` methods may round, and fail only when no decimal can hold the
    /// result. For a difference that must be exact, see [`Decimal::exact_sub`].
    ///
    /// # Errors
    ///
    /// [`ArithmeticError::Overflow`] when the result does not fit at any
    /// scale, and [`ArithmeticError::InvalidOperation`] for `inf + -inf`.
    pub fn try_add(self, other: Self) -> Result<Self, ArithmeticError> {
        self.add_signed(other, false)
    }

    /// `self − other`, as [`Decimal::try_add`] rounds it.
    ///
    /// # Errors
    ///
    /// [`ArithmeticError::Overflow`] when the result does not fit at any
    /// scale, and [`ArithmeticError::InvalidOperation`] for `inf - inf`.
    pub fn try_sub(self, other: Self) -> Result<Self, ArithmeticError> {
        self.add_signed(other, true)
    }

    fn add_signed(self, other: Self, subtract: bool) -> Result<Self, ArithmeticError> {
        let flip = |negative: bool| negative != subtract;
        let ((a, sa), (b, sb)) = match (self.repr, other.repr) {
            (Repr::Infinite { negative: x }, Repr::Infinite { negative: y }) => {
                return if x == flip(y) {
                    Ok(Self::infinity(x))
                } else {
                    Err(ArithmeticError::InvalidOperation)
                };
            }
            (Repr::Infinite { negative }, Repr::Finite { .. }) => {
                return Ok(Self::infinity(negative));
            }
            (Repr::Finite { .. }, Repr::Infinite { negative }) => {
                return Ok(Self::infinity(flip(negative)));
            }
            (
                Repr::Finite {
                    coefficient: a,
                    scale: sa,
                },
                Repr::Finite {
                    coefficient: b,
                    scale: sb,
                },
            ) => ((a, u32::from(sa)), (b, u32::from(sb))),
        };
        // |b| < 10^38, so its negation fits.
        let b = if subtract { -b } else { b };
        let scale = sa.max(sb);
        // The common case: the aligned sum fits in 128 bits and in 38 digits.
        let aligned = |c: i128, s: u32| c.checked_mul(10_i128.pow(scale - s));
        if let Some(sum) = aligned(a, sa)
            .zip(aligned(b, sb))
            .and_then(|(x, y)| x.checked_add(y))
            && let Ok(result) = Self::new(sum, scale)
        {
            return Ok(result);
        }
        let x = BigUint::from(a.unsigned_abs()).mul_pow10(scale - sa);
        let y = BigUint::from(b.unsigned_abs()).mul_pow10(scale - sb);
        let (magnitude, negative) = match (a < 0, b < 0) {
            (false, false) => (x.add(&y), false),
            (true, true) => (x.add(&y), true),
            (false, true) => x.abs_diff(&y),
            (true, false) => {
                let (magnitude, negative) = x.abs_diff(&y);
                (magnitude, !negative)
            }
        };
        fit(negative, magnitude.digits(), scale, false, scale)
    }

    /// `self × other`, as [`Decimal::try_add`] rounds it.
    ///
    /// # Errors
    ///
    /// [`ArithmeticError::Overflow`] when the result does not fit at any
    /// scale, and [`ArithmeticError::InvalidOperation`] for an infinity times zero.
    pub fn try_mul(self, other: Self) -> Result<Self, ArithmeticError> {
        let (Some((a, sa)), Some((b, sb))) = (self.parts(), other.parts()) else {
            let sign = self.signum() * other.signum();
            return if sign == 0 {
                Err(ArithmeticError::InvalidOperation)
            } else {
                Ok(Self::infinity(sign < 0))
            };
        };
        let scale = sa + sb;
        if let Some(product) = a.checked_mul(b)
            && let Ok(result) = Self::new(product, scale)
        {
            return Ok(result);
        }
        let magnitude = BigUint::from(a.unsigned_abs()).mul(&BigUint::from(b.unsigned_abs()));
        fit((a < 0) != (b < 0), magnitude.digits(), scale, false, scale)
    }

    /// `self ÷ other`: the exact quotient when it fits, with the scale closest
    /// to `max(a − b, 0)` for operand scales `a` and `b`, and otherwise the
    /// quotient rounded half to even to 38 significant digits, at most 38 after
    /// the point (`spec/ast.md`, *Decimal arithmetic*).
    ///
    /// ```
    /// use cleanrules::value::Decimal;
    ///
    /// let d = |text| Decimal::parse(text).unwrap();
    /// // Exact: the scale closest to max(2 − 0, 0) that writes it.
    /// assert_eq!(d("1.00").try_div(d("4")).unwrap().to_string(), "0.25");
    /// // Repeating: rounded half to even to 38 significant digits.
    /// assert_eq!(
    ///     d("1").try_div(d("3")).unwrap().to_string(),
    ///     format!("0.{}", "3".repeat(38))
    /// );
    /// ```
    ///
    /// # Errors
    ///
    /// [`ArithmeticError::DivisionByZero`] when `other` is zero, even if `self`
    /// is infinite; [`ArithmeticError::Overflow`] when the quotient does not
    /// fit at any scale; [`ArithmeticError::InvalidOperation`] for an infinity
    /// divided by an infinity.
    pub fn try_div(self, other: Self) -> Result<Self, ArithmeticError> {
        if other.signum() == 0 {
            return Err(ArithmeticError::DivisionByZero);
        }
        let ((a, sa), (b, sb)) = match (self.parts(), other.parts()) {
            (None, None) => return Err(ArithmeticError::InvalidOperation),
            (None, Some(_)) => return Ok(Self::infinity(self.signum() * other.signum() < 0)),
            // A finite decimal divided by an infinity: zero with the ideal scale,
            // in which the infinity counts as scale 0.
            (Some((_, sa)), None) => return fit(false, Vec::new(), sa, false, sa),
            (Some(a), Some(b)) => (a, b),
        };
        let ideal = sa.saturating_sub(sb);
        // a / b × 10^(sb − sa): the digits of a / b, with enough fraction digits
        // that the result reaches scale 39, one guard digit past the largest.
        let wanted = (MAX_DIGITS + 1 + sb).saturating_sub(sa);
        let (digits, fraction, inexact) = long_division(a.unsigned_abs(), b.unsigned_abs(), wanted);
        let negative = (a < 0) != (b < 0);
        // The quotient's scale is fraction + sa − sb; a negative one appends zeros.
        let scale = i64::from(fraction) + i64::from(sa) - i64::from(sb);
        let (digits, scale) = if scale < 0 {
            let mut digits = digits;
            if !digits.is_empty() {
                // −scale ≤ 38 here, because sb ≤ 38.
                digits.extend(std::iter::repeat_n(0, usize::try_from(-scale).unwrap_or(0)));
            }
            (digits, 0)
        } else {
            (
                digits,
                u32::try_from(scale).map_err(|_| ArithmeticError::Overflow)?,
            )
        };
        fit(negative, digits, scale, inexact, ideal)
    }

    /// `|self|`.
    #[must_use]
    pub fn abs(self) -> Self {
        if self.signum() < 0 { -self } else { self }
    }
}

/// `-self`. A zero has no sign, so `-(0.00)` is `0.00`.
impl std::ops::Neg for Decimal {
    type Output = Self;

    fn neg(self) -> Self {
        match self.repr {
            Repr::Infinite { negative } => Self::infinity(!negative),
            // |coefficient| < 10^38, so its negation fits.
            Repr::Finite { coefficient, scale } => Self {
                repr: Repr::Finite {
                    coefficient: -coefficient,
                    scale,
                },
            },
        }
    }
}

/// Why decimal arithmetic gave no decimal (`spec/ast.md`, *Arithmetic*). The
/// names follow Python's `decimal` signals.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum ArithmeticError {
    /// A division by zero.
    #[error("division by zero")]
    DivisionByZero,
    /// The result does not fit in 38 digits at any scale.
    #[error("the decimal result does not fit in 38 digits")]
    Overflow,
    /// The result is not a number: `inf - inf`, `inf * 0` or `inf / inf`.
    #[error("the result is not a number")]
    InvalidOperation,
}

impl ArithmeticError {
    /// The reason code of the null that evaluation gives instead
    /// (`spec/ast.md`, *Results and reasons*): `division_by_zero`,
    /// `decimal_overflow` or `nan_result`.
    #[must_use]
    pub fn code(self) -> &'static str {
        match self {
            ArithmeticError::DivisionByZero => "division_by_zero",
            ArithmeticError::Overflow => "decimal_overflow",
            ArithmeticError::InvalidOperation => "nan_result",
        }
    }
}

/// How [`Decimal::round`] drops digits (`spec/ast.md`, *Rounding*): the modes
/// of Java's `RoundingMode`, without `UNNECESSARY`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum RoundingMode {
    /// The nearest neighbour; a tie goes to the even one.
    #[default]
    HalfEven,
    /// The nearest neighbour; a tie goes away from zero.
    HalfUp,
    /// The nearest neighbour; a tie goes toward zero.
    HalfDown,
    /// Away from zero.
    Up,
    /// Toward zero.
    Down,
    /// Toward positive infinity.
    Ceiling,
    /// Toward negative infinity.
    Floor,
}

impl RoundingMode {
    /// Every mode, in the order of the spec.
    pub const ALL: [RoundingMode; 7] = [
        RoundingMode::HalfEven,
        RoundingMode::HalfUp,
        RoundingMode::HalfDown,
        RoundingMode::Up,
        RoundingMode::Down,
        RoundingMode::Ceiling,
        RoundingMode::Floor,
    ];

    /// The name in the format: `half_even`, `half_up`, `half_down`, `up`,
    /// `down`, `ceiling` or `floor`.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            RoundingMode::HalfEven => "half_even",
            RoundingMode::HalfUp => "half_up",
            RoundingMode::HalfDown => "half_down",
            RoundingMode::Up => "up",
            RoundingMode::Down => "down",
            RoundingMode::Ceiling => "ceiling",
            RoundingMode::Floor => "floor",
        }
    }

    /// The mode for a name, exactly as the format writes it.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|mode| mode.as_str() == name)
    }

    /// Whether to round the magnitude up, away from zero, given the value's
    /// sign, whether the dropped part is below, at or above half a step, and
    /// whether it is zero, and whether the kept last digit is odd.
    fn away(self, negative: bool, dropped: Ordering, exact: bool, odd: bool) -> bool {
        if exact {
            return false;
        }
        match self {
            RoundingMode::HalfEven => dropped.is_gt() || (dropped.is_eq() && odd),
            RoundingMode::HalfUp => dropped.is_ge(),
            RoundingMode::HalfDown => dropped.is_gt(),
            RoundingMode::Up => true,
            RoundingMode::Down => false,
            RoundingMode::Ceiling => !negative,
            RoundingMode::Floor => negative,
        }
    }
}

impl fmt::Display for RoundingMode {
    /// The name in the format, such as `half_even`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl Decimal {
    /// The decimal with exactly `scale` digits after the point, digits dropped
    /// as `mode` says (`spec/ast.md`, *Rounding*). An infinity is kept.
    ///
    /// ```
    /// use cleanrules::value::{Decimal, RoundingMode};
    ///
    /// let price = Decimal::parse("16.9915").unwrap();
    /// assert_eq!(price.round(2, RoundingMode::HalfEven).unwrap().to_string(), "16.99");
    /// assert_eq!(price.round(2, RoundingMode::Up).unwrap().to_string(), "17.00");
    /// assert_eq!(Decimal::from(5).round(2, RoundingMode::Down).unwrap().to_string(), "5.00");
    /// ```
    ///
    /// # Errors
    ///
    /// [`ArithmeticError::Overflow`] when the result's coefficient is 10^38 or
    /// more in absolute value, or `scale` is above 38.
    pub fn round(self, scale: u32, mode: RoundingMode) -> Result<Self, ArithmeticError> {
        let Some((coefficient, current)) = self.parts() else {
            return Ok(self);
        };
        if scale > MAX_DIGITS {
            return Err(ArithmeticError::Overflow);
        }
        if scale >= current {
            // Exact: pad with zeros.
            let padded = 10_i128
                .checked_pow(scale - current)
                .and_then(|factor| coefficient.checked_mul(factor))
                .ok_or(ArithmeticError::Overflow)?;
            return Self::new(padded, scale).map_err(|_| ArithmeticError::Overflow);
        }
        // 1 ≤ dropped places ≤ 38: the step 10^dropped fits in an i128.
        let step = 10_i128.pow(current - scale);
        let (kept, rest) = (coefficient / step, (coefficient % step).abs());
        let half = step / 2;
        let away = mode.away(coefficient < 0, rest.cmp(&half), rest == 0, kept % 2 != 0);
        // |kept| < 10^37, so one more fits.
        let rounded = if away {
            kept + coefficient.signum()
        } else {
            kept
        };
        Self::new(rounded, scale).map_err(|_| ArithmeticError::Overflow)
    }
}

/// The digits of `a / b` (`b > 0`): the integer part's, then up to `wanted`
/// fraction digits, stopping early when the division is exact. Returns the
/// digits without leading zeros, how many of them are fraction digits, and
/// whether a non-zero remainder was left (the quotient goes on).
fn long_division(a: u128, b: u128, wanted: u32) -> (Vec<u8>, u32, bool) {
    let quotient = a / b;
    let mut digits: Vec<u8> = if quotient == 0 {
        Vec::new()
    } else {
        quotient.to_string().bytes().map(|d| d - b'0').collect()
    };
    let mut remainder = a % b;
    let mut fraction = 0;
    while remainder != 0 && fraction < wanted {
        // The next digit is ⌊10 r / b⌋, and the new remainder 10 r mod b. 10 r
        // can exceed 128 bits when b is above 3.4 × 10^37, so r is added ten
        // times modulo b instead, counting the wraps: each sum is below 2 b.
        let mut digit = 0_u8;
        let mut accumulated = 0_u128;
        for _ in 0..10 {
            let room = b - accumulated;
            if remainder >= room {
                accumulated = remainder - room;
                digit += 1;
            } else {
                accumulated += remainder;
            }
        }
        remainder = accumulated;
        if !(digits.is_empty() && digit == 0) {
            digits.push(digit);
        }
        fraction += 1;
    }
    (digits, fraction, remainder != 0)
}

/// Write an exact result, or a truncated one, as a decimal (`spec/ast.md`,
/// *Decimal arithmetic*, steps 3 to 5).
///
/// `digits` are the coefficient's, without leading zeros, at `scale`; `inexact`
/// says that non-zero digits follow them, which only a division leaves, and
/// then always past scale 38. An exact result keeps the scale closest to
/// `ideal` that writes it and fits; any other is rounded half to even to the
/// largest scale that fits.
pub(super) fn fit(
    negative: bool,
    mut digits: Vec<u8>,
    mut scale: u32,
    inexact: bool,
    ideal: u32,
) -> Result<Decimal, ArithmeticError> {
    if !inexact {
        if digits.is_empty() {
            return Decimal::new(0, ideal.min(MAX_DIGITS)).map_err(|_| ArithmeticError::Overflow);
        }
        while scale > ideal && digits.last() == Some(&0) {
            digits.pop();
            scale -= 1;
        }
    }
    let length = u32::try_from(digits.len()).map_err(|_| ArithmeticError::Overflow)?;
    let drop = scale
        .saturating_sub(MAX_DIGITS)
        .max(length.saturating_sub(MAX_DIGITS));
    if drop > scale {
        // The integer part alone has more than 38 digits.
        return Err(ArithmeticError::Overflow);
    }
    if drop > 0 {
        let drop = drop as usize;
        if drop > digits.len() {
            // The dropped digits start with zeros the coefficient does not show.
            digits.splice(0..0, std::iter::repeat_n(0, drop - digits.len()));
        }
        let kept = digits.len() - drop;
        let first = digits[kept];
        let rest = inexact || digits[kept + 1..].iter().any(|&d| d != 0);
        let odd = kept > 0 && digits[kept - 1] % 2 == 1;
        digits.truncate(kept);
        scale -= u32::try_from(drop).map_err(|_| ArithmeticError::Overflow)?;
        if first > 5 || (first == 5 && (rest || odd)) {
            increment(&mut digits);
        }
        let first_non_zero = digits.iter().position(|&d| d != 0).unwrap_or(digits.len());
        digits.drain(..first_non_zero);
        if digits.len() > MAX_DIGITS as usize {
            // Rounding carried into a 39th digit: the coefficient is 10^38, whose
            // last digit is a zero that one less place drops exactly.
            if scale == 0 {
                return Err(ArithmeticError::Overflow);
            }
            digits.pop();
            scale -= 1;
        }
    }
    let mut coefficient: i128 = 0;
    for d in digits {
        // At most 38 digits: below 10^38, which fits.
        coefficient = coefficient * 10 + i128::from(d);
    }
    if negative {
        coefficient = -coefficient;
    }
    Decimal::new(coefficient, scale).map_err(|_| ArithmeticError::Overflow)
}

/// Add one to the number that `digits` spell, most significant first.
fn increment(digits: &mut Vec<u8>) {
    for d in digits.iter_mut().rev() {
        if *d == 9 {
            *d = 0;
        } else {
            *d += 1;
            return;
        }
    }
    digits.insert(0, 1);
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
    use super::{ArithmeticError, Decimal, DecimalError};

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
        assert_eq!(d("10.05").exact_sub(d("9.9")).unwrap().to_string(), "0.15");
        assert_eq!(d("1").exact_sub(d("0.001")).unwrap().to_string(), "0.999");
        assert!(d(&"9".repeat(38)).exact_sub(d("-1")).is_none());
        assert!(d("inf").exact_sub(d("1")).is_none());
    }

    fn op(f: fn(Decimal, Decimal) -> Result<Decimal, ArithmeticError>, a: &str, b: &str) -> String {
        f(d(a), d(b)).map_or_else(|e| format!("{e:?}"), |r| r.to_string())
    }

    #[test]
    fn exact_results_keep_the_ideal_scale() {
        assert_eq!(op(Decimal::try_add, "1.10", "2.205"), "3.305");
        assert_eq!(op(Decimal::try_add, "1.5", "1.5"), "3.0");
        assert_eq!(op(Decimal::try_sub, "1.50", "1.5"), "0.00");
        assert_eq!(op(Decimal::try_sub, "-1", "-1.5"), "0.5");
        assert_eq!(op(Decimal::try_mul, "19.99", "0.15"), "2.9985");
        assert_eq!(op(Decimal::try_mul, "-1.5", "0"), "0.0");
        assert_eq!(op(Decimal::try_div, "1.00", "4"), "0.25");
        assert_eq!(op(Decimal::try_div, "1.000", "4"), "0.250");
        assert_eq!(op(Decimal::try_div, "1", "0.25"), "4");
        assert_eq!(op(Decimal::try_div, "7.5", "2.5"), "3");
        assert_eq!(op(Decimal::try_div, "0.00", "3.0"), "0.0");
        assert_eq!(op(Decimal::try_div, "-6", "0.2"), "-30");
        assert_eq!(op(Decimal::try_div, "1", "0.0001"), "10000");
    }

    #[test]
    fn inexact_results_are_rounded_half_to_even() {
        assert_eq!(
            op(Decimal::try_div, "1", "3"),
            format!("0.{}", "3".repeat(38))
        );
        assert_eq!(
            op(Decimal::try_div, "-2", "3"),
            format!("-0.{}7", "6".repeat(37))
        );
        assert_eq!(
            op(Decimal::try_div, "10", "3"),
            format!("3.{}", "3".repeat(37))
        );
        // Ties go to the even neighbour, in both directions.
        assert_eq!(
            op(Decimal::try_div, &"9".repeat(38), "2"),
            format!("5{}", "0".repeat(37))
        );
        assert_eq!(
            op(Decimal::try_div, &format!("{}7", "9".repeat(37)), "2"),
            format!("4{}8", "9".repeat(36))
        );
        let e37 = format!("1{}", "0".repeat(37));
        assert_eq!(op(Decimal::try_add, &e37, "0.5"), e37);
        assert_eq!(
            op(Decimal::try_add, &e37, "-0.5"),
            format!("{}.5", "9".repeat(37))
        );
        assert_eq!(
            op(Decimal::try_sub, &e37, "0.06"),
            format!("{}.9", "9".repeat(37))
        );
        assert_eq!(
            op(Decimal::try_add, &format!("-{e37}"), "-0.51"),
            format!("-1{}1", "0".repeat(36))
        );
        // A divisor above 3.4 × 10^37: ten times the remainder needs 129 bits.
        let big = format!("9{}", "8".repeat(37));
        assert_eq!(
            op(Decimal::try_div, &format!("9{}7", "8".repeat(36)), &big),
            "0.99999999999999999999999999999999999999"
        );
        // Below 38 places: zero with scale 38, or the smallest step.
        let tiny = format!("0.{}1", "0".repeat(19));
        assert_eq!(
            op(Decimal::try_mul, &tiny, &tiny),
            format!("0.{}", "0".repeat(38))
        );
        assert_eq!(
            op(
                Decimal::try_mul,
                &format!("0.{}6", "0".repeat(19)),
                &format!("0.{}1", "0".repeat(18))
            ),
            format!("0.{}1", "0".repeat(37))
        );
        // Rounding carries into a 39th digit: one place fewer, exactly.
        assert_eq!(
            op(
                Decimal::try_add,
                &format!("9.{}", "9".repeat(37)),
                &format!("0.{}5", "0".repeat(37))
            ),
            format!("10.{}", "0".repeat(36))
        );
    }

    #[test]
    fn results_that_do_not_fit_overflow() {
        let max = "9".repeat(38);
        assert_eq!(op(Decimal::try_add, &max, "1"), "Overflow");
        assert_eq!(op(Decimal::try_sub, &format!("-{max}"), "0.5"), "Overflow");
        assert_eq!(op(Decimal::try_add, &max, "0.4"), max);
        let e20 = format!("1{}", "0".repeat(20));
        assert_eq!(op(Decimal::try_mul, &e20, &e20), "Overflow");
        assert_eq!(
            op(Decimal::try_div, &format!("1{}", "0".repeat(37)), "0.01"),
            "Overflow"
        );
        // Rounding up the last integer digit carries past 38 digits.
        assert_eq!(op(Decimal::try_add, &max, "0.5"), "Overflow");
    }

    #[test]
    fn zero_and_infinities() {
        assert_eq!(op(Decimal::try_div, "1", "0.00"), "DivisionByZero");
        assert_eq!(op(Decimal::try_div, "inf", "0"), "DivisionByZero");
        assert_eq!(op(Decimal::try_div, "inf", "-inf"), "InvalidOperation");
        assert_eq!(op(Decimal::try_div, "-inf", "2"), "-inf");
        assert_eq!(op(Decimal::try_div, "1.50", "-inf"), "0.00");
        assert_eq!(op(Decimal::try_add, "inf", "-inf"), "InvalidOperation");
        assert_eq!(op(Decimal::try_sub, "inf", "-inf"), "inf");
        assert_eq!(op(Decimal::try_sub, "1", "inf"), "-inf");
        assert_eq!(op(Decimal::try_add, "-inf", "5"), "-inf");
        assert_eq!(op(Decimal::try_mul, "inf", "0.0"), "InvalidOperation");
        assert_eq!(op(Decimal::try_mul, "-inf", "-inf"), "inf");
        assert_eq!((-d("0.00")).to_string(), "0.00");
        assert_eq!(d("-1.50").abs().to_string(), "1.50");
        assert_eq!(d("1.5").abs().to_string(), "1.5");
        assert_eq!(-d("-inf"), d("inf"));
        assert_eq!(d("-inf").abs(), d("inf"));
    }

    #[test]
    fn rounding_modes() {
        use super::RoundingMode::{Ceiling, Down, Floor, HalfDown, HalfEven, HalfUp, Up};
        let r = |text: &str, scale, mode| {
            d(text)
                .round(scale, mode)
                .map_or_else(|e| format!("{e:?}"), |r| r.to_string())
        };
        for (mode, positive, negative) in [
            (HalfEven, "2.34", "-2.34"),
            (HalfUp, "2.35", "-2.35"),
            (HalfDown, "2.34", "-2.34"),
            (Up, "2.35", "-2.35"),
            (Down, "2.34", "-2.34"),
            (Ceiling, "2.35", "-2.34"),
            (Floor, "2.34", "-2.35"),
        ] {
            assert_eq!(r("2.345", 2, mode), positive, "{mode:?}");
            assert_eq!(r("-2.345", 2, mode), negative, "{mode:?}");
            assert_eq!(r("2.340", 2, mode), "2.34", "{mode:?}");
            assert_eq!(
                mode.as_str()
                    .parse::<String>()
                    .ok()
                    .and_then(|n| super::RoundingMode::from_name(&n)),
                Some(mode)
            );
        }
        assert_eq!(r("2.355", 2, HalfEven), "2.36");
        assert_eq!(r("-0.001", 2, HalfEven), "0.00");
        assert_eq!(r("1.5", 3, HalfEven), "1.500");
        assert_eq!(r("inf", 2, Up), "inf");
        assert_eq!(r(&format!("1{}", "0".repeat(37)), 1, HalfEven), "Overflow");
        assert_eq!(r("1", 39, HalfEven), "Overflow");
        assert_eq!(super::RoundingMode::from_name("HALF_UP"), None);
        assert_eq!(super::RoundingMode::default(), HalfEven);
        assert_eq!(ArithmeticError::DivisionByZero.code(), "division_by_zero");
        assert_eq!(ArithmeticError::Overflow.code(), "decimal_overflow");
        assert_eq!(ArithmeticError::InvalidOperation.code(), "nan_result");
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
