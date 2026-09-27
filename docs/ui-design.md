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
   - `crates/librepcb-app` (bin `librepcb`): the backend (`app.rs`:
     application and main window, `section.rs`, `tabs/`: home, schematic,
     board 2D, library tabs later), models, panels, and glue to the canvas.
     The `.slint` files in `crates/librepcb-app/ui/` are compiled by the
     separate crate `librepcb-app-ui` (`ui/Cargo.toml`, `build.rs` with
     `slint-build`): the generated code is tens of MB of Rust, so keeping
     it apart means backend changes do not recompile it.
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

## Debugging the UI with agents

Two ways to look at and drive the running UI without a display.

**Headless screenshots** (no extra features): the app renders its main
window with Slint's software renderer into a PNG and exits.

```sh
cargo run -p librepcb-app -- --workspace /tmp/ws --project X.lpp \
    --tab board --panel layers --screenshot out.png --size 1400x900
```

`--tab home|schematic|board` and
`--panel home|libraries|documents|layers|about|none`
choose what is shown. The test `crates/librepcb-app/tests/screenshot.rs`
does the same with an upstream test project (copied to a temporary
directory: opening a project locks its directory) and writes
`app_home.png`, `app_board.png` and `app_schematic.png` to
`$CARGO_TARGET_TMPDIR`.

**Slint's embedded MCP server** (Slint 1.18, feature `mcp`): lets an agent
inspect the element tree, take screenshots and send clicks, drags, wheel
and key events to the live application.

```sh
SLINT_EMIT_DEBUG_INFO=1 SLINT_MCP_PORT=8765 SLINT_BACKEND=headless \
    cargo run -p librepcb-app --features ui-debug-mcp -- \
    --workspace /tmp/ws --project X.lpp &
```

- `ui-debug-mcp` is an opt-in crate feature (`= ["slint/mcp"]`); never
  enable `mcp` by default.
- `SLINT_EMIT_DEBUG_INFO=1` is read by `slint-build` at build time (it
  embeds element type names and ids; without it the element tree is empty).
  Changing it rebuilds the `librepcb-app-ui` crate.
- `SLINT_BACKEND=headless` is a windowless software-rendered backend.
- Ports: 8765 for Slint's UI server, 8766 for LibrePCB's own MCP server
  once it is embedded (M2c).

The server speaks JSON-RPC over HTTP at `http://127.0.0.1:8765/mcp`; its
`initialize` result contains usage instructions. A session that zooms,
selects and pans in the schematic tab:

```sh
mcp() { curl -s -X POST http://127.0.0.1:8765/mcp -H 'Content-Type: application/json' \
    -d "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"tools/call\",\"params\":{\"name\":\"$1\",\"arguments\":$2}}"; }
W='"windowHandle":{"index":"1","generation":"1"}'
mcp list_windows '{}'
mcp find_elements_by_id "{$W,\"elementsId\":\"SchematicTab::ta\"}"   # the scene's TouchArea
mcp dispatch_pointer_scroll "{$W,\"position\":{\"x\":420,\"y\":388},\"deltaX\":0,\"deltaY\":120}"
mcp click_element '{"elementHandle":{"index":"2","generation":"1"}}'
mcp drag_element '{"elementHandle":{"index":"2","generation":"1"},"button":"Middle","target":{"x":500,"y":300}}'
mcp take_screenshot "{$W}"   # result: base64 PNG in content[0].data
```

Useful element ids: `SchematicTab::ta` and `Board2dTab::ta` (scene touch
areas), `AppWindow::menubar`, `AppWindow::sidebar`. Everything else is found
with `get_element_tree` / `query_element_descendants` (accessible labels of
upstream's widgets).
