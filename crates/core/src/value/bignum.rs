//! Just enough unsigned big-integer arithmetic for exact numbers.
//!
//! Comparing a float with a decimal, or two decimals of different scales, means
//! comparing products such as `m × 2^e × 10^s` that overflow 128 bits; decimal
//! arithmetic (`spec/ast.md`, *Decimal arithmetic*) adds and multiplies
//! coefficients of up to 38 digits each, and writes the exact result as decimal
//! digits before it is rounded. Only multiplication, addition, subtraction,
//! shifts, comparison and conversion to decimal digits are needed.

use std::cmp::Ordering;

/// An unsigned integer as little-endian 32-bit limbs, without leading zero limbs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct BigUint {
    limbs: Vec<u32>,
}

impl From<u128> for BigUint {
    fn from(mut value: u128) -> Self {
        let mut limbs = Vec::with_capacity(4);
        while value > 0 {
            // Truncation keeps the low 32 bits, which is the point.
            #[allow(clippy::cast_possible_truncation)]
            limbs.push(value as u32);
            value >>= 32;
        }
        Self { limbs }
    }
}

impl BigUint {
    /// `self × factor`.
    fn mul_small(mut self, factor: u32) -> Self {
        let mut carry: u64 = 0;
        for limb in &mut self.limbs {
            let product = u64::from(*limb) * u64::from(factor) + carry;
            #[allow(clippy::cast_possible_truncation)]
            {
                *limb = product as u32;
            }
            carry = product >> 32;
        }
        if carry > 0 {
            #[allow(clippy::cast_possible_truncation)]
            self.limbs.push(carry as u32);
        }
        self.trim();
        self
    }

    /// `self × 10^exponent`.
    pub(crate) fn mul_pow10(mut self, mut exponent: u32) -> Self {
        while exponent >= 9 {
            self = self.mul_small(1_000_000_000);
            exponent -= 9;
        }
        self.mul_small(10_u32.pow(exponent))
    }

    /// `self × 5^exponent`.
    pub(crate) fn mul_pow5(mut self, mut exponent: u32) -> Self {
        // 5^13 is the largest power of 5 below 2^32.
        while exponent >= 13 {
            self = self.mul_small(1_220_703_125);
            exponent -= 13;
        }
        self.mul_small(5_u32.pow(exponent))
    }

    /// `self × 2^exponent`.
    pub(crate) fn shl(mut self, exponent: u32) -> Self {
        if self.limbs.is_empty() {
            return self;
        }
        let whole = (exponent / 32) as usize;
        let bits = exponent % 32;
        if bits > 0 {
            let mut carry = 0_u32;
            for limb in &mut self.limbs {
                let next = *limb >> (32 - bits);
                *limb = (*limb << bits) | carry;
                carry = next;
            }
            if carry > 0 {
                self.limbs.push(carry);
            }
        }
        self.limbs.splice(0..0, std::iter::repeat_n(0, whole));
        self
    }

    /// `self + other`.
    pub(crate) fn add(&self, other: &Self) -> Self {
        let (long, short) = if self.limbs.len() >= other.limbs.len() {
            (self, other)
        } else {
            (other, self)
        };
        let mut limbs = Vec::with_capacity(long.limbs.len() + 1);
        let mut carry = 0_u64;
        for (i, &limb) in long.limbs.iter().enumerate() {
            let sum = u64::from(limb) + u64::from(short.limbs.get(i).copied().unwrap_or(0)) + carry;
            #[allow(clippy::cast_possible_truncation)]
            limbs.push(sum as u32);
            carry = sum >> 32;
        }
        if carry > 0 {
            #[allow(clippy::cast_possible_truncation)]
            limbs.push(carry as u32);
        }
        Self { limbs }
    }

    /// `|self − other|`, and whether `self < other`.
    pub(crate) fn abs_diff(&self, other: &Self) -> (Self, bool) {
        let (large, small, negative) = if self >= other {
            (self, other, false)
        } else {
            (other, self, true)
        };
        let mut limbs = Vec::with_capacity(large.limbs.len());
        let mut borrow = 0_i64;
        for (i, &limb) in large.limbs.iter().enumerate() {
            let mut difference =
                i64::from(limb) - i64::from(small.limbs.get(i).copied().unwrap_or(0)) - borrow;
            borrow = i64::from(difference < 0);
            if difference < 0 {
                difference += 1 << 32;
            }
            // 0 ≤ difference < 2^32 here.
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            limbs.push(difference as u32);
        }
        let mut result = Self { limbs };
        result.trim();
        (result, negative)
    }

