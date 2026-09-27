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
  planes, outline, autorouting) used by MCP and later the UI.
- **`crates/librepcb-autoroute`:** built-in grid A* router with rip-up and
  exact clearance verification.
- **`crates/librepcb-scene`:** scene builders for schematics, boards,
  symbols and footprints on `crates/librepcb-canvas`, headless PNG
  rendering, and the graphics export (PDF via `pdf-writer`, SVG,
  PNG/JPEG/BMP; upstream's page layout and realistic board rendering).
- **`crates/librepcb-mcp`:** MCP server (rmcp, stdio/HTTP), see
  `mcp-design.md`.
- **`crates/librepcb-i18n`, `tools/ts2po`, `lang/`:** translations from the
  upstream catalogs.

## Next steps

MCP acceptance (done): an agent-style stdio session designed a
two-transistor LED multivibrator from the official libraries — library
install, part search, schematic with `connect`/`schematic_tidy`, board
outline, connectivity-based placement, Freerouting, GND plane, ERC/DRC,
Gerber export — and the official `librepcb-cli` reports ERC 0, DRC 0.

1. M2 viewer (Slint) on `librepcb-scene`, with live updates from the
   change journal (watch an agent work).
2. MCP: stdio MCP mode for Freerouting, subcircuit templates, design
   intent file (see `mcp-research-konnect.md`), more end-to-end scenarios
   (ICs with multiple gates, multi-page schematics, 4-layer boards).
3. `Board::updateDrcMessageApprovals()`, STEP/3D (needs an OpenCascade
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
