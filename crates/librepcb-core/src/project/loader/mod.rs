//! Port of libs/librepcb/core/project/projectloader.{h,cpp}.
//!
//! Loads a project in the same order as upstream: version check, file
//! format migrations, metadata, settings, output jobs, library, circuit,
//! ERC approvals, schematics (index file order), boards (index file
//! order), user settings. Loading goes through the same validating
//! insertion functions as editing, so the error messages are upstream's;
//! the change journal is cleared afterwards.
//!
//! Differences to upstream:
//! - Projects in an older file format are rejected with
//!   [`Error::MigrationRequired`] until the file format migrations are
//!   ported (their single call site is marked below); the migration log
//!   (`logs/*_migration_to_v*.html`) and the ERC approval cleanup after a
//!   migration are therefore not ported yet.
//! - `mAutoAssignDeviceModels` belongs to the board loading
//!   (TODO(wave3b/board)).

mod board;
mod circuit;
mod schematic;

use std::collections::BTreeSet;

use super::Project;
use super::error::{Error, Result};
use super::library::{LibraryElementKind, ProjectLibrary};
use super::ref_index::RefIndex;
use crate::application;
use crate::attribute::AttributeList;
use crate::fileio::{FilePath, FileSystem, TransactionalDirectory, VersionFile};
use crate::library::LibraryBaseElement;
use crate::serialization::{DeserializeObject, Mode, SExpression};

/// Loads projects, see the module documentation.
#[derive(Debug, Default)]
pub struct ProjectLoader {}

impl ProjectLoader {
    /// Creates a loader with default options.
    pub fn new() -> Self {
        Self::default()
    }

    /// Opens the project `file_name` (`*.lpp`) in `directory`.
    pub fn open(&self, directory: TransactionalDirectory, file_name: &str) -> Result<Project> {
        let file_path = || {
            directory
                .abs_path(file_name)
                .unwrap_or_else(|| FilePath::new("/").expect("valid path"))
        };
        if !directory.file_exists(file_name) {
            return Err(Error::ProjectFileNotFound(file_path()));
        }
        if !directory.file_exists(".librepcb-project") {
            return Err(Error::NotAProjectDirectory(directory.abs_path("")));
        }
        let file_format = VersionFile::from_bytes(&directory.read(".librepcb-project")?)?
            .version()
            .clone();
        log::debug!("Detected project file format: {file_format}");
        let current = application::file_format_version();
        if file_format > current {
            return Err(Error::NewerFileFormat {
                version: file_format,
                path: file_path(),
            });
        }
        if file_format < current {
            // TODO(wave3b/migrations): run `FileFormatMigration`s on
            // `directory` here (upgrading from `file_format` to `current`),
            // write the migration log, and after loading run the ERC to
            // drop obsolete approvals and save the project.
            return Err(Error::MigrationRequired {
                version: file_format,
                path: file_path(),
            });
        }
        let mut p = Project::new(directory, file_name, crate::types::Uuid::new_random())?;
        load_metadata(&mut p)?;
        load_settings(&mut p)?;
        load_output_jobs(&mut p)?;
        load_library(&mut p)?;
        circuit::load_circuit(&mut p)?;
        load_erc(&mut p)?;
        schematic::load_schematics(&mut p)?;
        board::load_boards(&mut p)?;
        load_project_user_settings(&p);
        p.refs = RefIndex::build(&p);
        p.journal.reset();
        log::debug!("Successfully opened project.");
        Ok(p)
    }
}

impl Project {
    /// Opens the project `file_name` in `directory` with the default loader
    /// options.
    pub fn open(directory: TransactionalDirectory, file_name: &str) -> Result<Self> {
        ProjectLoader::new().open(directory, file_name)
    }
}

/// Parses the file `path` of the project directory.
pub(super) fn parse_file(p: &Project, path: &str) -> Result<SExpression> {
    let content = p.directory.read(path)?;
    let file_path = p.directory.abs_path(path);
    Ok(SExpression::parse(
        &content,
        file_path.as_ref().map(FilePath::as_path),
        Mode::LibrePcb,
    )?)
}

