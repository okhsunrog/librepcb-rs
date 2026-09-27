//! The MCP server: tool registration (rmcp tool router) on top of the
//! synchronous tool implementations in [`tools`](crate::tools).
//!
//! Every tool runs its core work in `tokio::task::spawn_blocking` with the
//! session lock held only inside the blocking closure (never across
//! `.await`). Results use the envelope of [`outcome`](crate::outcome).

use std::sync::Arc;

use parking_lot::Mutex;
use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, Implementation, ServerCapabilities, ServerConfig};
use rmcp::{ErrorData as McpError, ServerHandler, tool, tool_handler, tool_router};

use crate::error::{ToolError, ToolResult};
use crate::outcome::{ToolOutput, error_result};
use crate::session::Session;
use crate::tools::circuit::{ComponentGetArgs, NetListArgs};
use crate::tools::layout::{BoardGetArgs, SchematicGetArgs};
use crate::tools::library::{LibraryElementArgs, LibraryInstallArgs, LibrarySearchArgs};
use crate::tools::mutation::{MutationApplyArgs, MutationSchemaArgs};
use crate::tools::output::{
    ErcArgs, ExportBomArgs, ExportFabricationArgs, ExportNetlistArgs, ExportPickPlaceArgs,
    RenderArgs,
};
use crate::tools::project::{
    ProjectCloseArgs, ProjectCreateArgs, ProjectOpenArgs, ProjectSaveArgs, WorkspacePathArgs,
};
use crate::tools::{circuit, layout, library, mutation, output, project};

/// Instructions sent to the client on initialization.
const INSTRUCTIONS: &str = "LibrePCB MCP server: design schematics and boards of LibrePCB \
projects. One workspace (libraries) and one project are open at a time: use project_open or \
project_create first, project_save to write the files (upstream LibrePCB opens them). Units: \
millimeters and degrees. Address entities by designator (\"R1\"), pin (\"R1.1\" pad name or \
\"U1.VCC\" signal name), net name, schematic/board name or index; UUIDs work everywhere. \
Every result has {outcome, revision, result, warnings}; errors have a stable kind \
(not_found, invalid_argument, conflict, stale_revision, no_project, no_workspace, io, network, \
internal). Pass expected_revision to write tools to avoid lost updates.";

/// The LibrePCB MCP server (cheap to clone; clones share the session).
#[derive(Clone)]
pub struct LibrePcbMcp {
    session: Arc<Mutex<Session>>,
    tool_router: ToolRouter<Self>,
}

impl std::fmt::Debug for LibrePcbMcp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LibrePcbMcp").finish_non_exhaustive()
    }
}

impl LibrePcbMcp {
    /// Creates a server on `session`.
    pub fn new(session: Session) -> Self {
        Self::with_shared_session(Arc::new(Mutex::new(session)))
    }

    /// Creates a server on a shared session (e.g. one per HTTP client
    /// session, all working on the same project).
    pub fn with_shared_session(session: Arc<Mutex<Session>>) -> Self {
        Self {
            session,
            tool_router: Self::tool_router(),
        }
    }

    /// Returns the shared session.
    pub fn session(&self) -> &Arc<Mutex<Session>> {
        &self.session
    }

    /// Returns the names of all tools.
    pub fn tool_names(&self) -> Vec<String> {
        self.tool_router
            .list_all()
            .into_iter()
            .map(|t| t.name.into_owned())
            .collect()
    }

    /// Runs `f` on the session in a blocking thread.
    async fn blocking<T, F>(&self, f: F) -> ToolResult<T>
    where
        T: Send + 'static,
        F: FnOnce(&mut Session) -> ToolResult<T> + Send + 'static,
    {
        let session = Arc::clone(&self.session);
        tokio::task::spawn_blocking(move || f(&mut session.lock()))
            .await
            .map_err(|e| ToolError::internal(format!("tool failed: {e}")))?
    }

    /// Runs a tool implementation and builds the MCP result.
    async fn run<F>(&self, f: F) -> Result<CallToolResult, McpError>
    where
        F: FnOnce(&mut Session) -> ToolResult<ToolOutput> + Send + 'static,
    {
        let result = self
            .blocking(move |s| {
                let revision = |s: &Session| s.project.as_ref().map(|p| p.project.revision());
                Ok(match f(s) {
                    Ok(mut out) => {
                        out.revision = revision(s);
                        out.into_call_tool_result()
                    }
                    Err(e) => error_result(&e, revision(s)),
                })
            })
            .await;
        Ok(result.unwrap_or_else(|e| error_result(&e, None)))
    }
}

#[tool_router]
impl LibrePcbMcp {
    // --- Session / project -------------------------------------------------

