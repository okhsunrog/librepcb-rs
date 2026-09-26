//! Port of libs/librepcb/core/types/simplestring.h.

use super::string_newtype::string_newtype;
use crate::utils::unicode::{is_print, simplified, trimmed};

string_newtype!(
    /// A string consisting of printable characters only, without leading or
    /// trailing whitespace. The empty string is valid (and the default).
    #[derive(Default)]
    SimpleString,
    |value| trimmed(value) == value && value.chars().all(is_print),
    crate::types::Error::InvalidSimpleString
);

impl SimpleString {
    /// Cleans a user input string: collapses whitespace and removes
    /// non-printable characters (upstream `cleanSimpleString()`).
    pub fn clean(user_input: &str) -> Self {
        let cleaned: String = simplified(user_input)
            .chars()
            .filter(|&c| is_print(c))
            .collect();
        // Upstream would abort if removing a non-printable character exposed
        // leading/trailing whitespace; trim instead to keep the invariant.
        Self(trimmed(&cleaned).to_owned())
    }
}
