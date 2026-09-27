//! Design rule check of boards for `open-project --drc` (upstream
//! `BoardDesignRuleCheck`, called from `CommandLineInterface::openProject()`).
//!
//! TODO(merge-drc): the DRC is ported concurrently
//! (`librepcb_core::project::board::drc`); wire [`run_drc()`] to it once it
//! is merged. Until then, `--drc` fails with an error.

use librepcb_core::library::org::BoardDesignRuleCheckSettings;
use librepcb_core::project::{BoardId, Project};
use librepcb_core::rule_check::RuleCheckMessage;

/// Result of the DRC of one board (upstream
/// `BoardDesignRuleCheck::Result`).
#[derive(Debug, Default)]
pub struct DrcResult {
    /// Fatal errors (the check could not be completed).
    pub errors: Vec<String>,
    /// The check messages.
    pub messages: Vec<RuleCheckMessage>,
}

/// Error of [`run_drc()`].
#[derive(Debug, thiserror::Error)]
pub enum DrcError {
    /// The DRC is not available in this build yet.
    #[error("The design rule check is not supported yet by this LibrePCB version (librepcb-rs).")]
    NotAvailable,
}

/// Runs the DRC of a board with the given settings (upstream
/// `BoardDesignRuleCheck::start()` + `waitForFinished()`, without the
/// quick check).
///
/// TODO(merge-drc): call the ported DRC here.
pub fn run_drc(
    _project: &mut Project,
    _board: BoardId,
    _settings: &BoardDesignRuleCheckSettings,
) -> Result<DrcResult, DrcError> {
    Err(DrcError::NotAvailable)
}
