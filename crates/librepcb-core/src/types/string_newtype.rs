//! Shared implementation of the validated string newtypes (upstream
//! `type_safe::constrained_type<QString, ...>` aliases).

/// Defines a validated string newtype.
///
/// `$valid` is a `fn(&str) -> bool` constraint and `$err` the error variant
/// (taking the rejected string). The type serializes as S-expression string.
macro_rules! string_newtype {
    ($(#[$meta:meta])* $name:ident, $valid:expr, $err:path) => {
        $(#[$meta])*
        #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(String);

        impl $name {
            /// Creates the value, or returns an error if `value` is invalid.
            pub fn new(value: impl Into<String>) -> Result<Self, $crate::types::Error> {
                let value = value.into();
                if Self::is_valid(&value) {
                    Ok(Self(value))
                } else {
                    Err($err(value))
                }
            }

            /// Returns whether `value` is valid.
            pub fn is_valid(value: &str) -> bool {
                let valid: fn(&str) -> bool = $valid;
                valid(value)
            }

            /// Returns the string.
            pub fn as_str(&self) -> &str {
                &self.0
            }

            /// Returns the owned string.
            pub fn into_string(self) -> String {
                self.0
            }
        }

        impl std::ops::Deref for $name {
            type Target = str;
            fn deref(&self) -> &str {
                &self.0
            }
        }

        impl AsRef<str> for $name {
            fn as_ref(&self) -> &str {
                &self.0
            }
        }

        impl std::borrow::Borrow<str> for $name {
            fn borrow(&self) -> &str {
                &self.0
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(&self.0)
            }
        }

        impl std::str::FromStr for $name {
            type Err = $crate::types::Error;
            fn from_str(s: &str) -> Result<Self, Self::Err> {
                Self::new(s)
            }
        }

        impl TryFrom<String> for $name {
            type Error = $crate::types::Error;
            fn try_from(value: String) -> Result<Self, Self::Error> {
                Self::new(value)
            }
        }

        impl TryFrom<&str> for $name {
            type Error = $crate::types::Error;
            fn try_from(value: &str) -> Result<Self, Self::Error> {
                Self::new(value)
            }
        }

        impl From<$name> for String {
            fn from(value: $name) -> String {
                value.0
            }
        }

        impl PartialEq<str> for $name {
            fn eq(&self, other: &str) -> bool {
                self.0 == other
            }
        }

        impl PartialEq<&str> for $name {
            fn eq(&self, other: &&str) -> bool {
                self.0 == *other
            }
        }

        impl PartialEq<String> for $name {
            fn eq(&self, other: &String) -> bool {
                &self.0 == other
            }
        }

        impl $crate::serialization::ToSExpression for $name {
            fn to_sexpression(&self) -> $crate::serialization::SExpression {
                $crate::serialization::SExpression::string(self.0.as_str())
            }
        }

        impl $crate::serialization::FromSExpression for $name {
            fn from_sexpression(
                node: &$crate::serialization::SExpression,
            ) -> $crate::serialization::Result<Self> {
                Ok(Self::new(node.value()?)?)
            }
        }
    };
}

/// Implements (de)serialization of `Option<$name>` as (possibly empty)
/// string, where the empty string represents `None`.
macro_rules! optional_string_newtype_serialization {
    ($name:ident) => {
        impl $crate::serialization::ToSExpression for Option<$name> {
            fn to_sexpression(&self) -> $crate::serialization::SExpression {
                $crate::serialization::SExpression::string(
                    self.as_ref().map(|v| v.as_str()).unwrap_or_default(),
                )
            }
        }

        impl $crate::serialization::FromSExpression for Option<$name> {
            fn from_sexpression(
                node: &$crate::serialization::SExpression,
            ) -> $crate::serialization::Result<Self> {
                match node.value()? {
                    "" => Ok(None),
                    value => Ok(Some($name::new(value)?)),
                }
            }
        }
    };
}

pub(crate) use optional_string_newtype_serialization;
pub(crate) use string_newtype;

/// Returns whether `value` consists of 1..=`max_len` characters which all
/// satisfy `allowed` (equivalent of the upstream regexes like
/// `\A[-a-z0-9.]{1,32}\z` with ASCII character classes).
pub(crate) fn is_restricted(value: &str, max_len: usize, allowed: impl Fn(char) -> bool) -> bool {
    !value.is_empty() && value.chars().count() <= max_len && value.chars().all(allowed)
}
