//! Application startup: choosing and opening the workspace, and the
//! language.
//!
//! Port of `openWorkspace()` of apps/librepcb/main.cpp and of
//! `Workspace::getMostRecentlyUsedWorkspacePath()`
//! (libs/librepcb/core/workspace/workspace.cpp). The workspace is taken
//! from, in this order:
//!
//! 1. the `--workspace` command line option,
//! 2. the environment variable `LIBREPCB_WORKSPACE` (like upstream),
//! 3. upstream's client settings (`workspaces/most_recently_used` in
//!    `~/.config/LibrePCB/LibrePCB.conf` on Linux), so both applications
//!    use the same workspace,
//! 4. the default `~/LibrePCB-Workspace` (upstream's suggestion in the
//!    workspace wizard), created if it does not exist.
//!
//! Upstream shows a wizard for the last case; here the workspace is created
//! without asking (the wizard is not ported yet).

use std::path::{Path, PathBuf};

use librepcb_core::fileio::{FilePath, LockStatus};
use librepcb_core::workspace::Workspace;

/// Errors of the startup.
#[derive(Debug, thiserror::Error)]
pub enum StartupError {
    /// The workspace path is invalid.
    #[error("Invalid workspace path: {0}")]
    InvalidPath(String),
    /// Opening the workspace failed.
    #[error(transparent)]
    Workspace(#[from] librepcb_core::workspace::Error),
}

/// The home directory.
fn home_dir() -> Option<PathBuf> {
    std::env::home_dir().filter(|p| p.is_absolute())
}

/// Upstream's client settings file (Qt `QSettings` INI format, Linux).
fn client_settings_file() -> Option<PathBuf> {
    let config = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| home_dir().map(|h| h.join(".config")))?;
    Some(config.join("LibrePCB").join("LibrePCB.conf"))
}

/// Reads `workspaces/most_recently_used` from a Qt INI settings file.
pub fn most_recently_used_workspace(ini: &str) -> Option<PathBuf> {
    let mut in_section = false;
    for line in ini.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_section = line == "[workspaces]";
        } else if in_section && let Some(value) = line.strip_prefix("most_recently_used=") {
            let value = value.trim().trim_matches('"');
            if !value.is_empty() {
                return Some(PathBuf::from(value));
            }
        }
    }
    None
}

/// Determines the workspace path (see the module documentation).
pub fn workspace_path(cli: Option<&Path>) -> Option<PathBuf> {
    if let Some(p) = cli {
        return Some(p.to_path_buf());
    }
    if let Some(p) = std::env::var_os("LIBREPCB_WORKSPACE").filter(|p| !p.is_empty()) {
        log::info!("Workspace path overridden by LIBREPCB_WORKSPACE.");
        return Some(PathBuf::from(p));
    }
    if let Some(p) = client_settings_file()
        .and_then(|f| std::fs::read_to_string(f).ok())
        .and_then(|ini| most_recently_used_workspace(&ini))
    {
        return Some(p);
    }
    home_dir().map(|h| h.join("LibrePCB-Workspace"))
}

/// Opens (or creates) the workspace at `path`. Stale locks are overridden,
/// locks of running applications refused.
pub fn open_workspace(path: &Path) -> Result<Workspace, StartupError> {
    let abs = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|_| StartupError::InvalidPath(path.display().to_string()))?
            .join(path)
    };
    let fp =
        FilePath::new(&abs).ok_or_else(|| StartupError::InvalidPath(abs.display().to_string()))?;
    let mut handler =
        |_: &FilePath, status: LockStatus, _: &str| Ok(status == LockStatus::StaleLock);
    Ok(Workspace::open_or_create(&fp, Some(&mut handler))?)
}

/// Selects the language for `librepcb_i18n` (and returns the tag to pass
/// to `slint::select_bundled_translation()` once the window exists):
/// the command line option, else the workspace setting, else the system
/// locale.
pub fn select_language(cli: Option<&str>, workspace: &Workspace) -> &'static str {
    let setting = workspace.settings().application_locale.get();
    let requested = cli.filter(|l| !l.is_empty()).or(if setting.is_empty() {
        None
    } else {
        Some(setting.as_str())
    });
    match requested {
        Some(lang) => librepcb_i18n::set_language(lang).unwrap_or_else(|e| {
            log::warn!("{e}");
            librepcb_i18n::set_language_from_system()
        }),
        None => librepcb_i18n::set_language_from_system(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_client_settings() {
        let ini = "[General]\nfoo=1\n\n[workspaces]\nmost_recently_used=/home/me/ws\n";
        assert_eq!(
            most_recently_used_workspace(ini),
            Some(PathBuf::from("/home/me/ws"))
        );
        assert_eq!(
            most_recently_used_workspace("[other]\nmost_recently_used=/x\n"),
            None
        );
    }

    #[test]
    fn cli_wins() {
        assert_eq!(
            workspace_path(Some(Path::new("/tmp/ws"))),
            Some(PathBuf::from("/tmp/ws"))
        );
    }
}
