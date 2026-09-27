//! The installed workspace libraries shown in the libraries panel and on
//! the home tab, and the background library scan.
//!
//! Port of the "installed libraries" part of
//! libs/librepcb/editor/library/librariesmodel.{h,cpp} (both modes: local
//! libraries and libraries installed from the server) and of the library
//! rescan of `WorkspaceLibraryDb`. Fetching the online library list
//! (install/update/uninstall) is not ported yet.

use std::collections::BTreeSet;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use librepcb_app_ui as ui;
use librepcb_core::fileio::FilePath;
use librepcb_core::workspace::{ElementKind, LibraryDb, LibraryScanner};

use crate::icons::decode_png;

/// Lists the installed libraries: local ones (`remote == false`) or those
/// installed from the server. Sorted by name; duplicates (same UUID) are
/// marked.
pub fn installed_libraries(
    db: &LibraryDb,
    remote_dir: &FilePath,
    remote: bool,
    locale_order: &[String],
) -> Vec<ui::LibraryInfoData> {
    let libraries = match db.all(ElementKind::Library, None, None) {
        Ok(l) => l,
        Err(e) => {
            log::error!("Failed to list the workspace libraries: {e}");
            return Vec::new();
        }
    };
    let mut uuids = BTreeSet::new();
    let mut result = Vec::new();
    // Latest versions first, so older duplicates are marked.
    for (_, dir) in libraries.iter().rev() {
        let is_remote = dir.as_path().starts_with(remote_dir.as_path());
        let Ok(Some(info)) = db.metadata(ElementKind::Library, dir) else {
            continue;
        };
        if is_remote != remote {
            continue;
        }
        let texts = db
            .translations(ElementKind::Library, dir, locale_order)
            .ok()
            .flatten()
            .unwrap_or_default();
        let icon = db
            .library_metadata(dir)
            .ok()
            .flatten()
            .filter(|m| !m.icon_png.is_empty())
            .and_then(|m| decode_png(&m.icon_png))
            .unwrap_or_default();
        let duplicate = !uuids.insert(info.uuid);
        result.push(ui::LibraryInfoData {
            uuid: info.uuid.to_string().into(),
            path: dir.to_native().into(),
            icon,
            name: texts.name.into(),
            description: texts.description.into(),
            installed_version: info.version.to_string().into(),
            duplicate,
            checked: !duplicate,
            ..Default::default()
        });
    }
    result.sort_by(|a, b| a.name.cmp(&b.name));
    result
}

/// The `LibraryListData` of a list of installed libraries.
pub fn list_data(libraries: &[ui::LibraryInfoData], remote: bool) -> ui::LibraryListData {
    let count = libraries.len() as i32;
    ui::LibraryListData {
        count,
        installed: if remote { count } else { 0 },
        ..Default::default()
    }
}

/// Scans the workspace libraries on a new thread and calls `done` on the
/// UI thread afterwards (upstream `WorkspaceLibraryDb::startLibraryRescan()`).
pub fn start_rescan(scanner: LibraryScanner, done: impl FnOnce(bool) + Send + 'static) {
    let abort = Arc::new(AtomicBool::new(false));
    let spawned = std::thread::Builder::new()
        .name("library-scan".into())
        .spawn(move || {
            let result = scanner.scan(&abort, &mut |_| {});
            let ok = match result {
                Ok(outcome) => {
                    log::info!("Workspace library scan finished: {outcome:?}");
                    true
                }
                Err(e) => {
                    log::error!("Workspace library scan failed: {e}");
                    false
                }
            };
            if let Err(e) = slint::invoke_from_event_loop(move || done(ok)) {
                log::debug!("Library scan result not delivered: {e}");
            }
        });
    if let Err(e) = spawned {
        log::error!("Failed to start the library scan: {e}");
    }
}
