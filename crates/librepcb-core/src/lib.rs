//! LibrePCB domain model (port of libs/librepcb/core).
//!
//! Modules mirror the upstream directory layout:
//!
//! - [`types`]: basic value types (lengths, angles, points, UUIDs, versions,
//!   validated string newtypes, the layer registry, ...).
//! - [`serialization`]: the S-expression file format and the traits used to
//!   (de)serialize objects from/to it.
//! - [`fileio`]: file paths, file utilities, the transactional file system,
//!   directory locks, ZIP and CSV files.
//! - [`system_info`]: user, host and process information.
//! - [`utils`]: small helpers from `core/utils` needed by the above.

pub mod fileio;
pub mod serialization;
pub mod system_info;
pub mod types;
pub mod utils;