fn load_metadata(p: &mut Project) -> Result<()> {
    let root = parse_file(p, "project/metadata.lp")?;
    p.uuid = root.child_value("@0")?;
    p.metadata.name = root.child_value("name/@0")?;
    p.metadata.author = root.child_value("author/@0")?;
    p.metadata.version = root.child_value("version/@0")?;
    p.metadata.created = root.child_value("created/@0")?;
    p.metadata.attributes = AttributeList::deserialize(&root)?;
    Ok(())
}

fn load_settings(p: &mut Project) -> Result<()> {
    let root = parse_file(p, "project/settings.lp")?;
    let strings = |list: &str, item: &str| -> Result<Vec<String>> {
        Ok(root
            .required_child(list)?
            .children_named(item)
            .map(|n| n.child_value("@0"))
            .collect::<crate::serialization::Result<_>>()?)
    };
    p.settings.locale_order = strings("library_locale_order", "locale")?;
    p.settings.norm_order = strings("library_norm_order", "norm")?;
    p.settings.custom_bom_attributes = strings("custom_bom_attributes", "attribute")?;
    p.settings.default_lock_component_assembly =
        root.child_value("default_lock_component_assembly/@0")?;
    Ok(())
}

fn load_output_jobs(p: &mut Project) -> Result<()> {
    // Kept as raw S-expression until the `job` module is ported.
    p.output_jobs = parse_file(p, "project/jobs.lp")?;
    Ok(())
}

fn load_library(p: &mut Project) -> Result<()> {
    load_library_elements(p, LibraryElementKind::Symbol, ProjectLibrary::add_symbol)?;
    load_library_elements(p, LibraryElementKind::Package, ProjectLibrary::add_package)?;
    load_library_elements(
        p,
        LibraryElementKind::Component,
        ProjectLibrary::add_component,
    )?;
    load_library_elements(p, LibraryElementKind::Device, ProjectLibrary::add_device)?;
    Ok(())
}

/// Loads all elements of one kind from `library/<kind>/*/` (upstream
/// `loadLibraryElements()`); invalid directories are skipped with a warning.
fn load_library_elements<E: LibraryBaseElement>(
    p: &mut Project,
    kind: LibraryElementKind,
    add: fn(&mut ProjectLibrary, E) -> Result<()>,
) -> Result<()> {
    let dirname = kind.directory_name();
    let mut count = 0;
    for sub in p.library.directory().dirs(dirname) {
        let dir = p
            .library
            .directory_mut()
            .subdir(&format!("{dirname}/{sub}"));
        if !E::is_valid_element_directory(&dir, "") {
            log::warn!(
                "Invalid directory in project library, ignoring it: {}",
                dir.abs_path("").map(|p| p.to_native()).unwrap_or_default()
            );
            continue;
        }
        add(&mut p.library, E::open(dir)?)?;
        count += 1;
    }
    log::debug!("Successfully loaded {count} {dirname} elements.");
    Ok(())
}

fn load_erc(p: &mut Project) -> Result<()> {
    let root = parse_file(p, "circuit/erc.lp")?;
    p.erc_approvals = root
        .children_named("approved")
        .cloned()
        .collect::<BTreeSet<_>>();
    Ok(())
}

fn load_project_user_settings(p: &Project) {
    // upstream: parses `project/settings.user.lp` (currently without
    // content) and uses defaults on error.
    if let Err(e) = parse_file(p, "project/settings.user.lp") {
        log::error!("Could not load project user settings, defaults will be used instead: {e}");
    }
}

/// Splits an index file entry like `schematics/main/schematic.lp` into the
/// directory (`schematics/main`) and the directory name (`main`), like
/// upstream's `FilePath::getParentDir()`/`getFilename()`.
pub(super) fn split_index_path(relative: &str) -> (String, String) {
    let dir = relative.rsplit_once('/').map_or("", |(d, _)| d);
    let name = dir.rsplit_once('/').map_or(dir, |(_, n)| n);
    (dir.to_owned(), name.to_owned())
}
