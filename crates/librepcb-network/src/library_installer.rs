//! Installation of libraries from a LibrePCB API server into a workspace
//! (no direct upstream counterpart; the steps follow upstream
//! `editor::LibrariesModel::applyChanges()` without the UI).
//!
//! Libraries are installed into `<workspace>/data/libraries/remote/
//! <uuid>.lplib` with a [`LibraryDownload`] each (in parallel, at most four
//! file operations at the same time like upstream). Afterwards the caller
//! rescans the workspace library database, e.g.
//! `tokio::task::spawn_blocking(move || ws.library_db().rescan(...))`.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use futures_util::future::join_all;
use librepcb_core::fileio::FilePath;
use librepcb_core::types::{Uuid, Version};
use librepcb_core::workspace::{ElementKind, LibraryDb};
use tokio::sync::Semaphore;

use crate::api_endpoint::{ApiEndpoint, Library};
use crate::error::{Error, Result};
use crate::file_download::ChecksumAlgorithm;
use crate::library_download::LibraryDownload;
use crate::network_access_manager::NetworkAccessManager;

/// A successfully installed library.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstalledLibrary {
    /// Library UUID.
    pub uuid: Uuid,
    /// Library name.
    pub name: String,
    /// Installed version.
    pub version: Version,
    /// Library directory.
    pub directory: FilePath,
}

/// Requests the complete (all pages) library list from an API endpoint.
pub async fn fetch_library_list(
    nam: &NetworkAccessManager,
    endpoint: &ApiEndpoint,
) -> Result<Vec<Library>> {
    let mut libraries = Vec::new();
    endpoint
        .request_library_list(nam, |page| libraries.extend(page))
        .await?;
    Ok(libraries)
}

/// Selects libraries from `available` by UUID or name (case insensitive),
/// optionally adding their dependencies (recursively). The result is in
/// the order of `available`, without duplicates.
pub fn select_libraries<'a, S: AsRef<str>>(
    available: &'a [Library],
    queries: &[S],
    with_dependencies: bool,
) -> Result<Vec<&'a Library>> {
    let mut selected: HashSet<Uuid> = HashSet::new();
    let mut todo: Vec<Uuid> = Vec::new();
    for query in queries {
        let query = query.as_ref().trim();
        let lib = available
            .iter()
            .find(|lib| lib.uuid.to_string() == query.to_lowercase())
            .or_else(|| {
                available
                    .iter()
                    .find(|lib| lib.name.to_lowercase() == query.to_lowercase())
            })
            .ok_or_else(|| Error::LibraryNotFound(query.to_owned()))?;
        todo.push(lib.uuid);
    }
    while let Some(uuid) = todo.pop() {
        if !selected.insert(uuid) || !with_dependencies {
            continue;
        }
        if let Some(lib) = available.iter().find(|lib| lib.uuid == uuid) {
            for dependency in &lib.dependencies {
                if available.iter().any(|l| l.uuid == *dependency) {
                    todo.push(*dependency);
                } else {
                    log::warn!(
                        "Dependency {dependency} of library {} not available.",
                        lib.name
                    );
                }
            }
        }
    }
    Ok(available
        .iter()
        .filter(|lib| selected.contains(&lib.uuid))
        .collect())
}

/// Returns the directories of all libraries in the database, by UUID (to
/// replace them when installing a library; upstream
/// `LibrariesModel::mInstalledLibDirs`).
pub fn installed_library_dirs(db: &LibraryDb) -> Result<HashMap<Uuid, HashSet<FilePath>>> {
    let mut dirs: HashMap<Uuid, HashSet<FilePath>> = HashMap::new();
    for (_, dir) in db.all(ElementKind::Library, None, None)? {
        if let Some(info) = db.metadata(ElementKind::Library, &dir)? {
            dirs.entry(info.uuid).or_default().insert(dir);
        }
    }
    Ok(dirs)
}

/// Downloads and installs libraries into `remote_libraries_dir` (as
/// `<uuid>.lplib`), in parallel. Other directories of the same library
/// listed in `existing_dirs` (see [`installed_library_dirs()`]) are removed
/// after a successful installation.
///
/// All downloads are run to completion; if any failed, the first error is
/// returned (the other libraries are installed anyway).
pub async fn install_libraries(
    nam: &NetworkAccessManager,
    libraries: &[&Library],
    remote_libraries_dir: &FilePath,
    existing_dirs: &HashMap<Uuid, HashSet<FilePath>>,
) -> Result<Vec<InstalledLibrary>> {
    // Limit number of parallel file access threads (like upstream).
    let semaphore = Arc::new(Semaphore::new(4));
    let downloads = libraries.iter().map(|lib| {
        let semaphore = Arc::clone(&semaphore);
        async move {
            let result = async {
                let url = lib
                    .download_url
                    .clone()
                    .ok_or_else(|| Error::NoDownloadUrl(lib.name.clone()))?;
                let dest_dir = remote_libraries_dir.path_to(&format!("{}.lplib", lib.uuid));
                let mut dl = LibraryDownload::new(url, dest_dir, semaphore)?;
                if let Some(size) = lib.download_size.filter(|s| *s > 0) {
                    dl = dl.expected_zip_file_size(size);
                }
                if !lib.download_sha256.is_empty() {
                    let checksum = hex::decode(&lib.download_sha256)
                        .map_err(|e| Error::Transport(format!("invalid SHA-256: {e}")))?;
                    dl = dl.expected_checksum(ChecksumAlgorithm::Sha256, checksum);
                }
                if let Some(dirs) = existing_dirs.get(&lib.uuid) {
                    dl = dl.existing_dirs_to_replace(dirs.clone());
                }
                log::info!("Install library {} v{}...", lib.name, lib.version);
                let directory = dl.download(nam).await?;
                Ok(InstalledLibrary {
                    uuid: lib.uuid,
                    name: lib.name.clone(),
                    version: lib.version.clone(),
                    directory,
                })
            }
            .await;
            result.map_err(|e| Error::LibraryInstall {
                library: lib.name.clone(),
                source: Box::new(e),
            })
        }
    });
    join_all(downloads).await.into_iter().collect()
}

/// Installs libraries of the API server `endpoint` selected by UUID or
/// name (see [`select_libraries()`], including dependencies) into
/// `remote_libraries_dir`, e.g.
/// `install_official_libraries(&nam, &endpoint, &ws.remote_libraries_path(),
/// &["LibrePCB Base"], &existing)`.
///
/// Rescan the library database afterwards.
pub async fn install_official_libraries<S: AsRef<str>>(
    nam: &NetworkAccessManager,
    endpoint: &ApiEndpoint,
    remote_libraries_dir: &FilePath,
    queries: &[S],
    existing_dirs: &HashMap<Uuid, HashSet<FilePath>>,
) -> Result<Vec<InstalledLibrary>> {
    let available = fetch_library_list(nam, endpoint).await?;
    let selected = select_libraries(&available, queries, true)?;
    install_libraries(nam, &selected, remote_libraries_dir, existing_dirs).await
}
