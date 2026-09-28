//! Checks that the translation contexts and source strings of all `tr!` and
//! `trn!` calls in the workspace exist in the catalog template
//! (`lang/librepcb.pot`, extracted from upstream), so that the upstream
//! translations are found at runtime. Contexts are the full upstream Qt
//! contexts, e.g. `librepcb::editor::CmdBoardEdit` for classes in the
//! `librepcb::editor` namespace.
//!
//! The scanner is deliberately simple: it finds `tr!(`/`trn!(` outside of
//! line comments, reads the context (a string literal, or a constant or
//! `let` binding with string literal values declared before the call in the
//! same file, or a constant of the same crate) and the source string (if it
//! is a literal). Strings without upstream counterpart are listed in
//! [`NEW_CONTEXTS`] and [`NEW_STRINGS`].
//!
//! Not scanned: `crates/librepcb-app` (its UI strings are checked there) and
//! this crate (its tests use made-up contexts).

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use crate::compile::catalog_keys;

/// Contexts of Rust-only features without upstream counterpart.
const NEW_CONTEXTS: &[&str] = &[
    // Built-in autorouter (no upstream counterpart).
    "Autorouter",
    // Qt's own command line parser strings (not in LibrePCB's catalogs,
    // see COMPAT.md "CLI").
    "QCommandLineParser",
];

/// Strings without upstream counterpart: `(context, source string)`.
const NEW_STRINGS: &[(&str, &str)] = &[
    // MCP placement and schematic tidy commands.
    ("CmdMoveSelectedSchematicItems", "Move Schematic Items"),
    // Read-only element editors refusing changes.
    (
        "librepcb::editor::LibraryEditorTab",
        "The library element is read-only.",
    ),
];

