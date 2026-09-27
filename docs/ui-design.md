# UI design (M2 viewer → M3 editor)

Design of the graphical application `crates/librepcb-app` (binary
`librepcb`). Status: agreed with the user (copy and port upstream's `.slint` files;
embedded MCP server in M2).

## Key finding: upstream already has a Slint UI

Upstream LibrePCB 2.x replaced most of its Qt Widgets UI with Slint:
`libs/librepcb/ui/` holds 122 `.slint` files (~34k lines): main window,
window sections and tabs, home tab, schematic and board 2D tabs with their
tool bars, library editors, panels (documents, layers, ERC/DRC, place
devices, order), widgets and themes. The C++ side (`libs/librepcb/editor`)
is a backend behind a narrow API (`ui/api/*.slint`):

- `global Data`: all view state as Slint structs and models (sections →
  tabs → `SchematicTabData`, `Board2dTabData`, ...; projects, libraries,
  notifications, rule check results).
- `global Backend`: callbacks — `trigger(Action)`, `trigger-section`,
  `trigger-tab(section, tab, TabAction)`, and the canvas protocol:
  `render-scene(section, width, height, scene, frame) -> image`,
  `scene-pointer-event`, `scene-scrolled`, `scene-key-pressed/released`.

The canvas protocol is exactly our architecture: the backend renders the
scene into a `slint::Image` (upstream: tiny-skia via `SlintGraphicsView`;
here: `librepcb-canvas` with vello_cpu) and receives pointer/key events.

Slint has first-class Rust support, so the UI port is: **take upstream's
`.slint` files and port the C++ backend to Rust.** The look, the layout
and the translations (the `.slint` strings use `@tr` with upstream
contexts, which the existing `lang/` catalogs already contain) come for
free; the work is the backend.

What upstream still does with Qt Widgets: 72 `.ui` dialogs (property
dialogs, board setup, output jobs, BOM review, ...) and the 3D view. Those
need new Slint dialogs (M3, batched per area) and a new 3D renderer (M5).

## Decisions

1. **UI sources:** copy `libs/librepcb/ui/**` at the pinned upstream
   revision into `crates/librepcb-app/ui/` as ported source (GPL like the
   rest), with a `PROVENANCE.md` naming the revision. Changes are allowed
   (they are our code from then on); keep a diff-friendly layout so later
   upstream UI changes can be merged by hand. Alternative rejected:
   compiling directly from the upstream checkout (the app would depend on
   an external checkout at build time and could not diverge).
2. **Crate layout:**
   - `crates/librepcb-app` (bin `librepcb`): `build.rs` with `slint-build`,
     the backend (`app/`: application, window, section, tab types), tabs
     (`tabs/`: home, schematic, board 2D, library tabs later), panels, and
     glue to the canvas.
   - Scene building stays in `librepcb-scene` (already used by render and
     graphics export); it gains **incremental updates**: apply
     `project::Change` events to a built scene (update/insert/remove the
     items of one symbol, segment, device, ...) instead of rebuilding.
   - Interaction logic (editor state machines: select, draw wire, add
     component, draw trace, ...) is UI-toolkit independent and goes into
     `librepcb-editor` (`fsm/` module, ported from
     `editor/project/{schematic,board}/fsm`) so it can be tested headless.
     The app only translates Slint events to FSM events and FSM outputs
     (overlays, cursor, tool bar data) to `Data`.
3. **State and threading:** one UI thread owns the open projects as
   `librepcb_editor::SharedProject` (`Arc<parking_lot::Mutex<OpenProject>>`,
   an `OpenProject` holds the `ProjectEditor` and the directory lock); the
   embedded MCP server gets the same handles via `McpState::set_project()`. Long jobs
   (DRC, plane fragments, exports, library scan, Freerouting) run on worker
   threads on snapshots (as in core) and post results back with
   `slint::invoke_from_event_loop`. After each command the tab pulls
   `Project::changes_since(cursor)` and updates its scene (or rebuilds on
   `Resync`).
4. **Embedded MCP ("watch the agent"):** the app can start the MCP server
   (`librepcb-mcp` library, streamable HTTP on localhost, on a tokio
   runtime in a background thread) sharing the same `ProjectEditor`
   mutex; every MCP write bumps the revision and wakes the UI, which
   applies the journal to the visible scenes. Agent edits appear live and
   are in the same undo history ("AI: ..." groups). This is the headline
   feature of M2 and needs only a small refactor of `librepcb-mcp`'s
   session to accept an externally owned editor.
5. **Rendering:** vello_cpu into a `SharedPixelBuffer` per canvas (works
   with every Slint renderer, incl. software); GPU vello later. Upstream's
   `frame` counter property triggers repaints.
6. **Testing:** backend logic unit-tested headless; UI tests with Slint's
   testing backend (`slint::testing` / `i-slint-backend-testing`) driving
   the real component tree; screenshot tests with the software renderer
   (`take_snapshot`) compared loosely (hashes are not stable across
   versions). Screenshots are also how agents without a display check
   their work.

## Milestones

**M2 — viewer** (this wave):
- M2a skeleton: crate, copied `.slint` files compiling with slint-build,
  `Data`/`Backend` bound in Rust, main window, home tab (recent projects,
  open/new project), window sections and tabs, notifications, about
  panel, themes, translations via `librepcb-i18n`.
- M2b project viewing: open a project (read-only first), documents panel,
  schematic tab and board 2D tab rendering through `librepcb-scene`,
  navigation (pan/zoom/fit like upstream), grid, layers panel with
  visibility, selection highlight and hover info (read-only select tool).
- M2c live updates: incremental scene updates from the change journal;
  embedded MCP server toggle in the UI; undo/redo/save buttons working
  (edits come from MCP until M3).
- M2d checks and outputs: ERC and DRC panels (run, list, zoom to
  location, approve), export graphics (PDF) and output jobs run from the
  menu.

**M3 — editor:** port the schematic and board editor FSMs (select/move/
rotate/flip/delete, draw wire, add component, net labels, draw trace,
vias, planes, polygons, zones, holes, texts), clipboard, and the property
dialogs as Slint dialogs (batched per area). **M4:** library editors
(their `.slint` files are already there). **M5:** 3D view, importers.

## Risks

- Upstream `.slint` files target a specific Slint version; pin the same
  Slint release (check upstream's `find_package(Slint)` version) and
  upgrade deliberately.
- Some upstream `.slint` code calls C++-only helpers through `Backend`
  pure callbacks (length/angle parsing, shortcuts, ...); each needs a Rust
  implementation — mechanical, but many.
- No display in the cloud sandbox: development relies on the testing
  backend and software-rendered snapshots; interactive checks happen on a
  desktop.
