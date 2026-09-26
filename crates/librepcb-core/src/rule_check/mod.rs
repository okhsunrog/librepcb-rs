//! Port of libs/librepcb/core/rulecheck (messages of the library element
//! checks, and later of the electrical and design rule checks).

mod rule_check_message;

pub use rule_check_message::{RuleCheckMessage, Severity, all_approvals};
