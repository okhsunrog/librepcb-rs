# Intentional differences to upstream LibrePCB

Behavior which intentionally differs from the C++ implementation (usually
because upstream relies on Qt semantics which are not reproduced). Entries
are grouped by module.

## geometry, font, utils

- **Stroke texts are processed per `char`** (`StrokeFont::stroke_line()`),
  upstream per UTF-16 code unit. Only characters outside the Basic
  Multilingual Plane (e.g. emojis) are affected: they are rendered as one
  replacement glyph (U+FFFD) instead of two, which changes the stroke text
  geometry (and thus exports) of such texts.
- **FontoBene parsing** (`font::fontobene`) uses Rust number parsing and
  `str::trim()` instead of `QString::toDouble()`/`toUShort(16)` and
  `QString::trimmed()`: non-finite numbers (`inf`, `nan`) and `0x` prefixed
  codepoints are rejected, and only Unicode whitespace (not other
  `QChar::isSpace()` characters) is trimmed. Irrelevant for the bundled
  fonts.
- **Fonts which fail to load** behave like an empty font with letter spacing
  0 and line spacing 9 (the FontoBene defaults). Upstream uses a
  default-constructed header whose spacings are uninitialized.
- **`TangentPathJoiner`**: the `timed_out` flag is set whenever the search
  is aborted; upstream misses the flag if the timeout happens in the last
  top-level iteration.
- **`Toolbox::incrementNumberInString()`** wraps on `i32` overflow (like the
  upstream release build of the Rust code, which panics in debug builds).
- Not ported (UI specific, will live in the rendering layer):
  `Path::toQPainterPathPx()`, `Via::toQPainterPathPx()`,
  `PadGeometry::to*QPainterPathPx()`, `Image::tryLoad()`,
  `Toolbox::shapeFromPath()`, `Toolbox::floatToString()`,
  `Toolbox::prettyPrintLocale()`, `Transform::mapPx()`.
