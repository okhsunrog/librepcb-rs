//! Character and number semantics of `QChar`/`QString` as relied upon by the
//! upstream validation and parsing code.
//!
//! Upstream iterates `QString`s by UTF-16 code unit, so characters outside the
//! Basic Multilingual Plane are seen as two surrogate code units. Surrogates
//! are neither printable, nor whitespace, nor digits, which is reproduced here
//! by treating every non-BMP `char` accordingly.

use std::cmp::Ordering;

use unicode_general_category::{GeneralCategory, get_general_category};

/// Returns whether `c` is a character outside the Basic Multilingual Plane
/// (i.e. a surrogate pair in UTF-16).
fn is_non_bmp(c: char) -> bool {
    u32::from(c) > 0xFFFF
}

/// Equivalent of `QChar::isSpace()`.
pub fn is_space(c: char) -> bool {
    match c {
        '\t'..='\r' | ' ' | '\u{85}' | '\u{a0}' => true,
        c if u32::from(c) < 0x80 || is_non_bmp(c) => false,
        c => matches!(
            get_general_category(c),
            GeneralCategory::SpaceSeparator
                | GeneralCategory::LineSeparator
                | GeneralCategory::ParagraphSeparator
        ),
    }
}

/// Equivalent of `QChar::isPrint()` (applied per UTF-16 code unit).
pub fn is_print(c: char) -> bool {
    if is_non_bmp(c) {
        return false; // Surrogate code units are not printable.
    }
    !matches!(
        get_general_category(c),
        GeneralCategory::Control
            | GeneralCategory::Format
            | GeneralCategory::Surrogate
            | GeneralCategory::PrivateUse
            | GeneralCategory::Unassigned
    )
}

/// Equivalent of `QChar::isDigit()` + `QChar::digitValue()`.
///
/// Returns the decimal value of any BMP character of general category `Nd`.
pub fn digit_value(c: char) -> Option<u32> {
    if let Some(d) = c.to_digit(10) {
        return Some(d);
    }
    let is_nd = |cp: u32| {
        char::from_u32(cp)
            .is_some_and(|c| get_general_category(c) == GeneralCategory::DecimalNumber)
    };
    let cp = u32::from(c);
    if is_non_bmp(c) || !is_nd(cp) {
        return None;
    }
    // Unicode guarantees that decimal digits are encoded in contiguous runs
    // of ten, starting with zero.
    let mut start = cp;
    while start > 0 && is_nd(start - 1) {
        start -= 1;
    }
    Some((cp - start) % 10)
}

/// Equivalent of `QString::trimmed()`.
pub fn trimmed(s: &str) -> &str {
    s.trim_matches(is_space)
}

/// Equivalent of `QString::simplified()`: trims the string and replaces each
/// internal sequence of whitespace by a single space.
pub fn simplified(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for word in s.split(is_space).filter(|w| !w.is_empty()) {
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(word);
    }
    out
}

/// Length of a string in UTF-16 code units (equivalent of `QString::length()`).
pub fn utf16_len(s: &str) -> usize {
    s.chars().map(char::len_utf16).sum()
}

/// Truncates a string to at most `max_len` UTF-16 code units (equivalent of
/// `QString::truncate()`, but never splits a surrogate pair).
pub fn truncate_utf16(s: &mut String, max_len: usize) {
    let mut len = 0;
    for (idx, c) in s.char_indices() {
        len += c.len_utf16();
        if len > max_len {
            s.truncate(idx);
            return;
        }
    }
}

/// Compares two strings by UTF-16 code units (equivalent of `QString`'s
/// comparison operators).
pub fn cmp_utf16(a: &str, b: &str) -> Ordering {
    a.encode_utf16().cmp(b.encode_utf16())
}

/// Returns the ASCII digits of an integer string after trimming and removing
/// an optional sign, as accepted by `QString::toInt()` & friends in the "C"
/// locale. The returned flag is `true` for a negative sign.
fn split_integer(s: &str) -> Option<(bool, &str)> {
    let s = trimmed(s);
    let (negative, digits) = match s.as_bytes().first()? {
        b'-' => (true, &s[1..]),
        b'+' => (false, &s[1..]),
        _ => (false, s),
    };
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    Some((negative, digits))
}

/// Parses a signed integer like `QString::toLongLong()` (base 10).
pub fn parse_i64(s: &str) -> Option<i64> {
    let (negative, digits) = split_integer(s)?;
    let abs: u64 = digits.parse().ok()?;
    if negative {
        0i64.checked_sub_unsigned(abs)
    } else {
        i64::try_from(abs).ok()
    }
}

/// Parses a signed integer like `QString::toInt()` (base 10).
pub fn parse_i32(s: &str) -> Option<i32> {
    parse_i64(s).and_then(|v| i32::try_from(v).ok())
}

/// Parses an unsigned integer like `QString::toUInt()` (base 10).
pub fn parse_u32(s: &str) -> Option<u32> {
    match split_integer(s)? {
        (true, _) => None,
        (false, digits) => digits.parse().ok(),
    }
}

