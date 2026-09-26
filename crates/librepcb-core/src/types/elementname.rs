//! Port of libs/librepcb/core/types/elementname.h.

use super::string_newtype::{optional_string_newtype_serialization, string_newtype};
use crate::utils::unicode::{is_print, trimmed, truncate_utf16, utf16_len};

/// Maximum length in UTF-16 code units.
const MAX_LENGTH: usize = 70;

string_newtype!(
    /// A valid element name (used as name of many objects).
    ///
    /// Valid names contain 1..70 (UTF-16) characters, only printable
    /// characters, and no leading or trailing whitespace. Like upstream,
    /// characters outside the Basic Multilingual Plane are not printable.
    ElementName,
    |value| {
        !value.is_empty()
            && utf16_len(value) <= MAX_LENGTH
            && trimmed(value) == value
            && value.chars().all(is_print)
    },
    crate::types::Error::InvalidElementName
);

optional_string_newtype_serialization!(ElementName);

impl ElementName {
    /// Cleans a user input string to make it a valid element name (if not
    /// empty afterwards): trims it, removes non-printable characters and
    /// truncates it (upstream `cleanElementName()`).
    pub fn clean(user_input: &str) -> String {
        let mut ret: String = trimmed(user_input)
            .chars()
            .filter(|&c| is_print(c))
            .collect();
        truncate_utf16(&mut ret, MAX_LENGTH);
        ret
    }

    /// Creates a name from a translatable string: the translation is used if
    /// it (cleaned) is a valid name, otherwise the untranslated
    /// `text_no_tr`, which must be valid (upstream `elementNameFromTr()`).
    pub fn from_tr(context: &str, text_no_tr: &'static str) -> Self {
        let translated = Self::clean(&librepcb_i18n::translate(context, text_no_tr));
        Self::new(translated).unwrap_or_else(|_| {
            debug_assert!(Self::is_valid(text_no_tr), "invalid name: {text_no_tr}");
            Self(text_no_tr.to_owned())
        })
    }
}