/// Directories (relative to the workspace root) which are not scanned.
const EXCLUDED: &[&str] = &["crates/librepcb-app", "crates/librepcb-i18n"];

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<_> = entries.flatten().map(|e| e.path()).collect();
    entries.sort();
    for path in entries {
        if path.is_dir() {
            if path.file_name().is_some_and(|n| n == "target") {
                continue;
            }
            rust_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

/// Parses a (possibly raw) string literal starting at `pos`; returns its
/// value and the position behind it.
fn string_literal(src: &str, pos: usize) -> Option<(String, usize)> {
    let rest = &src[pos..];
    if let Some(raw) = rest.strip_prefix('r') {
        let hashes = raw.len() - raw.trim_start_matches('#').len();
        let body = raw[hashes..].strip_prefix('"')?;
        let end_marker = format!("\"{}", "#".repeat(hashes));
        let end = body.find(&end_marker)?;
        let consumed = 1 + hashes + 1 + end + end_marker.len();
        return Some((body[..end].to_owned(), pos + consumed));
    }
    let body = rest.strip_prefix('"')?;
    let mut out = String::new();
    let mut chars = body.char_indices();
    while let Some((i, c)) = chars.next() {
        match c {
            '"' => return Some((out, pos + 1 + i + 1)),
            '\\' => match chars.next()?.1 {
                'n' => out.push('\n'),
                't' => out.push('\t'),
                'r' => out.push('\r'),
                '0' => out.push('\0'),
                '\\' => out.push('\\'),
                '"' => out.push('"'),
                '\'' => out.push('\''),
                'u' => {
                    let mut hex = String::new();
                    for (_, c) in chars.by_ref() {
                        match c {
                            '{' => {}
                            '}' => break,
                            c => hex.push(c),
                        }
                    }
                    out.push(char::from_u32(u32::from_str_radix(&hex, 16).ok()?)?);
                }
                '\n' => {
                    // Line continuation: skip the leading whitespace of the
                    // next line.
                    let skip = chars.clone().take_while(|(_, c)| c.is_whitespace()).count();
                    for _ in 0..skip {
                        chars.next();
                    }
                }
                _ => return None,
            },
            c => out.push(c),
        }
    }
    None
}

fn skip_whitespace(src: &str, pos: usize) -> usize {
    pos + (src[pos..].len() - src[pos..].trim_start().len())
}

/// Parses an identifier or path (`a::B`) at `pos`.
fn identifier(src: &str, pos: usize, path: bool) -> Option<(&str, usize)> {
    let len = src[pos..]
        .find(|c: char| !(c.is_alphanumeric() || c == '_' || (path && c == ':')))
        .unwrap_or(src.len() - pos);
    (len > 0).then(|| (&src[pos..pos + len], pos + len))
}

/// Returns the string literals of the initializer of `const NAME: &str =`
/// or `let NAME =` declared last before `before` (`None` if there is no
/// such declaration).
fn local_binding(src: &str, name: &str, before: usize) -> Option<Vec<String>> {
    let mut found = None;
    for prefix in ["const ", "let "] {
        let pattern = format!("{prefix}{name}");
        let mut start = 0;
        while let Some(i) = src[start..before].find(&pattern) {
            let at = start + i;
            start = at + pattern.len();
            let after = &src[start..];
            let next = after.chars().next();
            if next.is_some_and(|c| c.is_alphanumeric() || c == '_') {
                continue;
            }
            if at > 0 && src[..at].ends_with(|c: char| c.is_alphanumeric() || c == '_') {
                continue;
            }
            let Some(eq) = after.find('=') else { continue };
            let Some(semi) = after.find(';') else {
                continue;
            };
            if semi < eq {
                continue;
            }
            if found.is_none_or(|(f, _)| f < at) {
                found = Some((at, (start + eq + 1, start + semi)));
            }
        }
    }
    let (_, (init_start, init_end)) = found?;
    let mut values = Vec::new();
    let mut pos = init_start;
    while let Some(i) = src[pos..init_end].find('"') {
        let (value, end) = string_literal(src, pos + i)?;
        values.push(value);
        pos = end;
    }
    (!values.is_empty()).then_some(values)
}

/// A `tr!`/`trn!` call found by the scanner.
struct Call {
    location: String,
    /// Possible contexts, or the unresolved expression.
    contexts: Result<Vec<String>, String>,
    msgid: Option<String>,
}

fn scan_file(path: &Path, crate_consts: &[(String, String)], calls: &mut Vec<Call>) {
    let src = std::fs::read_to_string(path).expect("read source file");
    let display = path
        .strip_prefix(workspace_root())
        .unwrap_or(path)
        .display()
        .to_string();
    for macro_name in ["tr!(", "trn!("] {
        let mut start = 0;
        while let Some(i) = src[start..].find(macro_name) {
            let at = start + i;
            start = at + macro_name.len();
            if src[..at].ends_with(|c: char| c.is_alphanumeric() || c == '_') {
                continue;
            }
            let line_start = src[..at].rfind('\n').map_or(0, |i| i + 1);
            if src[line_start..at].contains("//") {
                continue;
            }
            let line = src[..at].matches('\n').count() + 1;
            let location = format!("{display}:{line}");
            let pos = skip_whitespace(&src, start);
            let (contexts, pos) = if let Some((ctx, end)) = string_literal(&src, pos) {
                (Ok(vec![ctx]), end)
            } else if let Some((ident, end)) = identifier(&src, pos, true) {
                let name = ident.rsplit("::").next().unwrap_or(ident);
                let values = local_binding(&src, name, at).or_else(|| {
                    let values: Vec<_> = crate_consts
                        .iter()
                        .filter(|(n, _)| n == name)
                        .map(|(_, v)| v.clone())
                        .collect();
                    (!values.is_empty()).then_some(values)
                });
                (values.ok_or_else(|| ident.to_owned()), end)
            } else {
                let expr: String = src[pos..].chars().take(20).collect();
                (Err(expr), pos)
            };
            let pos = skip_whitespace(&src, pos);
            let msgid = src[pos..].strip_prefix(',').and_then(|_| {
                let pos = skip_whitespace(&src, pos + 1);
                string_literal(&src, pos).map(|(s, _)| s)
            });
            calls.push(Call {
                location,
                contexts,
                msgid,
            });
        }
    }
}

/// Returns `(name, value)` of all `const NAME: &str = "value";` of a crate.
fn crate_consts(files: &[PathBuf]) -> Vec<(String, String)> {
    let mut consts = Vec::new();
    for file in files {
        let src = std::fs::read_to_string(file).expect("read source file");
        let mut start = 0;
        while let Some(i) = src[start..].find("const ") {
            let pos = start + i + "const ".len();
            start = pos;
            let Some((name, end)) = identifier(&src, pos, false) else {
                continue;
            };
            let rest = src[end..].trim_start();
            let Some(rest) = rest
                .strip_prefix(": &str")
                .or_else(|| rest.strip_prefix(": &'static str"))
            else {
                continue;
            };
            let Some(rest) = rest.trim_start().strip_prefix('=') else {
                continue;
            };
            let literal_pos = src.len() - rest.trim_start().len();
            if let Some((value, _)) = string_literal(&src, literal_pos) {
                consts.push((name.to_owned(), value));
            }
        }
    }
    consts
}

#[test]
fn tr_contexts_and_strings_exist_in_catalog() {
    let root = workspace_root();
    let pot = std::fs::read_to_string(root.join("lang/librepcb.pot")).expect("read template");
    let keys = catalog_keys(&pot).expect("parse template");
    let contexts: BTreeSet<&str> = keys.iter().map(|(c, _)| c.as_str()).collect();

    let mut calls = Vec::new();
    let mut crate_dirs: Vec<_> = std::fs::read_dir(root.join("crates"))
        .expect("read crates")
        .flatten()
        .map(|e| e.path())
        .collect();
    crate_dirs.push(root.join("tools/ts2po"));
    crate_dirs.sort();
    for dir in crate_dirs {
        let relative = dir.strip_prefix(&root).unwrap_or(&dir);
        if EXCLUDED.iter().any(|e| relative == Path::new(e)) {
            continue;
        }
        let mut files = Vec::new();
        rust_files(&dir, &mut files);
        let consts = crate_consts(&files);
        for file in &files {
            scan_file(file, &consts, &mut calls);
        }
    }
    assert!(
        calls.len() > 500,
        "scanner found only {} calls",
        calls.len()
    );

    let mut errors = Vec::new();
    for call in &calls {
        let call_contexts = match &call.contexts {
            Ok(c) => c,
            Err(expr) => {
                errors.push(format!("{}: unresolved context `{expr}`", call.location));
                continue;
            }
        };
        for ctx in call_contexts {
            if NEW_CONTEXTS.contains(&ctx.as_str()) {
                continue;
            }
            match &call.msgid {
                Some(msgid) => {
                    let known = keys.contains(&(ctx.clone(), msgid.clone()))
                        || NEW_STRINGS.contains(&(ctx.as_str(), msgid.as_str()));
                    if !known {
                        errors.push(format!(
                            "{}: ({ctx:?}, {msgid:?}) not in the catalog",
                            call.location
                        ));
                    }
                }
                None if !contexts.contains(ctx.as_str()) => {
                    errors.push(format!("{}: unknown context {ctx:?}", call.location))
                }
                None => {}
            }
        }
    }
    assert!(
        errors.is_empty(),
        "{} translation lookups would fail (use the full upstream context, \
         e.g. \"librepcb::editor::CmdBoardEdit\", or add genuinely new strings \
         to NEW_STRINGS):\n{}",
        errors.len(),
        errors.join("\n")
    );
}

#[test]
fn string_literals() {
    let src = r##"x "a\"b\n\u{e9}" r#"raw "q""# "line \
                 continued""##;
    assert_eq!(
        string_literal(src, 2),
        Some(("a\"b\n\u{e9}".to_owned(), 16))
    );
    assert_eq!(
        string_literal(src, src.find("r#").unwrap()).map(|(s, _)| s),
        Some("raw \"q\"".to_owned())
    );
    let pos = src.rfind("\"line").unwrap();
    assert_eq!(
        string_literal(src, pos).map(|(s, _)| s),
        Some("line continued".to_owned())
    );
}

#[test]
fn local_bindings() {
    let src = "const CTX: &str = \"A\";\nfn f() { let ctx = if x { \"B\" } else { \"C\" }; tr!(ctx, \"m\") }";
    let at = src.find("tr!").unwrap();
    assert_eq!(local_binding(src, "CTX", at), Some(vec!["A".to_owned()]));
    assert_eq!(
        local_binding(src, "ctx", at),
        Some(vec!["B".to_owned(), "C".to_owned()])
    );
    assert_eq!(local_binding(src, "other", at), None);
}
