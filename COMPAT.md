# Intentional divergences from upstream LibrePCB

Behavior which deliberately differs from the C++ implementation, including
exotic edge cases. File formats not listed here are byte-compatible.

## core: fileio

### FilePath

- Paths are stored as UTF-8 `String`; paths which are not valid Unicode are
  rejected (`FilePath::new()` returns `None`). Upstream decodes them lossily
  into a `QString`, i.e. they would not work anyway.
- Ordering is `str` ordering (code points) instead of `QString` ordering
  (UTF-16 code units). Only differs between non-BMP characters and
  U+E000..U+FFFF; nothing on disk depends on it.
- Cleaning uses `std::path` component semantics (via `path-clean`) instead
  of `QDir::cleanPath()`: `..` above the root is dropped (`/../a` → `/a`),
  Windows prefixes (drives, UNC, verbatim) are parsed by `std`.
- `is_located_in_dir()` works for the root directory (upstream appends `/`
  unconditionally, so nothing is ever located in `/`).
- `to_unique()` uses `dunce::canonicalize()` (no `\\?\` prefix on Windows).

### file_utils (upstream FileUtils)

- `write_file()` is atomic like `QSaveFile` (temporary file in the same
  directory, then rename), but the temporary file is named `.XXXXXX.tmp`
  instead of `<name>.XXXXXX`. Permissions of existing files are kept, new
  files get `0666 & ~umask` like upstream.
- `files_in_directory()` name filters use `glob` syntax (`*`, `?`, `[...]`,
  negation with `[!...]`), case-insensitive; Qt wildcards negate with
  `[^...]` as well.
- Listing errors (unreadable subdirectories) are returned as errors instead
  of being silently skipped.
- `temp_dir()` (upstream `Application::getTempDir()`): if
  `$XDG_RUNTIME_DIR` is unset on Unix, falls back to the system temporary
  directory instead of Qt's `/tmp/runtime-$USER`.

### TransactionalFileSystem

- `clean_path()` converts `\` to `/` *before* resolving `..`. Upstream (on
  Unix) resolves `..` first, so `a\..\..\x` passed the sandbox check and was
  resolved outside the root later by `FilePath` — a sandbox breakout. A
  leading `/` is ignored, so `/../x` is rejected (Qt may resolve it to the
  root). Trimming uses `str::trim()` (Unicode White_Space).
- `dirs()` / `files()` return sorted lists (upstream: `QSet` order).
- Entries in the backup/autosave index files are sorted by `str` ordering
  (upstream: UTF-16 order); only differs for non-BMP file names and does not
  affect reading.
- `load_from_zip*()` reads all entries before writing any, so a corrupt
  archive does not leave a partially loaded state.

### DirectoryLock / system_info

The lock file format and status logic are identical. Values which can
differ between platforms/implementations:

- Line 1 (full user name) is informational only: taken from
  `whoami::realname()`. On macOS upstream runs `finger`, on Windows it falls
  back to parsing `net user` output.
- Line 5 (process name) and the stale-lock check: identical on Linux
  (`/proc/<pid>/exe`). On macOS/Windows/BSD the name comes from `sysinfo`
  (executable file name, on Windows without extension) instead of
  `proc_name()` / `QueryFullProcessImageNameW()`; long macOS process names
  may therefore differ from what a C++ instance wrote, which makes a running
  C++ instance look like a stale lock to a Rust instance (and vice versa).
  Solaris/OpenBSD are not supported by `sysinfo` (processes always look
  stopped).
- PIDs `<= 0` or `> i32::MAX` in a lock file are treated as "not running"
  (upstream passes them to `kill()`, where `<= 0` means process groups).

### ZIP

- `ZipArchive::extract_to()` uses `zip` 8.x extraction (rejects unsafe paths,
  handles symlinks safely) instead of `zip` 0.6.6.
- Entries > 4 GiB are written as ZIP64.

### CsvFile

- A record consisting of a single empty value is written as `""` (by the
  `csv` crate) instead of an empty line.
- Trailing whitespace of comment lines is trimmed with
  `char::is_whitespace()` instead of `QChar::isSpace()`.

### OutputDirectoryWriter

- Index lines are sorted by `str` ordering (upstream: UTF-16 order).
- `remove_unknown_files()` removes empty parent directories only inside the
  output directory (upstream `QDir::rmpath()` may also remove empty
  directories above it).

## network (crate librepcb-network)

- Async (tokio/reqwest); progress is a `watch` channel, so intermediate
  progress states may be coalesced (upstream emits every signal).
- The HTTP disk cache is not ported (`setCacheLoadControl()`,
  `setMinimumCacheTime()`, `AlwaysCache`); `ApiEndpoint`'s `forceNoCache`
  parameter is therefore dropped.
- User agent: `LibrePCB/<version> (<os>; <arch>; <locale>)` (upstream:
  `LibrePCB/<version> (<pretty OS name>; <arch>; <locale>) Qt/<version>`).
- Error messages: HTTP errors read
  `Error transferring <url> - server replied: <reason> (<HTTP status>)`;
  upstream appends the `QNetworkReply::NetworkError` number instead of the
  HTTP status. `file://` errors read `Error opening <path>: <OS error>` with
  the Rust I/O error text.
- `NetworkRequest` aborts as soon as the 100 MB limit is exceeded instead of
  downloading everything first.
- `Content-Length` of POST requests is always set automatically.
- `FileDownload`: if moving the extracted directory into place fails, the
  backup is restored and an error is returned (upstream restores the backup
  but reports success). A ZIP discovery callback returning a path outside
  the temporary directory is an error (upstream: assertion).
- `ApiEndpoint`: a library entry with a field of the wrong JSON type (e.g.
  `"recommended": "yes"`) is skipped as a whole; missing and `null` fields
  default to empty values like upstream.
- `OrderPcbApiRequest`: the JSON upload body is compact instead of indented;
  the "project is too large" message formats the size like
  `formatFileSize()` (`"1.50 MB"`) instead of
  `QLocale::formattedDataSize()` (`"2 MiB"`).
