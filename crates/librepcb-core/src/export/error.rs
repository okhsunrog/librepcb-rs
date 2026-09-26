//! Errors of the [`export`](super) module.
//!
//! Replaces the `RuntimeError` exceptions thrown by the upstream generators.
//! Messages are the upstream strings; only those translated upstream go
//! through [`tr!`].

use librepcb_i18n::tr;

use crate::fileio;

/// Result type of the export module.
pub type Result<T, E = Error> = std::result::Result<T, E>;

/// Error returned by the export generators.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// A curved slot was exported with the G85 slot command enabled.
    #[error(
        "{}",
        tr!(
            "ExcellonGenerator",
            "Using the G85 slot command is not possible for curved slots. Either remove curved slots or disable the G85 export option."
        )
    )]
    CurvedSlotG85,
    /// Writing the output file (or building the CSV content) failed.
    #[error(transparent)]
    FileIo(#[from] fileio::Error),
}
