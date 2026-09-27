//! Session state of the MCP server (see `docs/mcp-design.md`, "Process and
//! state"): at most one workspace and one open project.
//!
//! The session lives behind `Arc<parking_lot::Mutex<Session>>` in the
//! server; tools run their synchronous work in `spawn_blocking` with the
//! lock held only inside the blocking closure, never across `.await`.

use std::sync::Arc;

use librepcb_core::fileio::{
    FilePath, LockStatus, RestoreMode, TransactionalDirectory, TransactionalFileSystem,
};
use librepcb_core::project::{Project, ProjectLoader};
use librepcb_core::workspace::Workspace;
use librepcb_editor::{LibraryElementSource, NoLibrarySource, ProjectEditor};

use crate::error::{ErrorKind, ToolError, ToolResult};

/// Lock handler which overrides stale locks (left behind by a crashed
/// process) and refuses locks of running applications, so upstream
/// LibrePCB and this server never edit the same directory concurrently.
fn override_stale_locks(
    _dir: &FilePath,
    status: LockStatus,
    _owner: &str,
) -> librepcb_core::fileio::Result<bool> {
    Ok(status == LockStatus::StaleLock)
}

/// The open project, edited through the editor's undo stack.
#[derive(Debug)]
pub struct OpenProject {
    /// The project editor (project, undo stack, library element source).
    pub editor: ProjectEditor,
    /// The file system of the project directory; while it exists, the
    /// directory is locked (upstream compatible `.lock` file).
    pub file_system: Arc<TransactionalFileSystem>,
    /// Revision at the last save (or open).
    pub saved_revision: u64,
    /// Whether the project was modified by the loader (file format
    /// upgrade) and not saved yet.
    pub upgraded: bool,
}

impl OpenProject {
    fn new(
        project: Project,
        file_system: Arc<TransactionalFileSystem>,
        upgraded: bool,
        source: Arc<dyn LibraryElementSource>,
    ) -> Self {
        Self {
            saved_revision: project.revision(),
            editor: ProjectEditor::with_source(project, source),
            file_system,
            upgraded,
        }
    }

    /// Returns the project.
    pub fn project(&self) -> &Project {
        self.editor.project()
    }

    /// Whether there are changes which are not saved to disk yet: the undo
    /// stack is not in its clean (saved) state, or the file format was
    /// upgraded. Derived data (air wires, plane fragments) is not saved
    /// and does not count.
    pub fn has_unsaved_changes(&self) -> bool {
        self.upgraded || !self.editor.is_clean()
    }

    /// Returns the path of the project file (`*.lpp`).
    pub fn file_path(&self) -> String {
        self.project()
            .file_path()
            .map(|p| p.to_native())
            .unwrap_or_default()
    }

    /// Writes all project files and commits them to disk.
    pub fn save(&mut self) -> ToolResult<()> {
        self.editor.save()?;
        self.saved_revision = self.project().revision();
        self.upgraded = false;
        Ok(())
    }
}

/// The server session.
#[derive(Debug, Default)]
pub struct Session {
    /// The open workspace, if any.
    pub workspace: Option<Workspace>,
    /// The open project, if any.
    pub project: Option<OpenProject>,
    /// Directory with the application resources (`fontobene/*.bene`), used
    /// for the stroke fonts of new projects; `None`: the fonts embedded
    /// into the binary are used (see [`librepcb_scene::resources`]).
    pub resources_dir: Option<FilePath>,
}

impl Session {
    /// Creates an empty session; the resources directory is taken from the
    /// environment variable `LIBREPCB_SHARE` or `../share/librepcb` next to
    /// the executable, if it exists (else the embedded resources are used,
    /// see [`librepcb_scene::resources`]).
    pub fn new() -> Self {
        let resources_dir = librepcb_scene::resources::resources_dir().and_then(FilePath::new);
        Self {
            workspace: None,
            project: None,
            resources_dir,
        }
    }

    /// Returns the open workspace or a `no_workspace` error.
    pub fn workspace(&self) -> ToolResult<&Workspace> {
        self.workspace.as_ref().ok_or_else(ToolError::no_workspace)
    }

    /// Returns the open project or a `no_project` error.
    pub fn project(&self) -> ToolResult<&OpenProject> {
        self.project.as_ref().ok_or_else(ToolError::no_project)
    }

    /// Returns the open project for modification or a `no_project` error.
    pub fn project_mut(&mut self) -> ToolResult<&mut OpenProject> {
        self.project.as_mut().ok_or_else(ToolError::no_project)
    }

