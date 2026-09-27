# MCP server design

Design of `crates/librepcb-mcp` (milestone M1.5), building on the decisions
in `mcp-research-konnect.md`. Goal: an AI agent designs a simple board end to
end through MCP alone — find parts, draw the schematic, place and route the
board, run ERC/DRC, export manufacturing data — and the result opens in
upstream LibrePCB.

## Layers

```
librepcb-mcp      MCP protocol (rmcp, stdio + optional streamable HTTP), tool
                  DTOs (serde + schemars), outcome envelope, session state
librepcb-editor   UndoStack + intent-level commands over Mutation (no MCP types)
librepcb-core     model, Mutation/apply, ERC/DRC, exports, jobs, workspace/library DB
librepcb-network  library download (api.librepcb.org)
```

`librepcb-mcp` owns no domain logic: every tool is a thin adapter that
parses the DTO, calls one editor command or core query, and builds the
response. Logic an agent needs that a UI would also need (auto-placement of
wires, device placement, routing helpers) lives in `librepcb-editor`.

## Process and state

- Binary `librepcb-mcp` (stdio by default, `--http <addr>` for streamable
  HTTP). One server process holds at most one workspace and one open
  project: `Session { workspace: Option<Workspace>, project: Option<OpenProject> }`
  behind `Arc<parking_lot::Mutex<_>>`. Tools run their synchronous core work
  in `tokio::task::spawn_blocking`; the lock is never held across `.await`.
- `OpenProject { editor: ProjectEditor, file_system (lock), saved_revision,
  upgraded }`. The `ProjectEditor` (editor crate) owns the project, its
  undo stack and the library element source (the workspace library
  database, shared via `Workspace::shared_library_db()`). Unsaved changes
  are the undo stack's clean state (plus a pending file format upgrade).
  Derived data (plane fragments, air wires) is updated outside the undo
  history through `ProjectEditor::update_derived_data()` (DRC, exports,
  output jobs, `planes_rebuild`). The upstream-compatible directory lock is
  taken on open, so upstream LibrePCB refuses to open the project
  concurrently (and vice versa).
- The workspace is optional: without one, library tools operate on the
  project library only and on library directories passed explicitly.
  `--workspace <dir>` or `workspace_open` selects one; `workspace_create`
  makes a fresh one (used by tests and first-time setup).

## Tool conventions

- Tools address entities by what the agent sees: designators (`"R1"`),
  pins as `"R1.1"` / `"U1.VCC"` (component designator + signal name, or pad
  name on the board) or a designator alone for single-pin components
  (`"GND1"`), net names, schematic/board names; UUIDs are accepted
  everywhere as an alternative.
- Arguments are strict: every DTO denies unknown fields, and the server
  checks the argument names of each call against the tool's input schema
  before dispatch, so a misspelled argument (e.g. `path` instead of
  `directory`) fails with `invalid_argument` naming the key and listing
  the valid ones instead of being ignored. Deserialization errors (wrong
  types, unknown nested keys) are `invalid_argument` tool errors too.
- Units: millimeters as JSON numbers in tool arguments and responses
  (converted exactly to nanometers with `Length::from_mm`), angles in
  degrees. The `apply_mutation` escape hatch uses the core serde
  representation (integer nanometers).
- Write tools take an optional `expected_revision`; a mismatch fails with
  kind `stale_revision` without changing anything. Every write is one undo
  group labeled `"AI: <tool>"` and returns `revision`, the created/affected
  entities re-read from the model, and the `Change` events.
- Response envelope (`structuredContent` + a short text summary):
  `{ "outcome": "complete"|"partial"|"failed", "revision": n, "result": {...},
  "warnings": [...] }`. Errors map `project::Error` & co. to stable kinds
  (`not_found`, `invalid_argument`, `conflict`, `stale_revision`,
  `no_project`, `io`, `internal`) plus the upstream message.
- Checks never read as passing when they did not run: `run_drc` returns
  `ran: true` and the message list, or an error.

## Tools (first version, < 60)