    #[tool(
        description = "Server version, file format, units, open workspace and project (with \
                       revision and unsaved state)."
    )]
    async fn server_info(&self) -> Result<CallToolResult, McpError> {
        self.run(|s| project::server_info(s)).await
    }

    #[tool(
        description = "Open an existing LibrePCB workspace (libraries, settings). Replaces the \
                       current workspace."
    )]
    async fn workspace_open(
        &self,
        Parameters(args): Parameters<WorkspacePathArgs>,
    ) -> Result<CallToolResult, McpError> {
        self.run(move |s| project::workspace_open(s, args, false))
            .await
    }

    #[tool(
        description = "Create a new workspace in an empty (or new) directory and open it; an \
                       existing workspace is just opened."
    )]
    async fn workspace_create(
        &self,
        Parameters(args): Parameters<WorkspacePathArgs>,
    ) -> Result<CallToolResult, McpError> {
        self.run(move |s| project::workspace_open(s, args, true))
            .await
    }

    #[tool(
        description = "Create a new project (like upstream's new project wizard: default \
                       assembly variant and net class, stroke fonts, optional first schematic \
                       page \"Main\" and board \"default\" with a 100x80 mm outline), save it \
                       and open it."
    )]
    async fn project_create(
        &self,
        Parameters(args): Parameters<ProjectCreateArgs>,
    ) -> Result<CallToolResult, McpError> {
        self.run(move |s| project::project_create(s, args)).await
    }

    #[tool(
        description = "Open a project (*.lpp file or its directory) and lock it like LibrePCB \
                       does. Older file formats are upgraded in memory."
    )]
    async fn project_open(
        &self,
        Parameters(args): Parameters<ProjectOpenArgs>,
    ) -> Result<CallToolResult, McpError> {
        self.run(move |s| project::project_open(s, args)).await
    }

    #[tool(description = "Save the open project to disk (upstream file format).")]
    async fn project_save(
        &self,
        Parameters(args): Parameters<ProjectSaveArgs>,
    ) -> Result<CallToolResult, McpError> {
        self.run(move |s| project::project_save(s, args)).await
    }

    #[tool(
        description = "Close the open project and release its lock (fails with unsaved \
                       changes unless discard_changes=true)."
    )]
    async fn project_close(
        &self,
        Parameters(args): Parameters<ProjectCloseArgs>,
    ) -> Result<CallToolResult, McpError> {
        self.run(move |s| project::project_close(s, args)).await
    }

    #[tool(
        description = "Overview of the open project: metadata, revision, unsaved changes, \
                       counts, assembly variants, net classes, schematic pages and boards."
    )]
    async fn project_summary(&self) -> Result<CallToolResult, McpError> {
        self.run(|s| project::project_summary(s)).await
    }

    // --- Library -------------------------------------------------------------

    #[tool(description = "List the libraries installed in the workspace.")]
    async fn library_list(&self) -> Result<CallToolResult, McpError> {
        self.run(|s| library::library_list(s)).await
    }

    #[tool(
        description = "Rescan the workspace libraries into the library index (after adding \
                       library directories to <workspace>/data/libraries/local)."
    )]
    async fn library_rescan(&self) -> Result<CallToolResult, McpError> {
        self.run(|s| library::library_rescan(s)).await
    }

    #[tool(
        description = "Install official libraries from api.librepcb.org into the workspace \
                       (with dependencies) and rescan; without names, list the available \
                       libraries."
    )]
    async fn library_install(
        &self,
        Parameters(args): Parameters<LibraryInstallArgs>,
    ) -> Result<CallToolResult, McpError> {
        let target = match self.blocking(|s| library::install_target(s)).await {
            Ok(t) => t,
            Err(e) => return self.run(move |_| Err(e)).await,
        };
        match library::install_download(&target, &args.libraries).await {
            Ok((available, installed)) => {
                self.run(move |s| library::install_finish(s, available, installed))
                    .await
            }
            Err(e) => self.run(move |_| Err(e)).await,
        }
    }

    #[tool(
        description = "Search library elements (components, devices, packages, symbols) by \
                       keyword or part number in the workspace libraries and the project \
                       library."
    )]
    async fn library_search(
        &self,
        Parameters(args): Parameters<LibrarySearchArgs>,
    ) -> Result<CallToolResult, McpError> {
        self.run(move |s| library::library_search(s, args)).await
    }

    #[tool(
        description = "Details of a library element: component signals, gates and symbol \
                       variants (pin to signal map); device pad to signal map, parts, package \
                       footprints with pad positions and sizes; symbol pins."
    )]
    async fn library_element(
        &self,
        Parameters(args): Parameters<LibraryElementArgs>,
    ) -> Result<CallToolResult, McpError> {
        self.run(move |s| library::library_element(s, args)).await
    }

    // --- Circuit & schematic ---------------------------------------------------

    #[tool(description = "List all components: designator, value, library component, placement.")]
    async fn component_list(&self) -> Result<CallToolResult, McpError> {
        self.run(|s| circuit::component_list(s)).await
    }

    #[tool(
        description = "Details of a component: signals with nets, placed symbols with pin \
                       positions, devices with pad positions."
    )]
    async fn component_get(
        &self,
        Parameters(args): Parameters<ComponentGetArgs>,
    ) -> Result<CallToolResult, McpError> {
        self.run(move |s| circuit::component_get(s, args)).await
    }

    #[tool(description = "Nets with their pins (R1.1 style), pads and routing items.")]
    async fn net_list(
        &self,
        Parameters(args): Parameters<NetListArgs>,
    ) -> Result<CallToolResult, McpError> {
        self.run(move |s| circuit::net_list(s, args)).await
    }

    #[tool(description = "Compact netlist: net name -> pins, plus unconnected pins.")]
    async fn netlist(&self) -> Result<CallToolResult, McpError> {
        self.run(|s| circuit::netlist(s)).await
    }

    #[tool(description = "List the schematic pages.")]
    async fn schematic_list(&self) -> Result<CallToolResult, McpError> {
        self.run(|s| layout::schematic_list(s)).await
    }

    #[tool(
        description = "All items of a schematic page with coordinates: symbols with pins, net \
                       segments (junctions, lines, labels), texts."
    )]
    async fn schematic_get(
        &self,
        Parameters(args): Parameters<SchematicGetArgs>,
    ) -> Result<CallToolResult, McpError> {
        self.run(move |s| layout::schematic_get(s, args)).await
    }

    // --- Board -----------------------------------------------------------------

    #[tool(
        description = "A board: outline, devices with pads, traces, vias, planes and the \
                       unrouted connections (air wires)."
    )]
    async fn board_get(
        &self,
        Parameters(args): Parameters<BoardGetArgs>,
    ) -> Result<CallToolResult, McpError> {
        self.run(move |s| layout::board_get(s, args)).await
    }

    // --- Checks and outputs ------------------------------------------------------

    #[tool(
        description = "Run the electrical rule check; returns ran=true and the messages \
                       (severity, text, location)."
    )]
    async fn erc_run(
        &self,
        Parameters(args): Parameters<ErcArgs>,
    ) -> Result<CallToolResult, McpError> {
        self.run(move |s| output::erc_run(s, args)).await
    }

    #[tool(
        description = "Export Gerber and Excellon fabrication files of a board (planes are \
                       rebuilt first)."
    )]
    async fn export_fabrication(
        &self,
        Parameters(args): Parameters<ExportFabricationArgs>,
    ) -> Result<CallToolResult, McpError> {
        self.run(move |s| output::export_fabrication(s, args)).await
    }

    #[tool(description = "Export the bill of materials as CSV (also returned inline).")]
    async fn export_bom(
        &self,
        Parameters(args): Parameters<ExportBomArgs>,
    ) -> Result<CallToolResult, McpError> {
        self.run(move |s| output::export_bom(s, args)).await
    }

    #[tool(description = "Export pick&place data of a board as CSV.")]
    async fn export_pick_place(
        &self,
        Parameters(args): Parameters<ExportPickPlaceArgs>,
    ) -> Result<CallToolResult, McpError> {
        self.run(move |s| output::export_pick_place(s, args)).await
    }

    #[tool(description = "Export the IPC-D-356A netlist of a board.")]
    async fn export_netlist(
        &self,
        Parameters(args): Parameters<ExportNetlistArgs>,
    ) -> Result<CallToolResult, McpError> {
        self.run(move |s| output::export_netlist(s, args)).await
    }

    #[tool(description = "List the output jobs of the project.")]
    async fn jobs_list(&self) -> Result<CallToolResult, McpError> {
        self.run(|s| output::jobs_list(s)).await
    }

    #[tool(
        description = "Render a schematic page or a board side as PNG image, to look at the \
                       design."
    )]
    async fn render(
        &self,
        Parameters(args): Parameters<RenderArgs>,
    ) -> Result<CallToolResult, McpError> {
        self.run(move |s| output::render(s, args)).await
    }

    // --- Escape hatch -------------------------------------------------------------

    #[tool(
        description = "Apply a raw core Mutation (serde JSON, integer nanometers) with full \
                       validation; returns the new revision and the change events. Prefer \
                       the intent-level tools; see mutation_schema."
    )]
    async fn mutation_apply(
        &self,
        Parameters(args): Parameters<MutationApplyArgs>,
    ) -> Result<CallToolResult, McpError> {
        self.run(move |s| mutation::mutation_apply(s, args)).await
    }

    #[tool(
        description = "Describe the Mutation JSON format: conventions, variants, examples, \
                       and the prefilled update mutation of an existing entity."
    )]
    async fn mutation_schema(
        &self,
        Parameters(args): Parameters<MutationSchemaArgs>,
    ) -> Result<CallToolResult, McpError> {
        self.run(move |s| mutation::mutation_schema(s, args)).await
    }
}

#[tool_handler]
impl ServerHandler for LibrePcbMcp {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new(
                "librepcb-mcp",
                env!("CARGO_PKG_VERSION"),
            ))
            .with_instructions(INSTRUCTIONS)
    }
}
