//! Port of libs/librepcb/core/types (basic value types).
//!
//! All types are small, validated value types: constrained upstream types
//! (`type_safe::constrained_type`) are newtypes with fallible constructors
//! (`new()`, [`TryFrom`], [`FromStr`](std::str::FromStr)), and upstream
//! singleton classes (layers, colors, roles) are `Copy` enums/handles.
//!
//! Additionally, [`Color`] replaces `QColor` where the file format needs it.

mod alignment;
mod angle;
mod boundedunsignedratio;
mod busname;
mod circuitidentifier;
mod color;
mod elementname;
mod enums;
mod error;
mod fileproofname;
mod layer;
mod length;
mod lengthunit;
mod maskconfig;
mod pcbcolor;
mod point;
mod ratio;
mod signalrole;
mod simplestring;
pub(crate) mod string_newtype;
mod stroketextspacing;
mod tag;
mod uuid;
mod version;

pub use alignment::{Alignment, HAlign, VAlign};
pub use angle::{Angle, Angle3D};
pub use boundedunsignedratio::BoundedUnsignedRatio;
pub use busname::BusName;
pub use circuitidentifier::CircuitIdentifier;
pub use color::Color;
pub use elementname::ElementName;
pub use enums::{AutoUpdateMode, GridStyle};
pub use error::Error;
pub use fileproofname::FileProofName;
pub use layer::Layer;
pub use length::{Length, Point3D, PositiveLength, UnsignedLength};
pub use lengthunit::LengthUnit;
pub use maskconfig::MaskConfig;
pub use pcbcolor::PcbColor;
pub use point::{Orientation, Point};
pub use ratio::{Ratio, UnsignedLimitedRatio, UnsignedRatio};
pub use signalrole::SignalRole;
pub use simplestring::SimpleString;
pub use stroketextspacing::StrokeTextSpacing;
pub use tag::Tag;
pub use uuid::Uuid;
pub use version::Version;