Session / project:
`server_info`, `workspace_open`, `workspace_create`, `project_create`,
`project_open`, `project_save`, `project_close`, `project_summary`
(metadata, pages, boards, counts, revision, unsaved changes), `undo`, `redo`,
`history`.

Library:
`library_search` (query, kind: component|device|package|symbol, limit;
upstream whole-string matching first, then — if fewer than `limit` —
elements containing all words of the query in name, keywords,
description or, for devices, component/package name and part numbers:
core facade `LibraryDb::search_all_tokens()`; results carry `match:
whole|all_words`), `library_element` (details: component signals + gates +
symbol variants, device pad→signal map with the pad geometry of the
default footprint + package footprints + pads with position, rotation,
shape, size, corner radius, drill and holes, package footprints),
`library_list` (installed libraries), `library_install` (official remote
libraries by name/uuid, downloads + rescans; if a ZIP download fails, e.g.
blocked by a proxy, and the library list entry has a repository URL and
`git` is available, the repository is cloned shallowly at the commit of
the ZIP URL into a temporary directory, `.git` is removed and the result
renamed into place; the result reports `method: zip|git`),
`library_rescan`.

Circuit & schematic:
`component_add` (component or device uuid, optional designator/value,
schematic page, position, rotation, mirror; places every gate, stacked
automatically if no position), `component_remove`, `component_update`
(designator, value, attributes, assembly options), `component_list`,
`component_get` (signals with nets, symbols and pin positions, device),
`symbol_move`, `connect` (net name + list of pins; creates/merges nets,
draws wires or net labels on the schematic), `disconnect` (pins),
`net_rename`, `net_list` (nets with pins and pads), `netlist`,
`schematic_add`, `schematic_list`, `schematic_get` (all items with
coordinates), `net_class_set`, `schematic_tidy` (re-arrange a page by
connectivity and redraw its nets).

Board:
`board_add`, `board_get` (outline, devices with pads and positions,
traces, vias, planes, unrouted air wires), `board_set_outline` (rectangle
with optional corner radius, or polygon), `device_place` (designator,
position, rotation, side, optional device/footprint choice),
`device_auto_place` (all unplaced devices inside the outline, placed by
connectivity),
`trace_add` (net or pads, layer, width, list of points; endpoints snap to
pads/vias/junctions at the given coordinates), `via_add`, `trace_remove`,
`plane_add` (net, layer, outline or whole board, clearance, connect style),
`design_rules_set`, `planes_rebuild`, `autoroute` (built-in router for
simple boards, see below), `unrouted` (air wires).

Checks and outputs:
`erc_run`, `drc_run`, `export_fabrication` (Gerber + Excellon),
`export_bom`, `export_pick_place`, `export_netlist`, `jobs_list`,
`jobs_run`, `render` (PNG of a schematic page or a board side, as MCP image
content; the target defaults to the board when `board` or `side` is
given).

Escape hatch: `mutation_apply` (core `Mutation` JSON, full validation, one
undo group), `mutation_schema`.

## Routing

An agent cannot route by coordinates reliably. Two paths, both behind
`autoroute`:

1. Built-in grid router: the pure algorithm in `librepcb-autoroute` (A* on
   a per-layer grid with via cost, exact clearance checks, rip-up and
   reroute, air wires routed shortest first) and the editor command
   `commands::Autoroute`, which extracts the problem from the board (DRC
   data: pads, copper, holes, keepouts, DRC settings and design rules),
   routes only unrouted air wires (optionally of selected nets and layers)
   and applies ordinary traces/vias through `AddTrace`/`AddVia` in one undo
   group. Enough for 2-layer boards with a few dozen nets.
   `autoroute` backend `builtin`, used by `auto` when FreeRouting is not
   installed or a nets/layers filter is given.
