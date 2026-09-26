# Intentional divergences from upstream LibrePCB

Behavior differences of the Rust port compared to the C++ reference
(`../LibrePCB`), including exotic edge cases. File formats are unaffected
unless stated otherwise.

## attribute

- **Numeric attribute values** (`AttributeType::is_value_valid()`): checked
  natively instead of with `QString::toFloat()`; the accepted set is
  identical to upstream (verified against Qt 6.11): `inf` with optional sign
  and `nan` without sign (any case), otherwise a finite number within the
  `f32` range that doesn't underflow to zero; surrounding whitespace allowed.
- **Translated type names** (`AttributeType::name_tr()`): looked up in the
  contexts `librepcb::AttrTypeVoltage` etc., where lupdate extracted them.
  Upstream looks them up in the (empty) context `AttributeType` at runtime,
  so it never showed them translated.
- **`AttributeType::valueFromTr()`**: not ported (system locale dependent,
  no callers).
- **Attribute substitution with a filter**: if the filter changes the length
  of a substituted value, upstream continues with stale positions and
  corrupts the text; the port restarts the search behind the filtered value.
  Results without filter, or with a length-preserving filter, are identical.
- **Whitespace** next to removed variables and around keys uses
  `char::is_whitespace()` instead of `QChar::isSpace()` (same character set
  for all practical purposes).

## algorithm

- **Air wires**: the Delaunay triangulation comes from `spade` and the
  minimum spanning forest from `petgraph`. The total air wire length is the
  same, but among several equally long alternatives (e.g. points on a
  regular grid) a different one may be chosen; upstream's choice depends on
  the unstable `std::sort` anyway. Upstream's workarounds for its
  triangulation library (fallback chain edges, manual triangles for 3
  points) are not needed.
- **Net segment simplification**: junctions are merged in ascending anchor
  ID order; upstream iterates a `QHash` (unspecified, per-process random
  order), so the surviving line IDs may differ when several merges are
  possible. The resulting geometry is the same.

## utils

- **Math parser** (user input of lengths/angles/ratios): evaluated with
  `evalexpr` instead of muparser. Same syntax for numbers (locale decimal
  point, `.` always accepted, thousands separators in groups of 3), `+ - * /
  ^`, unary signs, parentheses, muparser's functions (`sin`, `sqrt`, `min`,
  ..., arguments separated by `;`) and constants (`_pi`, `_e`). Differences:
  `^` is left-associative (`2^3^2` = 64, muparser: 512); comparison, logical
  and ternary operators and `rnd()` are not supported; non-ASCII separators
  work (upstream silently ignored them).
- **Overline markup**: `extract_overlines()` returns byte ranges of the
  output string instead of UTF-16 `(start, length)` pairs.

## sqlite_database

- Uses `rusqlite` (bundled SQLite) instead of the Qt SQL driver; same pragmas
  (`foreign_keys = ON`, WAL journal) and busy timeout (5 s). Queries use
  rusqlite's parameter binding directly.