    /// Opens (or creates, if `create`) the workspace at `path`, replacing
    /// the current one.
    pub fn open_workspace(&mut self, path: &str, create: bool) -> ToolResult<&Workspace> {
        let fp = absolute_path(path)?;
        let mut handler = override_stale_locks;
        let ws = if create {
            if fp.is_existing_dir()
                && !fp.is_empty_dir()
                && Workspace::check_compatibility(&fp).is_err()
            {
                return Err(ToolError::new(
                    ErrorKind::Conflict,
                    format!(
                        "The directory \"{}\" is not empty and not a workspace.",
                        fp.to_native()
                    ),
                ));
            }
            Workspace::open_or_create(&fp, Some(&mut handler))?
        } else {
            Workspace::check_compatibility(&fp)?;
            Workspace::open_or_create(&fp, Some(&mut handler))?
        };
        // Release the old workspace (and its lock) first.
        self.workspace = None;
        let source: Arc<dyn LibraryElementSource> = ws.shared_library_db();
        if let Some(open) = &mut self.project {
            open.editor.set_source(source);
        }
        Ok(self.workspace.insert(ws))
    }

    /// Opens the project `lpp` (a `*.lpp` file, or a directory containing
    /// exactly one) with a directory lock. Fails if a project is open.
    pub fn open_project(&mut self, lpp: &str) -> ToolResult<&mut OpenProject> {
        self.ensure_no_project()?;
        let path = absolute_path(lpp)?;
        let (dir, file_name) = if path.is_existing_dir() {
            let lpps: Vec<String> = std::fs::read_dir(path.as_path())
                .map_err(|e| ToolError::new(ErrorKind::Io, e.to_string()))?
                .filter_map(|e| e.ok())
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .filter(|n| n.ends_with(".lpp"))
                .collect();
            match lpps.as_slice() {
                [name] => (path.clone(), name.clone()),
                [] => {
                    return Err(ToolError::not_found(format!(
                        "No *.lpp file in \"{}\".",
                        path.to_native()
                    )));
                }
                _ => {
                    return Err(ToolError::invalid(format!(
                        "Several *.lpp files in \"{}\", pass the file.",
                        path.to_native()
                    )));
                }
            }
        } else {
            let dir = path
                .parent_dir()
                .ok_or_else(|| ToolError::invalid("the project file has no parent directory"))?;
            (dir, path.file_name().to_owned())
        };
        if !path.is_existing_dir() && !path.is_existing_file() {
            return Err(ToolError::not_found(format!(
                "The file \"{}\" does not exist.",
                path.to_native()
            )));
        }
        let mut handler = override_stale_locks;
        let fs = Arc::new(TransactionalFileSystem::open(
            &dir,
            true,
            RestoreMode::No,
            Some(&mut handler),
        )?);
        let mut loader = ProjectLoader::new();
        let project = loader.open(TransactionalDirectory::new(Arc::clone(&fs), ""), &file_name)?;
        let upgraded = loader.migration_log().is_some();
        let source = self.library_source();
        Ok(self
            .project
            .insert(OpenProject::new(project, fs, upgraded, source)))
    }

    /// Installs a newly created project (see the `project_create` tool).
    pub fn set_project(&mut self, project: Project, fs: Arc<TransactionalFileSystem>) {
        let source = self.library_source();
        self.project = Some(OpenProject::new(project, fs, false, source));
    }

    /// The source of library elements added to the project: the workspace
    /// library database, if a workspace is open (elements already in the
    /// project library are always available).
    pub fn library_source(&self) -> Arc<dyn LibraryElementSource> {
        match &self.workspace {
            Some(ws) => ws.shared_library_db(),
            None => Arc::new(NoLibrarySource),
        }
    }

    /// Fails with `conflict` if a project is open.
    pub fn ensure_no_project(&self) -> ToolResult<()> {
        match &self.project {
            Some(p) => Err(ToolError::new(
                ErrorKind::Conflict,
                format!(
                    "The project \"{}\" is still open (use project_close first).",
                    p.file_path()
                ),
            )),
            None => Ok(()),
        }
    }

    /// Closes the project (discarding unsaved changes if `discard`).
    pub fn close_project(&mut self, discard: bool) -> ToolResult<()> {
        let open = self.project()?;
        if open.has_unsaved_changes() && !discard {
            return Err(ToolError::new(
                ErrorKind::Conflict,
                "The project has unsaved changes (save it with project_save, or pass \
                 discard_changes=true).",
            ));
        }
        // Dropping the file system releases the directory lock.
        self.project = None;
        Ok(())
    }
}

/// Converts a path argument into an absolute [`FilePath`] (relative paths
/// are relative to the server's working directory).
pub fn absolute_path(path: &str) -> ToolResult<FilePath> {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return Err(ToolError::invalid("empty path"));
    }
    let abs = std::path::absolute(trimmed)
        .map_err(|e| ToolError::invalid(format!("invalid path \"{trimmed}\": {e}")))?;
    FilePath::new(&abs).ok_or_else(|| ToolError::invalid(format!("invalid path \"{trimmed}\"")))
}
