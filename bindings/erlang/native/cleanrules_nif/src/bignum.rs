//! Integers beyond 64 bits, read from the external term format
//! (`term_to_binary`), for the coefficient of an Elixir `%Decimal{}`
//! (`spec/data-model.md`, *Erlang*). The NIF API has no access to the digits of
//! a big integer, and the binding needs no big-integer crate for this one use.

use std::fmt::Write as _;

/// The largest magnitude converted to decimal digits, in bytes: 2,467 digits.
/// A coefficient this long fits in a decimal (38 digits) only when all but 38
/// of its digits are trailing zeros cancelled by the exponent, which no
/// decimal library produces; beyond it, the value reads as `decimal_overflow`.
pub(crate) const MAX_BYTES: usize = 1024;

/// What an integer's external form holds.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Integer {
    /// Its sign (`true` when negative) and its magnitude in decimal digits,
    /// without leading zeros (`"0"` for zero).
    Digits(bool, String),
    /// A magnitude longer than [`MAX_BYTES`].
    TooLong,
}

/// Read the external term format of an integer: `131` and then
/// `SMALL_INTEGER_EXT`, `INTEGER_EXT`, `SMALL_BIG_EXT` or `LARGE_BIG_EXT`.
/// `None` for anything else.
pub(crate) fn from_external(bytes: &[u8]) -> Option<Integer> {
    let rest = bytes.strip_prefix(&[131])?;
    let (&tag, rest) = rest.split_first()?;
    match tag {
        // SMALL_INTEGER_EXT: an unsigned byte.
        97 => {
            let [byte] = rest else { return None };
            Some(Integer::Digits(false, byte.to_string()))
        }
        // INTEGER_EXT: a signed 32-bit big-endian integer.
        98 => {
            let value = i32::from_be_bytes(rest.try_into().ok()?);
            Some(Integer::Digits(value < 0, value.unsigned_abs().to_string()))
        }
        // SMALL_BIG_EXT: n in one byte, the sign, n bytes little-endian.
        110 => {
            let (&n, rest) = rest.split_first()?;
            magnitude(usize::from(n), rest)
        }
        // LARGE_BIG_EXT: n in four bytes big-endian, the sign, n bytes.
        111 => {
            let n = u32::from_be_bytes(rest.get(..4)?.try_into().ok()?);
            magnitude(usize::try_from(n).ok()?, rest.get(4..)?)
        }
        _ => None,
    }
}

fn magnitude(n: usize, rest: &[u8]) -> Option<Integer> {
    let (&sign, digits) = rest.split_first()?;
    if digits.len() != n || sign > 1 {
        return None;
    }
    if n > MAX_BYTES {
        return Some(Integer::TooLong);
    }
    Some(Integer::Digits(sign == 1, decimal(digits)))
}

/// Chunks of nine decimal digits.
const BASE: u64 = 1_000_000_000;

/// The decimal digits of a little-endian magnitude.
fn decimal(little_endian: &[u8]) -> String {
    // Base 2^32 limbs, most significant first.
    let mut limbs: Vec<u32> = little_endian
        .chunks(4)
        .rev()
        .map(|chunk| {
            chunk
                .iter()
                .rev()
                .fold(0_u32, |acc, &byte| (acc << 8) | u32::from(byte))
        })
        .collect();
    // Base 10^9 chunks, least significant first, by repeated division.
    let mut chunks: Vec<u32> = Vec::new();
    loop {
        while limbs.first() == Some(&0) {
            limbs.remove(0);
        }
        if limbs.is_empty() {
            break;
        }
        let mut remainder: u64 = 0;
        for limb in &mut limbs {
            let current = (remainder << 32) | u64::from(*limb);
            // The quotient fits in 32 bits: remainder < BASE < 2^32.
            *limb = u32::try_from(current / BASE).unwrap_or(u32::MAX);
            remainder = current % BASE;
        }
        // The remainder is below BASE, which fits in 32 bits.
        chunks.push(u32::try_from(remainder).unwrap_or(0));
    }
    let Some((last, rest)) = chunks.split_last() else {
        return "0".to_owned();
    };
    let mut text = last.to_string();
    for chunk in rest.iter().rev() {
        // Writing to a `String` cannot fail.
        let _ = write!(text, "{chunk:09}");
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    fn big(negative: bool, magnitude: u128) -> Vec<u8> {
        let mut bytes = magnitude.to_le_bytes().to_vec();
        while bytes.last() == Some(&0) {
            bytes.pop();
        }
        let mut out = vec![
            131,
            110,
            u8::try_from(bytes.len()).unwrap(),
            u8::from(negative),
        ];
        out.extend(bytes);
        out
    }

    #[test]
    fn small_and_32_bit_integers() {
        assert_eq!(
            from_external(&[131, 97, 7]),
            Some(Integer::Digits(false, "7".into()))
        );
        let minus = (-1_234_567_i32).to_be_bytes();
        let mut bytes = vec![131, 98];
        bytes.extend(minus);
        assert_eq!(
            from_external(&bytes),
            Some(Integer::Digits(true, "1234567".into()))
        );
        let mut min = vec![131, 98];
        min.extend(i32::MIN.to_be_bytes());
        assert_eq!(
            from_external(&min),
            Some(Integer::Digits(true, "2147483648".into()))
        );
    }

    #[test]
    fn big_integers_in_decimal() {
        for value in [
            0_u128,
            1,
            999_999_999,
            1_000_000_000,
            u128::from(u64::MAX) + 1,
            10_u128.pow(38) - 1,
            10_u128.pow(38),
            u128::MAX,
        ] {
            assert_eq!(
                from_external(&big(false, value)),
                Some(Integer::Digits(false, value.to_string())),
                "{value}"
            );
        }
        assert_eq!(
            from_external(&big(true, 10_u128.pow(30))),
            Some(Integer::Digits(true, 10_u128.pow(30).to_string()))
        );
    }

    #[test]
    fn large_big_ext_and_its_limit() {
        // 2^(8 * 20): 1 followed by 20 zero bytes, little-endian.
        let mut magnitude = vec![0_u8; 20];
        magnitude.push(1);
        let mut bytes = vec![131, 111];
        bytes.extend(u32::try_from(magnitude.len()).unwrap().to_be_bytes());
        bytes.push(0);
        bytes.extend(&magnitude);
        assert_eq!(
            from_external(&bytes),
            Some(Integer::Digits(
                false,
                "1461501637330902918203684832716283019655932542976".into()
            ))
        );
        let mut long = vec![131, 111];
        long.extend(u32::try_from(MAX_BYTES + 1).unwrap().to_be_bytes());
        long.push(0);
        long.extend(vec![1_u8; MAX_BYTES + 1]);
        assert_eq!(from_external(&long), Some(Integer::TooLong));
    }

    #[test]
    fn anything_else_is_none() {
        assert_eq!(from_external(&[]), None);
        assert_eq!(from_external(&[131]), None);
        assert_eq!(from_external(&[130, 97, 1]), None);
        assert_eq!(from_external(&[131, 100, 0]), None);
        assert_eq!(from_external(&[131, 97]), None);
        assert_eq!(from_external(&[131, 98, 0, 0]), None);
        assert_eq!(from_external(&[131, 110, 2, 0, 1]), None);
        assert_eq!(from_external(&[131, 110, 1, 2, 1]), None);
        assert_eq!(from_external(&[131, 110]), None);
        assert_eq!(from_external(&[131, 111, 0, 0]), None);
    }
}
