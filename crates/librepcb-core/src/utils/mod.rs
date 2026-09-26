//! Helpers from libs/librepcb/core/utils (only what the ported modules need).
//!
//! - [`toolbox`]: parts of `utils/toolbox.{h,cpp}` (fixed point decimal
//!   conversion, user input cleaning, arc and line geometry, string ranges).
//! - [`math`]: `rust-core/src/math.rs` (deterministic floating point helpers
//!   used by the C++ code through the Rust FFI).
//! - [`unicode`]: emulation of the `QChar`/`QString` character semantics the
//!   upstream validation and parsing code relies on.
//! - [`transform`]: `utils/transform.{h,cpp}` (mirror/rotate/translate).
//! - [`clipper_helpers`]: `utils/clipperhelpers.{h,cpp}` (polygon clipping
//!   with the [`clipper`] crate).
//! - [`tangent_path_joiner`]: port of `utils/tangentpathjoiner.{h,cpp}`.
//! - [`painter_path`]: the `QPainterPath` bounding rectangle of paths, which
//!   the stroke font needs.

pub mod clipper_helpers;
pub mod math;
pub mod painter_path;
pub mod tangent_path_joiner;
pub mod toolbox;
pub mod transform;
pub mod unicode;
