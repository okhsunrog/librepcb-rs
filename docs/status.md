# Status and handoff

State of the port as of 2026-09-27, for continuing the work in a new
session (for example a cloud session). Read `AGENTS.md` first; this file
covers where things stand and how the work has been organized.

## Where we are

Milestone M1 (headless `librepcb-cli`) is done (without 3D/STEP);
M1.5 (MCP) works end to end. About 1150 tests pass.

Done, in `crates/librepcb-core` unless noted:

- **Base:** types, the S-expression format, the serialization traits, and utils.
  Upstream Qt semantics are dropped, with the divergences listed in `COMPAT.md`.
- **Geometry, stroke fonts, polygon clipping:** `crates/clipper` is a
  faithful port of Clipper 1, verified differentially against the C++
  library.
- **Attributes, air wires** (literal port incl. upstream's Delaunay
  library, same tie-breaking as upstream), the math parser (evalexpr),
  and SQLite.
- **File I/O** (transactional file system, directory locks, ZIP, CSV) and
  system info; `crates/librepcb-network` (tokio + reqwest) incl. the
  library download/installer (api.librepcb.org).
- **Workspace** (`workspace/`): workspace open/create, settings, the
  library index (SQLite, schema identical to upstream `cache_v8.sqlite`),
  scanner and search facade.
- **Libraries:** all element types and their checks, identical to upstream
  `librepcb-cli --check`.
- **Project model** (`project/`): circuit, schematics and boards with
  mutations, inverses, the reverse index, the change journal and serde.
  All test projects round-trip byte for byte.
- **Checks:** ERC and DRC; messages and approvals match `librepcb-cli`
  on all upstream test projects (DRC: 37/37 boards).
- **Exports:** plane fragments, Gerber/Excellon, pick & place, Gerber X3,
  IPC-D-356A, BOM, project JSON, interactive HTML BOM (`interactive-html-bom`
  crate like upstream); about 1000 files compared byte for byte with
  `librepcb-cli`. Output jobs (`job`) and the output job runner.
- **File format migrations** v0.1 → v1 → v2, byte-identical incl. `jobs.lp`.
- **`crates/librepcb-cli`:** port of the upstream CLI. Upstream
  `tests/cli`: 162 passed, 1 failed (`--version`), 15 skipped (STEP,
  incl. the 6 `--run-jobs` cases whose job lists contain a STEP job: 3D
  jobs fail like an upstream build without OpenCascade).
- **`crates/librepcb-editor`:** undo stack and intent-level commands
  (components, wiring with forced net names, devices, traces, vias,
  planes, outline, autorouting) used by MCP and later the UI. The
  schematic and board editor FSMs (`fsm::schematic`, `fsm::board`) cover
  all upstream tools and actions (M3d done in the editor crate: buses,
  images, polygon vertex editing, standalone board pads, DXF import,
  "find", change device, segment simplification after edits, the tool
  bar's value attribute, pasting graphics from the library editors,
  cross-probing outputs; headless scenarios in
  `tests/editor/{fsm_schematic,board_fsm}_test.rs`); the application tabs
  expose them (M3d wiring done). M4b
  groundwork: `library_editor` (element editor with snapshot undo stack,
  dirty state, interface check, save; commands for symbols, packages,
  components and devices; check fixes) and `fsm::library` (symbol and
  package editor FSMs with all upstream tools except images/DXF),
  headless scenarios in `tests/editor/library_editor_test.rs`; the Slint
  element tabs are wired (see `librepcb-app` below).
- **`crates/librepcb-autoroute`:** built-in grid A* router with rip-up and
  exact clearance verification.
- **`crates/librepcb-scene`:** scene builders for schematics, boards,
  symbols and footprints on `crates/librepcb-canvas`, headless PNG
  rendering, and the graphics export (PDF via `pdf-writer`, SVG,
  PNG/JPEG/BMP; upstream's page layout and realistic board rendering).
  Schematic and board scenes are updated incrementally from the change
  journal (`apply_changes()`, `SceneSync`), checked against fresh builds
  after editor command sequences and seeded random edits.
- **`crates/librepcb-mcp`:** MCP server (rmcp, stdio/HTTP), see
  `mcp-design.md`.
- **`crates/librepcb-i18n`, `tools/ts2po`, `lang/`:** translations from the
  upstream catalogs.
- **`crates/librepcb-app`** (binary `librepcb`, M2a + most of M2b):
  upstream's Slint UI (`ui/`, compiled by the `librepcb-app-ui` crate with
  Slint 1.18.1, see `ui/PROVENANCE.md`) with a Rust backend: main window,
  sections and tabs, home tab (folder tree, recent/favorite projects),
  themes, about panel, notifications, translations, documents panel,
  schematic and board 2D tabs rendered through `librepcb-scene` and
  `librepcb-canvas` (pan/zoom like upstream, grid, layers panel, read-only
  select/hover), headless screenshots (`--screenshot`) and Slint's MCP
  server for agents (feature `ui-debug-mcp`, see `docs/ui-design.md`).
  M2c: embedded LibrePCB MCP server (`--mcp`, status bar toggle) editing
  the open project live with a shared undo stack, the UI following the
  agent; M2d: ERC/DRC rule check panel (automatic ERC, background DRC,
  approvals, zoom to location) and exports/output jobs from the menus.
  M3a (editing in the tabs) is in place: the schematic and board tabs
  drive the `librepcb-editor` FSMs (tools, tool bars, overlays, context
  menus, clipboard with upstream MIME types, cross-probing, undo/redo;
  `crates/librepcb-app/tests/app/editing.rs`). M3b (project dialogs) is
  done: generic Slint form dialogs (`src/dialogs/`,
  `ui/dialogs/formdialog.slint`) for the properties of all schematic and
  board items the FSMs request (symbol, symbol text, net/bus label
  rename, polygon, text, device, via, pad, plane, board polygon, stroke
  text, hole, zone), line width, move/align, board setup, project setup,
  graphics export, output jobs (all job types), BOM review and the
  pick&place generator, and the add component dialog
  (`ui/dialogs/addcomponentdialog.slint`: categories, search, symbol and
  footprint previews, symbol variant, device); each applies one undo
  group. Tests: `tests/app/{dialogs,setup_dialogs,output_dialogs}.rs`
  (headless) and `tests/app/dialog_screenshots.rs` (the dialogs in the
  headless application, screenshots in `$CARGO_TARGET_TMPDIR`).
  M3d in the tabs: bus and image tools, board pad tools, DXF import,
  "find", plane visibility (`tests/app/m3d_tools.rs`). M4a (library
  management) and M4b (element editors) are done: libraries panel with
  online libraries and installer, create/download library tabs, library
  tab, symbol, package, component, device, category and organization
  tabs with the library FSMs, list panels, properties and chooser
  dialogs, checks with approvals and autofixes, undo/redo, wizard mode
  for new elements and saving into the library
  (`tests/app/{library_management,library_elements}.rs`); see
  `docs/ui-design.md` (M4) and COMPAT.md "Library management" /
  "Library element editors" for the gaps.

