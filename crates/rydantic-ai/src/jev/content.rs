//! Equality for already-decoded JSON content, without changing its representation.

use super::Content;
use serde_json::{Map, Number, Value};
use std::cmp::Ordering;

pub(super) fn same(left: &Content, right: &Content) -> bool {
    match (left, right) {
        (Content::Text(left), Content::Text(right)) => left == right,
        (Content::Array(left), Content::Array(right)) => arrays_equal(left, right),
        (Content::Object(left), Content::Object(right)) => objects_equal(left, right),
        _ => false,
    }
}

fn arrays_equal(left: &[Value], right: &[Value]) -> bool {
    left.len() == right.len()
        && left
            .iter()
            .zip(right)
            .all(|(left, right)| values_equal(left, right))
}

fn objects_equal(left: &Map<String, Value>, right: &Map<String, Value>) -> bool {
    left.len() == right.len()
        && left.iter().all(|(key, left)| {
            right
                .get(key)
                .is_some_and(|right| values_equal(left, right))
        })
}

fn values_equal(left: &Value, right: &Value) -> bool {
    match (left, right) {
        (Value::Number(left), Value::Number(right)) => {
            left == right || number_parts(left) == number_parts(right)
        }
        (Value::Array(left), Value::Array(right)) => arrays_equal(left, right),
        (Value::Object(left), Value::Object(right)) => objects_equal(left, right),
        _ => left == right,
    }
}

// Serde has already validated JSON number syntax. Compare sign, significant
// decimal digits, and a signed decimal exponent. Work/storage are bounded by
// the input spelling, never by the represented magnitude or an f64 conversion.
fn number_parts(number: &Number) -> (bool, String, (bool, String)) {
    let text = number.as_str();
    let negative = text.starts_with('-');
    let text = text.trim_start_matches('-');
    let (mantissa, exponent) = text.split_once(['e', 'E']).unwrap_or((text, "0"));
    let fractional = mantissa.split_once('.').map_or(0, |(_, part)| part.len());
    let digits: String = mantissa.chars().filter(|ch| *ch != '.').collect();
    let digits = digits.trim_start_matches('0');
    let significant = digits.trim_end_matches('0');
    if significant.is_empty() {
        return (false, String::new(), (false, "0".into()));
    }
    let trailing = digits.len() - significant.len();
    let exponent = shifted_exponent(
        exponent,
        fractional > trailing,
        fractional.abs_diff(trailing),
    );
    (negative, significant.into(), exponent)
}

// Add a signed, input-length-sized offset to an arbitrary-length exponent.
// Decimal carry/borrow visits each exponent digit once; no bigint conversion
// or allocation of zeros proportional to the exponent's numeric value occurs.
fn shifted_exponent(text: &str, offset_negative: bool, offset: usize) -> (bool, String) {
    let negative = text.starts_with('-');
    let digits = text.trim_start_matches(['-', '+']).trim_start_matches('0');
    let digits = if digits.is_empty() { "0" } else { digits };
    let negative = negative && digits != "0";
    if offset == 0 {
        return (negative, digits.into());
    }
    let offset = offset.to_string();
    let (negative, digits) = if negative == offset_negative {
        (negative, add_digits(digits, &offset))
    } else {
        match digits
            .len()
            .cmp(&offset.len())
            .then_with(|| digits.cmp(&offset))
        {
            Ordering::Greater => (negative, subtract_digits(digits, &offset)),
            Ordering::Less => (offset_negative, subtract_digits(&offset, digits)),
            Ordering::Equal => (false, "0".into()),
        }
    };
    (negative && digits != "0", digits)
}

fn add_digits(left: &str, right: &str) -> String {
    let mut left = left.bytes().rev();
    let mut right = right.bytes().rev();
    let mut digits = Vec::new();
    let mut carry = 0;
    loop {
        let (left, right) = (left.next(), right.next());
        if left.is_none() && right.is_none() {
            break;
        }
        let sum = left.unwrap_or(b'0') - b'0' + right.unwrap_or(b'0') - b'0' + carry;
        digits.push(b'0' + sum % 10);
        carry = sum / 10;
    }
    if carry != 0 {
        digits.push(b'0' + carry);
    }
    digits.reverse();
    String::from_utf8(digits).expect("decimal digits")
}

// The caller has established larger >= smaller for canonical positive digits.
fn subtract_digits(larger: &str, smaller: &str) -> String {
    let mut smaller = smaller.bytes().rev();
    let mut digits = Vec::with_capacity(larger.len());
    let mut borrow = 0i16;
    for digit in larger.bytes().rev() {
        let difference =
            i16::from(digit - b'0') - i16::from(smaller.next().unwrap_or(b'0') - b'0') - borrow;
        borrow = i16::from(difference < 0);
        digits.push(b'0' + difference.rem_euclid(10) as u8);
    }
    debug_assert_eq!(borrow, 0);
    while digits.len() > 1 && digits.last() == Some(&b'0') {
        digits.pop();
    }
    digits.reverse();
    String::from_utf8(digits).expect("decimal digits")
}

#[cfg(test)]
mod tests {
    use super::shifted_exponent;

    #[test]
    fn exponent_carry_borrow_and_sign_changes_match_integer_arithmetic() {
        for exponent in -200i64..=200 {
            for offset in [
                -210i64, -201, -200, -199, -101, -100, -99, -11, -10, -9, -1, 0, 1, 9, 10, 11, 99,
                100, 101, 199, 200, 201, 210,
            ] {
                let expected = exponent + offset;
                assert_eq!(
                    shifted_exponent(
                        &format!("{exponent:+}"),
                        offset < 0,
                        offset.unsigned_abs() as usize
                    ),
                    (expected < 0, expected.unsigned_abs().to_string())
                );
            }
        }
    }
}