/// Parses a floating point number like `QString::toDouble()`.
pub fn parse_f64(s: &str) -> Option<f64> {
    let s = trimmed(s);
    if s.is_empty() || s.bytes().any(|b| b.is_ascii_whitespace()) {
        return None;
    }
    let lower = s.to_ascii_lowercase();
    let unsigned = lower.trim_start_matches(['+', '-']);
    let is_special = matches!(unsigned, "inf" | "nan");
    if unsigned.starts_with("inf") && !is_special {
        return None; // Qt does not accept e.g. "infinity".
    }
    let value: f64 = lower.parse().ok()?;
    if value.is_infinite() && !is_special {
        return None; // Overflow.
    }
    let mantissa = unsigned.split('e').next().unwrap_or_default();
    if value == 0.0 && mantissa.bytes().any(|b| (b'1'..=b'9').contains(&b)) {
        return None; // Underflow.
    }
    Some(value)
}

/// Parses a floating point number like `QString::toFloat()`.
pub fn parse_f32(s: &str) -> Option<f32> {
    let d = parse_f64(s)?;
    if d.is_infinite() || d.is_nan() {
        return Some(d as f32);
    }
    if d.abs() > f64::from(f32::MAX) {
        return None;
    }
    let f = d as f32;
    if d != 0.0 && f == 0.0 {
        return None; // Underflow.
    }
    Some(f)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn space() {
        assert!(is_space(' '));
        assert!(is_space('\n'));
        assert!(is_space('\u{a0}'));
        assert!(is_space('\u{3000}'));
        assert!(!is_space('a'));
        assert!(!is_space('\u{200b}'));
        assert!(is_space('\u{85}'));
        assert!(is_space('\u{1680}'));
        assert!(is_space('\u{2028}'));
        assert!(!is_space('\u{180e}'));
    }

    #[test]
    fn print() {
        assert!(is_print('a'));
        assert!(is_print(' '));
        assert!(is_print('ä'));
        assert!(!is_print('\n'));
        assert!(!is_print('\u{200b}')); // Format
        assert!(!is_print('\u{e000}')); // Private use
        assert!(!is_print('😀')); // Surrogate pair
        // Values verified against QChar::isPrint() of Qt 6.
        assert!(is_print('\u{a0}'));
        assert!(is_print('\u{2028}'));
        assert!(is_print('\u{1680}'));
        assert!(!is_print('\u{ad}'));
        assert!(!is_print('\u{85}'));
        assert!(!is_print('\u{378}'));
        assert!(!is_print('\u{180e}'));
        assert!(!is_print('\u{feff}'));
    }

    #[test]
    fn digits() {
        assert_eq!(digit_value('7'), Some(7));
        assert_eq!(digit_value('\u{0663}'), Some(3)); // Arabic-Indic three
        assert_eq!(digit_value('\u{ff19}'), Some(9)); // Fullwidth nine
        assert_eq!(digit_value('a'), None);
        assert_eq!(digit_value('\u{1d7d9}'), None); // Non-BMP
    }

    #[test]
    fn simplify() {
        assert_eq!(simplified("  a \t b\n\nc  "), "a b c");
        assert_eq!(simplified("   "), "");
    }

    #[test]
    fn truncate() {
        let mut s = String::from("ab😀c");
        truncate_utf16(&mut s, 3);
        assert_eq!(s, "ab");
        let mut s = String::from("ab😀c");
        truncate_utf16(&mut s, 4);
        assert_eq!(s, "ab😀");
    }

    #[test]
    fn integers() {
        assert_eq!(parse_i32(" +42 "), Some(42));
        assert_eq!(parse_i32("-42"), Some(-42));
        assert_eq!(parse_i32("2147483648"), None);
        assert_eq!(parse_i32("4 2"), None);
        assert_eq!(parse_i32(""), None);
        assert_eq!(parse_u32("-0"), None);
        assert_eq!(parse_u32("007"), Some(7));
        assert_eq!(parse_i64("-9223372036854775808"), Some(i64::MIN));
    }

    #[test]
    fn floats() {
        assert_eq!(parse_f64(" 1.5 "), Some(1.5));
        assert_eq!(parse_f64("1e3"), Some(1000.0));
        assert_eq!(parse_f64("1e400"), None);
        assert_eq!(parse_f64("infinity"), None);
        assert_eq!(parse_f64("inf"), Some(f64::INFINITY));
        assert_eq!(parse_f64("-INF"), Some(f64::NEG_INFINITY));
        assert_eq!(parse_f64("1e-400"), None);
        assert_eq!(parse_f64("0.000e-400"), Some(0.0));
        assert_eq!(parse_f64("1e-310"), Some(1e-310));
        assert_eq!(parse_f64("+.5"), Some(0.5));
        assert_eq!(parse_f64("1e"), None);
        assert_eq!(parse_f64("1,5"), None);
        assert_eq!(parse_f32("1e39"), None);
        assert_eq!(parse_f32("1e-50"), None);
    }
}
