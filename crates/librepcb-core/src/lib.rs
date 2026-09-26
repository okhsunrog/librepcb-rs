//! LibrePCB domain model (port of libs/librepcb/core).
//!
//! Modules mirror the upstream directory layout:
//!
//! - [`types`]: basic value types (lengths, angles, points, UUIDs, versions,
//!   validated string newtypes, the layer registry, ...).
//! - [`serialization`]: the S-expression file format and the traits used to
//!   (de)serialize objects from/to it.
//! - [`utils`]: small helpers from `core/utils` needed by the above.

pub mod serialization;
pub mod types;
pub mod utils;
