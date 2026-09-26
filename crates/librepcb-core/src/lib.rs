//! LibrePCB domain model (port of libs/librepcb/core).
//!
//! Modules mirror the upstream directory layout:
//!
//! - [`types`]: basic value types (lengths, angles, points, UUIDs, versions,
//!   validated string newtypes, the layer registry, ...).
//! - [`serialization`]: the S-expression file format and the traits used to
//!   (de)serialize objects from/to it.
//! - [`geometry`]: geometric primitives (paths, polygons, pads, texts,
//!   traces, vias, ...).
//! - [`font`]: stroke fonts used to render stroke texts.
//! - [`utils`]: helpers from `core/utils` needed by the above.

pub mod font;
pub mod geometry;
pub mod serialization;
pub mod types;
pub mod utils;
