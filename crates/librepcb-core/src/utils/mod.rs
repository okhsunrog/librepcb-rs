//! Helpers from libs/librepcb/core/utils (only what the ported modules need).
//!
//! - [`toolbox`]: parts of `utils/toolbox.{h,cpp}` (fixed point decimal
//!   conversion, user input cleaning, arc and line geometry, string ranges).
//! - [`math`]: `rust-core/src/math.rs` (deterministic floating point helpers
//!   used by the C++ code through the Rust FFI).
//! - [`transform`]: `utils/transform.{h,cpp}` (mirror/rotate/translate).
//! - [`clipper_helpers`]: `utils/clipperhelpers.{h,cpp}` (polygon clipping
//!   with the [`clipper`] crate).
//! - [`tangent_path_joiner`]: port of `utils/tangentpathjoiner.{h,cpp}`.
//! - [`painter_path`]: the `QPainterPath` of paths (for the canvas) and its
//!   bounding rectangle, which the stroke font needs.
//! - [`math_parser`]: evaluation of mathematical expressions in user input.
//! - [`overline_markup_parser`]: overline markup (`!RESET`) in texts.
//! - [`tag_matcher`]: selection of the best option by preferred tags.
//!
//! Not ported: `scopeguard`/`scopeguardlist` (replaced by `Drop`),
//! `signalslot` and `qtmetatyperegistration` (Qt specific), `rusthandle`
//! (FFI), `messagelogger` (later, with the workspace).

pub mod clipper_helpers;
pub mod math;
pub mod math_parser;
pub mod overline_markup_parser;
pub mod painter_path;
pub mod tag_matcher;
pub mod tangent_path_joiner;
pub mod toolbox;
pub mod transform;
