//! The LibrePCB graphical application (binary `librepcb`): upstream's Slint
//! UI with a Rust backend.
//!
//! Port of the application part of libs/librepcb/editor (`GuiApplication`,
//! `MainWindow`, `WindowSection`, `WindowTab`, the home, schematic and
//! board tabs, `SlintGraphicsView`, ...) and apps/librepcb/main.cpp. See
//! `docs/ui-design.md` for the design.
//!
//! # Architecture
//!
//! - **UI:** upstream's `libs/librepcb/ui/**` `.slint` files, copied into
//!   `ui/` (see `ui/PROVENANCE.md`) and compiled by the `librepcb-app-ui`
//!   crate (`ui/Cargo.toml`) with the `lang/` translations bundled. The UI
//!   talks to the backend only through the `Data` global (view state as
//!   structs and models) and the `Backend` global (callbacks).
//! - **Backend** (this crate): [`App`] binds both globals. It holds the
//!   workspace, the open projects ([`project::AppProject`], a shared
//!   `librepcb_editor::OpenProject`), the window sections
//!   ([`section::WindowSection`]) and their tabs ([`tabs::Tab`]).
//!   Models written by the UI (tab rows, tree items, notifications, layers)
//!   are [`models::UiModel`]s, which report UI writes asynchronously.
//! - **Scenes:** the schematic and board tabs build `librepcb-scene`
//!   scenes and render them with `librepcb-canvas` (vello_cpu) into a
//!   `slint::Image` for `Backend.render-scene`; [`canvas_view::CanvasView`]
//!   ports upstream's pan/zoom behavior. Scenes are rebuilt when the
//!   project's revision changes (incremental updates come with M2c).
//!
//! # Running
//!
//! ```text
//! cargo run -p librepcb-app -- [--workspace DIR] [--project FILE.lpp] [FILE.lpp...]
//! ```
//!
//! Without `--workspace`, the workspace is chosen like upstream (see
//! [`startup`]).
//!
//! # Headless screenshots
//!
//! ```text
//! librepcb --screenshot out.png --project X.lpp --tab board [--size 1400x900]
//! ```
//!
//! renders the main window with Slint's software renderer into a PNG
//! without a display ([`screenshot`]). The test `tests/screenshot.rs` does
//! the same with an upstream test project.
//!
//! # Debugging the UI with agents
//!
//! Slint 1.18 has an embedded MCP server which lets agents inspect and
//! drive the running UI (element tree, screenshots, clicks, typing). It is
//! compiled in only on request:
//!
//! ```text
//! SLINT_EMIT_DEBUG_INFO=1 SLINT_MCP_PORT=8765 SLINT_BACKEND=headless \
//!     cargo run -p librepcb-app --features ui-debug-mcp -- --project X.lpp
//! ```
//!
//! - `ui-debug-mcp` enables `slint/mcp` (same as `--features slint/mcp`);
//!   never enable it by default.
//! - `SLINT_EMIT_DEBUG_INFO=1` must be set at build time: it embeds the
//!   element metadata needed for introspection (the UI crate is rebuilt
//!   when it changes).
//! - `SLINT_MCP_PORT` starts the server on `http://127.0.0.1:<port>/mcp`
//!   (convention: 8765 for the Slint UI server, 8766 for LibrePCB's own
//!   MCP server when embedded).
//! - `SLINT_BACKEND=headless` uses a windowless software-rendered backend
//!   (works without a display, e.g. in the cloud sandbox).
//!
//! Then talk JSON-RPC over HTTP: `initialize` (returns usage
//! instructions), `tools/list`, and `tools/call` with tools such as
//! `list_windows`, `get_element_tree`, `find_elements_by_id`,
//! `take_screenshot` (base64 PNG), `click_element`, `drag_element`,
//! `dispatch_pointer_scroll` and `dispatch_key_event`. The scene touch
//! areas have the ids `SchematicTab::ta` and `Board2dTab::ta`.
//! `docs/ui-design.md` ("Debugging the UI with agents") has a complete
//! example session.

pub mod app;
pub mod canvas_view;
pub mod helpers;
pub mod icons;
pub mod libraries;
pub mod models;
pub mod notifications;
pub mod project;
pub mod screenshot;
pub mod section;
pub mod startup;
pub mod tabs;
pub mod theme;
pub mod workspace_models;

pub use app::{App, InitialTab};
pub use librepcb_app_ui as ui;