## Next steps

MCP acceptance (done): an agent-style stdio session designed a
two-transistor LED multivibrator from the official libraries — library
install, part search, schematic with `connect`/`schematic_tidy`, board
outline, connectivity-based placement, Freerouting, GND plane, ERC/DRC,
Gerber export — and the official `librepcb-cli` reports ERC 0, DRC 0.

1. M4c: EAGLE/KiCad library import wizards (the `librepcb-import` crate
   exists on the coordinating branch), then M3c (project lifecycle).
2. M2 viewer: finish M2b (library info on the home panel, grid/unit and
   layer visibility persisted in the user settings, keyboard handling) and
   the rest of M2d (rule check location markers and autofixes, print and
   image export, opening `*.lppz`); M2c is done.
3. MCP: stdio MCP mode for Freerouting, subcircuit templates, design
   intent file (see `mcp-research-konnect.md`), more end-to-end scenarios
   (ICs with multiple gates, multi-page schematics, 4-layer boards).
4. `Board::updateDrcMessageApprovals()`, STEP/3D (needs an OpenCascade
   replacement), Eagle/KiCad importers (M5).

## How the work is organized

- Work goes in **waves** of parallel agents, each in its own git worktree
  (`../librepcb-rs.w<N>-<name>`, branch `wave<N>/<name>`) with a clearly
  separated file ownership. The coordinating session reviews each branch
  (fmt, clippy, tests, no hardcoded paths), merges it into `master` and
  pushes.
- Related small tasks are batched per agent (about 5–10 agents per wave),
  rather than one agent per tiny task.
- Architecture and design work (design documents) runs on the strongest
  available model, and the design is reviewed with the user before
  implementation starts.
- Acceptance is always mechanical where possible: upstream unit test
  vectors, byte-identical round trips over upstream `tests/data`, and diffs
  against the official `librepcb-cli`.
- Each intentional divergence from upstream goes into `COMPAT.md`.

## Environment

Run `scripts/cloud-setup.sh` on a fresh machine. It sets up the pinned
upstream checkout next to the repository and the official `librepcb-cli`
behind Xvfb, and Freerouting 2.4.1 with Java 25; after that,
`cargo test --workspace` runs everything, including the upstream
comparisons. Set `USER` if the environment lacks it (a ported system info
test needs it), and `LIBREPCB_TEST_LIBRARIES_DIR` to a directory with
official `*.lplib` clones to run the real-library design tests. With
several parallel agents, use `CARGO_INCREMENTAL=0` and shared target
directories: the cloud disk allowance fills up quickly.

To keep the target directory small, the dev profile builds workspace crates
with line tables only (backtraces keep file and line) and dependencies
without debug info, and each crate has one integration test binary (see
AGENTS.md). For full debug info in a local debugging session, build with
`CARGO_PROFILE_DEV_DEBUG=full` (workspace crates), plus
`--config 'profile.dev.package."*".debug="full"'` for the dependencies.
Measured with `cargo clippy --workspace --all-targets` followed by
`cargo test --workspace` in a fresh target directory: 25 GB before (49
executables, 20 GB), 16 GB with one test binary per crate, 5.7 GB with the
debug info settings too (26 executables, 2.7 GB). `split-debuginfo =
"unpacked"` saved only another 0.4 GB and is not supported on all
platforms, so it is not set.

## Open items to remember

- Adding and removing project library elements are `Project` methods, not
  `Mutation`s; `librepcb-editor`'s undo stack makes them undoable.
- `Board::updateDrcMessageApprovals()` (obsolete approval cleanup) is not
  ported.
- Direct ZIP downloads from codeload.github.com are blocked in the cloud
  sandbox; official libraries can be cloned with git instead.
- Upstream bugs found along the way, which could be reported upstream:
  Qt/Transifex plural form order mismatches in several languages, `"1e-3"`
  rejected as a length, and a v0.1 circular board outline that cannot be
  migrated.
