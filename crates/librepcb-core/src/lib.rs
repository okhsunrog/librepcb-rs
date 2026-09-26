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
//! - [`attribute`]: attributes of library elements and projects, and the
//!   `{{KEY}}` substitution in texts.
//! - [`algorithm`]: air wire calculation and net segment simplification.
//! - [`sqlite_database`]: thin wrapper around SQLite (for the library index).
//! - [`utils`]: helpers from `core/utils` needed by the above.

pub mod algorithm;
pub mod attribute;
pub mod font;
pub mod geometry;
pub mod serialization;
pub mod sqlite_database;
pub mod types;
pub mod utils;
