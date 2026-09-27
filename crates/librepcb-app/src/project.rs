//! A project opened in the application.
//!
//! Port of the project handling of libs/librepcb/editor/project/projecteditor.{h,cpp}
//! and `GuiApplication::openProject()`: opening a `*.lpp` file with a
//! directory lock (read-only if the project is locked by another
//! application or not writable), and the project's `ProjectData` for the
//! documents panel.
//!
//! The project lives in a [`SharedProject`] (`Arc<Mutex<OpenProject>>`
//! holding the [`ProjectEditor`](librepcb_editor::ProjectEditor) and the
//! directory lock) so that background jobs and the embedded MCP server can
//! share it (see `docs/ui-design.md`, decisions 3 and 4).

use std::cell::RefCell;
use std::sync::Arc;

use librepcb_app_ui as ui;
use librepcb_core::fileio::{FilePath, TransactionalDirectory, TransactionalFileSystem};
use librepcb_core::project::{BoardId, ProjectLoader, SchematicId};
use librepcb_editor::{LibraryElementSource, OpenProject, SharedProject};

use crate::models::vec_model;
use crate::rule_check::ProjectChecks;

/// Errors when opening a project.
#[derive(Debug, thiserror::Error)]
pub enum OpenError {
    /// Not a project file.
    #[error("Not a LibrePCB project file: {0}")]
    NotAProject(String),
    /// File system error (locks, I/O).
    #[error(transparent)]
    FileIo(#[from] librepcb_core::fileio::Error),
    /// Loading failed (boxed: large).
    #[error(transparent)]
    Project(Box<librepcb_core::project::Error>),
}

impl From<librepcb_core::project::Error> for OpenError {
    fn from(e: librepcb_core::project::Error) -> Self {
        Self::Project(Box::new(e))
    }
}

/// A project open in the application.
pub struct AppProject {
    shared: SharedProject,
    path: FilePath,
    writable: bool,
    /// ERC and DRC results (UI thread only).
    checks: RefCell<ProjectChecks>,
}

impl std::fmt::Debug for AppProject {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AppProject")
            .field("path", &self.path)
            .field("writable", &self.writable)
            .finish_non_exhaustive()
    }
}

impl AppProject {
    /// Opens a project file (`*.lpp`). Falls back to read-only mode if the
    /// directory cannot be locked (e.g. the project is open in another
    /// application).
    pub fn open(lpp: &FilePath, source: Arc<dyn LibraryElementSource>) -> Result<Self, OpenError> {
        if lpp.suffix() != "lpp" || !lpp.is_existing_file() {
            return Err(OpenError::NotAProject(lpp.to_native()));
        }
        let dir = lpp
            .parent_dir()
            .ok_or_else(|| OpenError::NotAProject(lpp.to_native()))?;
        let (project, writable) = match OpenProject::open(&dir, lpp.file_name(), source.clone()) {
            Ok(p) => (p, true),
            Err(librepcb_editor::Error::FileIo(e)) => {
                log::warn!("Opening {} read-only: {e}", dir.to_native());
                let fs = Arc::new(TransactionalFileSystem::open_ro(&dir)?);
                let mut loader = ProjectLoader::new();
                let project = loader.open(
                    TransactionalDirectory::new(Arc::clone(&fs), ""),
                    lpp.file_name(),
                )?;
                let upgraded = loader.migration_log().is_some();
                (OpenProject::new(project, fs, upgraded, source), false)
            }
            Err(librepcb_editor::Error::Project(e)) => return Err(OpenError::Project(e)),
            Err(e) => return Err(OpenError::NotAProject(format!("{}: {e}", lpp.to_native()))),
        };
        Ok(Self {
            shared: project.into_shared(),
            path: lpp.clone(),
            writable,
            checks: RefCell::default(),
        })
    }

    /// Wraps a project which is already open (e.g. created by the embedded
    /// MCP server); `None` if it has no project file on disk.
    pub fn from_shared(shared: SharedProject) -> Option<Self> {
        let (path, writable) = {
            let p = shared.lock();
            (p.project().file_path()?, p.file_system.is_writable())
        };
        Some(Self {
            shared,
            path,
            writable,
            checks: RefCell::default(),
        })
    }

    /// The rule check results (ERC, DRC per board).
    pub fn checks(&self) -> &RefCell<ProjectChecks> {
        &self.checks
    }

    /// The shared project (editor, undo stack, directory lock).
    pub fn shared(&self) -> &SharedProject {
        &self.shared
    }

    /// The project file.
    pub fn path(&self) -> &FilePath {
        &self.path
    }

    /// Whether the project can be saved.
    pub fn is_writable(&self) -> bool {
        self.writable
    }

    /// The project's revision (changes with every modification).
    pub fn revision(&self) -> u64 {
        self.shared.lock().project().revision()
    }

    /// The schematic at a documents panel index.
    pub fn schematic_at(&self, index: usize) -> Option<(SchematicId, String)> {
        let p = self.shared.lock();
        p.project()
            .schematics()
            .get(index)
            .map(|s| (s.id(), s.properties().name.to_string()))
    }

    /// The board at a documents panel index.
    pub fn board_at(&self, index: usize) -> Option<(BoardId, String)> {
        let p = self.shared.lock();
        p.project()
            .boards()
            .get(index)
            .map(|b| (b.id(), b.properties().name.to_string()))
    }

    /// The data for `Data.projects`.
    pub fn ui_data(&self) -> ui::ProjectData {
        let p = self.shared.lock();
        let project = p.project();
        let checks = self.checks.borrow();
        let undo_state = p.editor.undo_stack().state_id();
        let read_only = !self.writable;
        let schematics = project
            .schematics()
            .iter()
            .map(|s| ui::SchematicData {
                name: s.properties().name.to_string().into(),
            })
            .collect();
        let boards = project
            .boards()
            .iter()
            .map(|b| ui::BoardData {
                name: b.properties().name.to_string().into(),
                drc: checks.drc_data(b.id(), undo_state, read_only),
                order_upload_progress: -1,
                ..Default::default()
            })
            .collect();
        ui::ProjectData {
            valid: true,
            path: self.path.to_native().into(),
            name: project.metadata().name.to_string().into(),
            schematics: vec_model(schematics),
            boards: vec_model(boards),
            writable: self.writable,
            ieee315_symbols: false,
            unsaved_changes: p.has_unsaved_changes(),
            buses: vec_model(Vec::new()),
            erc: checks.erc_data(read_only),
        }
    }
}
