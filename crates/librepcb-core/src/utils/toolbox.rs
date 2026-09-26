//! Port of parts of libs/librepcb/core/utils/toolbox.{h,cpp}:
//! `decimalFixedPointToString()`, `decimalFixedPointFromString()` and
//! `cleanUserInputString()`.

use unicode_normalization::UnicodeNormalization;

use super::unicode::{digit_value, trimmed, truncate_utf16};
use crate::types::Error;

/// Converts a fixed point decimal number to a string, e.g. `1234567` with
/// `point_pos = 6` to `"1.234567"`.
///
/// Trailing zeros are removed, but at least one decimal is always kept (e.g.
/// `"1.0"`). Zero is always formatted as `"0.0"`.
pub fn decimal_fixed_point_to_string(value: i64, point_pos: usize) -> String {
    if value == 0 {
        return "0.0".to_owned();
    }
    let mut s = value.unsigned_abs().to_string();
    if s.len() > point_pos {
        s.insert(s.len() - point_pos, '.');
    } else {
        s.insert_str(0, &"0".repeat(point_pos - s.len()));
        s.insert_str(0, "0.");
    }
    while s.ends_with('0') && !s.ends_with(".0") {
        s.pop();
    }
    if value < 0 {
        s.insert(0, '-');
    }
    s
}

/// Target integer type of [`decimal_fixed_point_from_string()`].
///
/// Upstream is a template over the integer type, and the overflow checks are
/// performed in the corresponding *unsigned* type, which affects which inputs
/// are accepted. This trait provides the required limits.
pub trait FixedPointInt: Copy {
    /// `std::numeric_limits<std::make_unsigned<T>>::max()`.
    const MAX_UNSIGNED: u128;
    /// Absolute value of `std::numeric_limits<T>::min()`.
    const MIN_ABS: u128;
    /// `std::numeric_limits<T>::max()`.
    const MAX: u128;
    /// Converts the (range-checked) signed value to `Self`.
    fn from_i128(value: i128) -> Self;
}

impl FixedPointInt for i64 {
    const MAX_UNSIGNED: u128 = u64::MAX as u128;
    const MIN_ABS: u128 = i64::MIN.unsigned_abs() as u128;
    const MAX: u128 = i64::MAX as u128;
    fn from_i128(value: i128) -> Self {
        // Range is checked by the caller against MIN_ABS/MAX.
        value as i64
    }
}

impl FixedPointInt for i32 {
    const MAX_UNSIGNED: u128 = u32::MAX as u128;
    const MIN_ABS: u128 = i32::MIN.unsigned_abs() as u128;
    const MAX: u128 = i32::MAX as u128;
    fn from_i128(value: i128) -> Self {
        // Range is checked by the caller against MIN_ABS/MAX.
        value as i32
    }
}

/// Converts a fixed point decimal number string to an integer, e.g. `"1.5"`
/// with `point_pos = 6` to `1500000`.
///
/// Accepts an optional sign, an integer part, a fractional part and an
/// exponent (e.g. `"-1.5e-3"`). Fails if the string is malformed, has more
/// decimals than `point_pos`, or the value is out of range of `T`.
pub fn decimal_fixed_point_from_string<T: FixedPointInt>(
    s: &str,
    point_pos: u32,
) -> Result<T, Error> {
    parse_fixed_point::<T>(s, point_pos).ok_or_else(|| Error::InvalidFixedPointNumber(s.to_owned()))
}

