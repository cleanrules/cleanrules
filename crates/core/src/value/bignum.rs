//! Just enough unsigned big-integer arithmetic to compare numbers exactly.
//!
//! Comparing a float with a decimal, or two decimals of different scales, means
//! comparing products such as `m × 2^e × 10^s` that overflow 128 bits. Only
//! multiplication by small factors, shifts and comparison are needed.

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
}
