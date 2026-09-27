# Status and handoff

State of the port as of 2026-09-27, for continuing the work in a new
session (for example a cloud session). Read `AGENTS.md` first; this file
covers where things stand and how the work has been organized.

## Where we are

Milestone M1 (headless `librepcb-cli`) is done except graphics export;
M1.5 (MCP) is in progress. About 1100 tests pass.

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
  IPC-D-356A, BOM, project JSON; about 1000 files compared byte for byte
  with `librepcb-cli`. Output jobs (`job`) and the output job runner.
- **File format migrations** v0.1 → v1 → v2, byte-identical incl. `jobs.lp`.
- **`crates/librepcb-cli`:** port of the upstream CLI. Upstream
  `tests/cli`: 140 passed, 29 failed (graphics export and `--version`),
  9 skipped (STEP).
- **`crates/librepcb-editor`:** undo stack and intent-level commands
  (components, wiring with forced net names, devices, traces, vias,
  planes, outline, autorouting) used by MCP and later the UI.
- **`crates/librepcb-autoroute`:** built-in grid A* router with rip-up and
  exact clearance verification.
- **`crates/librepcb-scene`:** schematic/board scenes on
  `crates/librepcb-canvas` and headless PNG rendering.
- **`crates/librepcb-mcp`:** MCP server (rmcp, stdio/HTTP), see
  `mcp-design.md`.
- **`crates/librepcb-i18n`, `tools/ts2po`, `lang/`:** translations from the
  upstream catalogs.

## In progress / next steps

1. MCP phase 2 is done: 57 tools (write tools on `librepcb-editor` with
   undo/redo/history, `connect` with automatic schematic wiring,
   automatic symbol/device placement, `autoroute` with Freerouting or the
   built-in router, DRC, output jobs); end-to-end design over stdio with
   the official libraries, verified by `librepcb-cli`
   (`LIBREPCB_TEST_LIBRARIES_DIR`).
2. Graphics export (PDF/SVG/PNG) for the CLI and graphics output jobs
   (`jobs_run` skips graphics, interactive BOM and 3D jobs until then).
3. Then M2 (viewer) per `roadmap.md`.

Deliberately deferred: the interactive HTML BOM, 3D/STEP, and the
Eagle/KiCad importers (M5).

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
behind Xvfb; after that, `cargo test --workspace` runs everything,
including the upstream comparisons.

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