    /// `self × other`.
    pub(crate) fn mul(&self, other: &Self) -> Self {
        if self.limbs.is_empty() || other.limbs.is_empty() {
            return Self { limbs: Vec::new() };
        }
        let mut limbs = vec![0_u32; self.limbs.len() + other.limbs.len()];
        for (i, &a) in self.limbs.iter().enumerate() {
            let mut carry = 0_u64;
            for (j, &b) in other.limbs.iter().enumerate() {
                // At most (2^32 − 1)^2 + 2 (2^32 − 1) < 2^64.
                let product = u64::from(a) * u64::from(b) + u64::from(limbs[i + j]) + carry;
                #[allow(clippy::cast_possible_truncation)]
                {
                    limbs[i + j] = product as u32;
                }
                carry = product >> 32;
            }
            #[allow(clippy::cast_possible_truncation)]
            {
                limbs[i + other.limbs.len()] = carry as u32;
            }
        }
        let mut result = Self { limbs };
        result.trim();
        result
    }

    /// The decimal digits, most significant first, without leading zeros
    /// (none for zero).
    pub(crate) fn digits(&self) -> Vec<u8> {
        const CHUNK: u64 = 1_000_000_000;
        let mut limbs = self.limbs.clone();
        // Base-10^9 chunks, least significant first.
        let mut chunks = Vec::new();
        while !limbs.is_empty() {
            let mut remainder = 0_u64;
            for limb in limbs.iter_mut().rev() {
                let current = (remainder << 32) | u64::from(*limb);
                // current < CHUNK × 2^32, so the quotient fits in 32 bits.
                #[allow(clippy::cast_possible_truncation)]
                {
                    *limb = (current / CHUNK) as u32;
                }
                remainder = current % CHUNK;
            }
            while limbs.last() == Some(&0) {
                limbs.pop();
            }
            chunks.push(remainder);
        }
        let mut digits = Vec::with_capacity(chunks.len() * 9);
        for chunk in chunks.iter().rev() {
            let mut chunk_digits = [0_u8; 9];
            let mut rest = *chunk;
            for slot in chunk_digits.iter_mut().rev() {
                // rest % 10 < 10.
                #[allow(clippy::cast_possible_truncation)]
                {
                    *slot = (rest % 10) as u8;
                }
                rest /= 10;
            }
            digits.extend_from_slice(&chunk_digits);
        }
        let first = digits.iter().position(|&d| d != 0).unwrap_or(digits.len());
        digits.drain(..first);
        digits
    }

    fn trim(&mut self) {
        while self.limbs.last() == Some(&0) {
            self.limbs.pop();
        }
    }
}

impl PartialOrd for BigUint {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for BigUint {
    fn cmp(&self, other: &Self) -> Ordering {
        self.limbs
            .len()
            .cmp(&other.limbs.len())
            .then_with(|| self.limbs.iter().rev().cmp(other.limbs.iter().rev()))
    }
}

#[cfg(test)]
mod tests {
    use super::BigUint;

    #[test]
    fn products_compare_exactly() {
        let ten_to_40 = BigUint::from(1).mul_pow10(40);
        let ten_to_39 = BigUint::from(10).mul_pow10(38);
        assert_eq!(ten_to_40, ten_to_39.clone().mul_pow10(1));
        assert!(ten_to_39 < ten_to_40);
        assert_eq!(BigUint::from(3).shl(100), BigUint::from(3_u128 << 100));
        assert_eq!(
            BigUint::from(1).shl(64).mul_pow10(0),
            BigUint::from(1_u128 << 64)
        );
        assert!(BigUint::from(0) < BigUint::from(1));
        assert_eq!(BigUint::from(0).shl(10), BigUint::from(0));
    }

    fn digits(value: &BigUint) -> String {
        value
            .digits()
            .iter()
            .map(|d| char::from(b'0' + d))
            .collect()
    }

    #[test]
    fn exact_arithmetic_and_digits() {
        let max = BigUint::from(u128::MAX);
        assert_eq!(digits(&max), u128::MAX.to_string());
        assert_eq!(
            digits(&max.mul(&max)),
            "115792089237316195423570985008687907852589419931798687112530834793049593217025"
        );
        assert_eq!(
            digits(&max.add(&BigUint::from(1))),
            "340282366920938463463374607431768211456"
        );
        assert_eq!(
            digits(&BigUint::from(1).add(&max)),
            "340282366920938463463374607431768211456"
        );
        assert_eq!(digits(&BigUint::from(0)), "");
        assert_eq!(digits(&BigUint::from(1_000_000_000)), "1000000000");
        assert_eq!(digits(&BigUint::from(0).mul(&max)), "");
        assert_eq!(
            digits(&BigUint::from(3).mul_pow5(30)),
            (3 * 5_u128.pow(30)).to_string()
        );
        let ten_to_40 = BigUint::from(1).mul_pow10(40);
        let (difference, negative) = BigUint::from(1).abs_diff(&ten_to_40);
        assert!(negative);
        assert_eq!(digits(&difference), "9".repeat(40));
        let (difference, negative) = ten_to_40.abs_diff(&ten_to_40);
        assert!(!negative);
        assert_eq!(difference, BigUint::from(0));
    }
}