2. FreeRouting through Specctra DSN export / SES import
   (`librepcb_editor::FreeroutingRouter`: export with a manifest, run the
   jar headless as a subprocess with a timeout, strict session import as
   one undo group). For MCP, export under the read lock, run FreeRouting
   without lock and import under the write lock; the manifest rejects the
   session if the project changed meanwhile. `autoroute` backend
   `freerouting`; `auto` (default) uses it when `FreeroutingConfig::detect()`
   finds it and falls back to `builtin` if it fails. `specctra_export` /
   `specctra_import` expose the files for other routers.

## Agent-oriented editor commands

Logic an agent needs beyond upstream's interactive editing lives in the
editor crate (`commands::wiring`, `commands::placement`), usable by a UI
too:

- `ConnectNet` (tool `connect`): connects component signals to one net
  (forced net names of supply symbols win, other nets are merged; supply
  symbols whose signal is unconnected and whose forced net name equals the
  net name are attached too and reported) and draws a readable schematic:
  unwired supply symbols are moved in front of the nearest pin, pins on a
  page are joined by a minimum spanning tree of wires (up to 25.4 mm, one
  corner, only routes which do not cross symbol bodies or texts, pins,
  junctions, or run along or across other wires), all other pins get a
  stub wire carrying a net label (upstream draws the label text along its
  wire, so the stub is as long as the estimated label text: 0.9 mm per
  character); the stub goes straight out or jogs sideways, whichever first
  keeps the label rectangle and the wire clear of symbols with their texts,
  other labels, wires and pins; a net with several segments gets a label
  on each segment (supply symbols count as label).
- `DisconnectSignals` (tool `disconnect`): removes the wires at the pins;
  the other ends of direct wires keep their net through a stub with label.
- `AutoPlaceSymbols` (`component_add` without position, `symbol_move`
  with `auto`): first free place in rows of about 200 mm (2.54 mm grid)
  without overlapping symbols (with their texts), wires or labels; every
  symbol keeps 12.7 mm of room in the direction of each pin for a stub
  with a net label, so labels of neighbors do not collide.
- `TidySchematic` (tool `schematic_tidy`): removes the wiring of a page
  (nets, net classes and boards stay), places the symbols again by
  connectivity (greedy: the most connected symbol first, then the symbol
  most strongly connected to the placed ones at the free grid position
  minimizing the weighted pin distance to its nets, keeping the pin room;
  net weights 1/(n-1), supply nets less) and draws all nets again with
  `ConnectNet` (shortest nets first, direct wires up to 38.1 mm); supply
  symbols end up next to a pin of their net. Deterministic.
- `AutoPlaceDevices` (tool `device_auto_place`): greedy placement by
  connectivity inside the board outline: the most connected device first
  (near the center), then the device most strongly connected to the
  placed ones at the free position (1 mm steps) minimizing the weighted
  Manhattan length of its air wires to placed pads plus a small pull to
  the center; connectors (designator prefix J, X, P, CN) only at the
  outline edge. The gap between footprints (courtyard/pads/texts) grows
  with the free board area (up to 5 mm, at least `spacing`) so the
  devices spread over the board and leave room for traces; if not all
  devices fit, smaller gaps and finally row packing are tried. Board edge
  clearance of the DRC settings; devices which do not fit are put right
  of the board and reported (`outcome: partial`). Deterministic.

## Resources

The stroke fonts new projects get (`resources/fontobene/*.bene`, like
upstream's `Project::create()` copies them from `share/librepcb`) and the
default font for rendering come from `librepcb_scene::resources`: a
resources directory on disk (`LIBREPCB_SHARE`, else `../share/librepcb`
next to the executable) wins; otherwise the fonts embedded into the
binary at build time from `$LIBREPCB_UPSTREAM_DIR/share/librepcb/fontobene`
(nothing is copied into this repository) are used, so an installed
`librepcb-mcp` works on machines without the upstream checkout.

## Tests

- Unit tests per tool adapter on an in-memory session.
- End-to-end test over stdio (rmcp client): create workspace, install/scan a
  test library, create a project, add components, connect nets, place
  devices, set outline, autoroute, add a ground plane, run ERC/DRC, export,
  save, and open the result with the official `librepcb-cli` (ERC/DRC
  clean, Gerber export works).
