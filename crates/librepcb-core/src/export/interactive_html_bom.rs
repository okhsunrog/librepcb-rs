//! Port of the option enums of libs/librepcb/core/export/interactivehtmlbom.{h,cpp}.
//!
//! Only `InteractiveHtmlBom::ViewMode` and `HighlightPin1Mode` with their
//! (de)serialization are ported (needed by the interactive BOM output job);
//! the generator itself is not ported yet.

use std::fmt;
use std::str::FromStr;

use crate::serialization::{self, FromSExpression, SExpression, ToSExpression};
use crate::types;

/// Layout of the interactive HTML BOM (upstream
/// `InteractiveHtmlBom::ViewMode`).
///
/// Serde: the file format token (`"bom_only"`, `"left_right"`,
/// `"top_bottom"`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum InteractiveHtmlBomViewMode {
    /// Only the BOM table.
    BomOnly,
    /// BOM table left, board views right.
    #[default]
    LeftRight,
    /// BOM table on top, board views below.
    TopBottom,
}

impl InteractiveHtmlBomViewMode {
    /// Returns the file format token.
    pub fn to_str(self) -> &'static str {
        match self {
            Self::BomOnly => "bom_only",
            Self::LeftRight => "left_right",
            Self::TopBottom => "top_bottom",
        }
    }
}

impl fmt::Display for InteractiveHtmlBomViewMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.to_str())
    }
}

impl FromStr for InteractiveHtmlBomViewMode {
    type Err = types::Error;
    fn from_str(s: &str) -> Result<Self, types::Error> {
        [Self::BomOnly, Self::LeftRight, Self::TopBottom]
            .into_iter()
            .find(|v| v.to_str() == s)
            .ok_or_else(|| types::Error::UnknownInteractiveHtmlBomViewMode(s.to_owned()))
    }
}

crate::utils::serde_string::serde_string!(InteractiveHtmlBomViewMode);

impl ToSExpression for InteractiveHtmlBomViewMode {
    fn to_sexpression(&self) -> SExpression {
        SExpression::token(self.to_str())
    }
}

impl FromSExpression for InteractiveHtmlBomViewMode {
    fn from_sexpression(node: &SExpression) -> serialization::Result<Self> {
        Ok(node.value()?.parse()?)
    }
}

/// Which pin 1 pads are highlighted in the interactive HTML BOM (upstream
/// `InteractiveHtmlBom::HighlightPin1Mode`).
///
/// Serde: the file format token (`"none"`, `"selected"`, `"all"`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum InteractiveHtmlBomHighlightPin1Mode {
    /// No highlighting.
    #[default]
    None,
    /// Pin 1 of selected components.
    Selected,
    /// Pin 1 of all components.
    All,
}

impl InteractiveHtmlBomHighlightPin1Mode {
    /// Returns the file format token.
    pub fn to_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Selected => "selected",
            Self::All => "all",
        }
    }
}

impl fmt::Display for InteractiveHtmlBomHighlightPin1Mode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.to_str())
    }
}

impl FromStr for InteractiveHtmlBomHighlightPin1Mode {
    type Err = types::Error;
    fn from_str(s: &str) -> Result<Self, types::Error> {
        [Self::None, Self::Selected, Self::All]
            .into_iter()
            .find(|v| v.to_str() == s)
            .ok_or_else(|| types::Error::UnknownInteractiveHtmlBomHighlightPin1Mode(s.to_owned()))
    }
}

crate::utils::serde_string::serde_string!(InteractiveHtmlBomHighlightPin1Mode);

impl ToSExpression for InteractiveHtmlBomHighlightPin1Mode {
    fn to_sexpression(&self) -> SExpression {
        SExpression::token(self.to_str())
    }
}

impl FromSExpression for InteractiveHtmlBomHighlightPin1Mode {
    fn from_sexpression(node: &SExpression) -> serialization::Result<Self> {
        Ok(node.value()?.parse()?)
    }
}
