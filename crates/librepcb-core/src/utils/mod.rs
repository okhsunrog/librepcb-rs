//! Helpers from libs/librepcb/core/utils (only what the ported modules need).
//!
//! - [`toolbox`]: parts of `utils/toolbox.{h,cpp}` (fixed point decimal
//!   conversion, user input cleaning).
//! - [`math`]: parts of `rust-core/src/math.rs` (deterministic floating point
//!   helpers used by the C++ code through the Rust FFI).
//! - [`unicode`]: emulation of the `QChar`/`QString` character semantics the
//!   upstream validation and parsing code relies on.
//! - [`math_parser`]: evaluation of mathematical expressions in user input.
//! - [`overline_markup_parser`]: overline markup (`!RESET`) in texts.
//! - [`tag_matcher`]: selection of the best option by preferred tags.
//!
//! Not ported: `scopeguard`/`scopeguardlist` (replaced by `Drop`),
//! `signalslot` and `qtmetatyperegistration` (Qt specific), `rusthandle`
//! (FFI), `messagelogger` (later, with the workspace).

pub mod math;
pub mod math_parser;
pub mod overline_markup_parser;
pub mod tag_matcher;
pub mod toolbox;
pub mod unicode;