fn parse_fixed_point<T: FixedPointInt>(s: &str, point_pos: u32) -> Option<T> {
    #[derive(PartialEq, Eq, Clone, Copy)]
    enum State {
        Invalid,
        Start,
        AfterSign,
        LonelyDot,
        IntPart,
        FracPart,
        Exp,
        ExpAfterSign,
        ExpDigits,
    }

    let max_u = T::MAX_UNSIGNED;
    let mut state = State::Start;
    let mut value_abs: u128 = 0;
    let mut sign = false;
    let mut exp_offset: i64 = i64::from(point_pos);
    let max_exp = u32::MAX;
    let mut exp: u32 = 0;
    let mut exp_sign = false;

    // Appends a digit to `value`, returning `None` on overflow of `max`.
    fn push_digit<V>(value: V, digit: V, max: V) -> Option<V>
    where
        V: Copy
            + PartialOrd
            + std::ops::Div<Output = V>
            + std::ops::Mul<Output = V>
            + std::ops::Add<Output = V>
            + std::ops::Sub<Output = V>
            + From<u8>,
    {
        if value > max / V::from(10) {
            return None;
        }
        let value = value * V::from(10);
        if value > max - digit {
            return None;
        }
        Some(value + digit)
    }

    for c in s.chars() {
        let digit = digit_value(c);
        state = match state {
            State::Invalid => break,
            State::Start => match (c, digit) {
                ('-', _) => {
                    sign = true;
                    State::AfterSign
                }
                ('+', _) => State::AfterSign,
                ('.', _) => State::LonelyDot,
                (_, Some(d)) => {
                    value_abs = u128::from(d);
                    State::IntPart
                }
                _ => State::Invalid,
            },
            State::AfterSign => match (c, digit) {
                ('.', _) => State::LonelyDot,
                (_, Some(d)) => {
                    value_abs = u128::from(d);
                    State::IntPart
                }
                _ => State::Invalid,
            },
            State::LonelyDot => match digit {
                Some(d) => {
                    value_abs = u128::from(d);
                    exp_offset -= 1;
                    State::FracPart
                }
                None => State::Invalid,
            },
            State::IntPart => match (c, digit) {
                ('.', _) => State::FracPart,
                ('e' | 'E', _) => State::Exp,
                (_, Some(d)) => match push_digit(value_abs, u128::from(d), max_u) {
                    Some(v) => {
                        value_abs = v;
                        State::IntPart
                    }
                    None => State::Invalid,
                },
                _ => State::Invalid,
            },
            State::FracPart => match (c, digit) {
                ('e' | 'E', _) => State::Exp,
                (_, Some(d)) => match push_digit(value_abs, u128::from(d), max_u) {
                    Some(v) => {
                        value_abs = v;
                        exp_offset -= 1;
                        State::FracPart
                    }
                    None => State::Invalid,
                },
                _ => State::Invalid,
            },
            State::Exp => match (c, digit) {
                ('-', _) => {
                    exp_sign = true;
                    State::ExpAfterSign
                }
                ('+', _) => State::ExpAfterSign,
                (_, Some(d)) => {
                    exp = d;
                    State::ExpDigits
                }
                _ => State::Invalid,
            },
            State::ExpAfterSign => match digit {
                Some(d) => {
                    exp = d;
                    State::ExpDigits
                }
                None => State::Invalid,
            },
            State::ExpDigits => match digit.and_then(|d| push_digit(exp, d, max_exp)) {
                Some(e) => {
                    exp = e;
                    State::ExpDigits
                }
                None => State::Invalid,
            },
        };
    }

    if !matches!(state, State::IntPart | State::FracPart | State::ExpDigits) {
        return None;
    }

    // Merge the exponent offset (from the point position and the number of
    // fractional digits) into the exponent.
    let exp_offset_abs = u32::try_from(exp_offset.unsigned_abs()).ok()?;
    if exp_sign == (exp_offset < 0) {
        exp = exp.checked_add(exp_offset_abs)?;
    } else {
        if exp < exp_offset_abs {
            return None;
        }
        exp -= exp_offset_abs;
    }

    if value_abs == 0 {
        // No need to apply exponent or sign if the value is zero.
        return Some(T::from_i128(0));
    }
    for _ in 0..exp {
        if exp_sign {
            if !value_abs.is_multiple_of(10) {
                return None; // More decimal digits than allowed.
            }
            value_abs /= 10;
        } else {
            if value_abs > max_u / 10 {
                return None; // Would overflow.
            }
            value_abs *= 10;
        }
    }
    if sign {
        if value_abs > T::MIN_ABS {
            return None;
        }
        Some(T::from_i128(-(value_abs as i128)))
    } else {
        if value_abs > T::MAX {
            return None;
        }
        Some(T::from_i128(value_abs as i128))
    }
}

/// Cleans a user input string (equivalent of `Toolbox::cleanUserInputString()`).
///
/// Performs NFKD normalization, optional case conversion and trimming,
/// replaces spaces by `space_replacement`, removes all characters for which
/// `is_allowed` returns `false`, and truncates the result to `max_length`
/// UTF-16 code units.
pub fn clean_user_input_string(
    input: &str,
    is_allowed: impl Fn(char) -> bool,
    trim: bool,
    to_lower: bool,
    to_upper: bool,
    space_replacement: &str,
    max_length: Option<usize>,
) -> String {
    let mut ret: String = input.nfkd().collect();
    if to_lower {
        ret = ret.to_lowercase();
    }
    if to_upper {
        ret = ret.to_uppercase();
    }
    if trim {
        ret = trimmed(&ret).to_owned();
    }
    ret = ret.replace(' ', space_replacement);
    ret.retain(is_allowed);
    if let Some(max) = max_length {
        truncate_utf16(&mut ret, max);
    }
    if trim {
        ret = trimmed(&ret).to_owned();
    }
    ret
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn to_string() {
        assert_eq!(decimal_fixed_point_to_string(0, 6), "0.0");
        assert_eq!(decimal_fixed_point_to_string(1, 6), "0.000001");
        assert_eq!(decimal_fixed_point_to_string(-1_000_000, 6), "-1.0");
        assert_eq!(
            decimal_fixed_point_to_string(i64::MIN, 6),
            "-9223372036854.775808"
        );
    }

    #[test]
    fn from_string_width_matters() {
        // Overflow checks are done in the unsigned type of the target width.
        assert!(decimal_fixed_point_from_string::<i32>("1000000000000e-12", 6).is_err());
        assert_eq!(
            decimal_fixed_point_from_string::<i64>("1000000000000e-12", 6).unwrap(),
            1_000_000
        );
        assert_eq!(
            decimal_fixed_point_from_string::<i32>("1000000000e-9", 6).unwrap(),
            1_000_000
        );
    }

    #[test]
    fn from_string_exponent_quirk() {
        // Upstream rejects a negative exponent smaller than the number of
        // fixed point decimals (the exponent offset merge fails).
        assert!(decimal_fixed_point_from_string::<i64>("1e-3", 6).is_err());
        assert_eq!(
            decimal_fixed_point_from_string::<i64>("1e-6", 6).unwrap(),
            1
        );
        assert_eq!(
            decimal_fixed_point_from_string::<i64>("1e3", 6).unwrap(),
            1_000_000_000
        );
    }

    #[test]
    fn from_string_unicode_digits() {
        assert_eq!(
            decimal_fixed_point_from_string::<i64>("\u{0661}.\u{0665}", 6).unwrap(),
            1_500_000
        );
    }

    #[test]
    fn clean() {
        let s = clean_user_input_string(
            " Foo Bär ",
            |c| c.is_ascii_lowercase() || c == '-',
            true,
            true,
            false,
            "-",
            Some(32),
        );
        assert_eq!(s, "foo-bar");
    }
}
