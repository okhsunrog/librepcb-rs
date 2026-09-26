//! Helpers from libs/librepcb/core/utils (only what the ported modules need).
//!
//! - [`toolbox`]: parts of `utils/toolbox.{h,cpp}` (fixed point decimal
//!   conversion, user input cleaning).
//! - [`math`]: parts of `rust-core/src/math.rs` (deterministic floating point
//!   helpers used by the C++ code through the Rust FFI).
//! - [`unicode`]: emulation of the `QChar`/`QString` character semantics the
//!   upstream validation and parsing code relies on.

pub mod math;
pub mod toolbox;
pub mod unicode;
