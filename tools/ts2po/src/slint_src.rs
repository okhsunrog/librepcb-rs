//! Recovery of the exact `@tr()` strings from upstream `.slint` sources.
//!
//! `dev/i18n.py` rewrote Slint strings Qt-style before merging them into the
//! `.ts` files (`{}`/`{0}` → `%1`, `{n}` → `%n`, `%` → `%%`, PO escaping) and
//! only kept the *plural* string of plural messages. Slint's bundled
//! translations look strings up by the literal source text, so we scan the
//! `.slint` files referenced by the `.ts` locations and match their `@tr()`
//! calls against the Qt-ized `.ts` source.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::placeholders::qtize_slint;

/// One `@tr(...)` call in a `.slint` file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrCall {
    pub context: Option<String>,
    pub msgid: String,
    pub plural: Option<String>,
}

/// Lazily scanned `.slint` sources, keyed by repository-relative path.
#[derive(Debug)]
pub struct SlintSources {
    root: Option<PathBuf>,
    files: HashMap<String, Vec<TrCall>>,
}

impl SlintSources {
    /// `root` is the upstream repository root; `None` disables recovery.
    pub fn new(root: Option<PathBuf>) -> Self {
        Self {
            root,
            files: HashMap::new(),
        }
    }

    fn calls(&mut self, file: &str) -> &[TrCall] {
        let root = self.root.as_deref();
        self.files.entry(file.to_owned()).or_insert_with(|| {
            root.and_then(|r| std::fs::read_to_string(Path::new(r).join(file)).ok())
                .map(|s| scan(&s))
                .unwrap_or_default()
        })
    }

    /// Finds the `@tr()` call in any of `files` whose Qt-ized form equals the
    /// `.ts` source string.
    pub fn find<'a>(
        &mut self,
        files: impl IntoIterator<Item = &'a str>,
        ts_source: &str,
        numerus: bool,
    ) -> Option<TrCall> {
        for file in files {
            let found = self.calls(file).iter().find(|c| {
                c.plural.is_some() == numerus
                    && qtize_slint(c.plural.as_deref().unwrap_or(&c.msgid)) == ts_source
            });
            if let Some(call) = found {
                return Some(call.clone());
            }
        }
        None
    }
}

/// Character cursor over Slint source code.
struct Cursor<'a> {
    rest: &'a str,
}

impl<'a> Cursor<'a> {
    fn skip_trivia(&mut self) {
        loop {
            let trimmed = self.rest.trim_start();
            if let Some(r) = trimmed.strip_prefix("//") {
                self.rest = r.find('\n').map_or("", |i| &r[i..]);
            } else if let Some(r) = trimmed.strip_prefix("/*") {
                self.rest = r.find("*/").map_or("", |i| &r[i + 2..]);
            } else {
                self.rest = trimmed;
                return;
            }
        }
    }

    fn eat(&mut self, token: &str) -> bool {
        self.skip_trivia();
        match self.rest.strip_prefix(token) {
            Some(r) => {
                self.rest = r;
                true
            }
            None => false,
        }
    }

    /// Parses a Slint string literal without interpolations (as required by
    /// `@tr`). The cursor is not moved on failure.
    fn string(&mut self) -> Option<String> {
        match self.string_literal()? {
            (s, false) => Some(s),
            (_, true) => None,
        }
    }

    /// Parses a Slint string literal. Returns the unescaped text (with
    /// interpolations removed) and whether it contained `\{...}`
    /// interpolations. The cursor is not moved on failure.
    fn string_literal(&mut self) -> Option<(String, bool)> {
        self.skip_trivia();
        let mut chars = self.rest.strip_prefix('"')?.char_indices();
        let body = &self.rest[1..];
        let mut out = String::new();
        let mut interpolated = false;
        while let Some((i, c)) = chars.next() {
            match c {
                '"' => {
                    self.rest = &body[i + 1..];
                    return Some((out, interpolated));
                }
                '\\' if body[i + 1..].starts_with('{') => {
                    chars.next();
                    interpolated = true;
                    let mut depth = 1;
                    while depth > 0 {
                        match chars.next()?.1 {
                            '{' => depth += 1,
                            '}' => depth -= 1,
                            _ => {}
                        }
                    }
                }
                '\\' => match chars.next()?.1 {
                    'n' => out.push('\n'),
                    't' => out.push('\t'),
                    '"' => out.push('"'),
                    '\\' => out.push('\\'),
                    'u' => {
                        let (_, '{') = chars.next()? else { return None };
                        let mut hex = String::new();
                        loop {
                            match chars.next()?.1 {
                                '}' => break,
                                h => hex.push(h),
                            }
                        }
                        out.push(char::from_u32(u32::from_str_radix(&hex, 16).ok()?)?);
                    }
                    _ => return None,
                },
                c => out.push(c),
            }
        }
        None
    }
}

/// Extracts all `@tr()` calls from Slint source code.
pub fn scan(source: &str) -> Vec<TrCall> {
    let mut calls = Vec::new();
    let mut cursor = Cursor { rest: source };
    // Skip to the next interesting token, stepping over strings and comments.
    while let Some(pos) = cursor.rest.find(['@', '"', '/']) {
        cursor.rest = &cursor.rest[pos..];
        if cursor.rest.starts_with('"') {
            if cursor.string_literal().is_none() {
                cursor.rest = &cursor.rest[1..];
            }
            continue;
        }
        if cursor.rest.starts_with("//") || cursor.rest.starts_with("/*") {
            cursor.skip_trivia();
            continue;
        }
        if let Some(r) = cursor.rest.strip_prefix("@tr") {
            cursor.rest = r;
            if cursor.eat("(")
                && let Some(call) = parse_tr_args(&mut cursor)
            {
                calls.push(call);
            }
            continue;
        }
        cursor.rest = &cursor.rest[1..];
    }
    calls
}

fn parse_tr_args(cursor: &mut Cursor<'_>) -> Option<TrCall> {
    let first = cursor.string()?;
    let (context, msgid) = if cursor.eat("=>") {
        (Some(first), cursor.string()?)
    } else {
        (None, first)
    };
    let plural = if cursor.eat("|") {
        Some(cursor.string()?)
    } else {
        None
    };
    Some(TrCall {
        context,
        msgid,
        plural,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scans_tr_calls() {
        let calls = scan(
            r#"
export component Foo {
    // @tr("commented out")
    /* @tr("block") */
    property <string> a: "not @tr(\"x\")";
    property <string> b: "interpolated \{ foo("\"") } \{ {a: 1}.a } @tr(\"y\")";
    text: @tr("Hello {}", name);
    t2: @tr("Ctx" => "With \"context\"\n\u{41}");
    t3: @tr("" | "{n} item(s)" % count) + ":";
    t4: @tr( "One {n}" | "Many {n}" % n );
}"#,
        );
        assert_eq!(
            calls,
            vec![
                TrCall {
                    context: None,
                    msgid: "Hello {}".into(),
                    plural: None
                },
                TrCall {
                    context: Some("Ctx".into()),
                    msgid: "With \"context\"\nA".into(),
                    plural: None
                },
                TrCall {
                    context: None,
                    msgid: String::new(),
                    plural: Some("{n} item(s)".into())
                },
                TrCall {
                    context: None,
                    msgid: "One {n}".into(),
                    plural: Some("Many {n}".into())
                },
            ]
        );
    }

    #[test]
    fn finds_by_qtized_source() {
        let dir = std::env::temp_dir().join(format!("ts2po-test-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("ui")).unwrap();
        std::fs::write(
            dir.join("ui/a.slint"),
            r#"component A { t: @tr("Open \"{0}\" from {1}", a, b); u: @tr("" | "{n} x" % 3); v: @tr("100% {}", 1); }"#,
        )
        .unwrap();
        let mut sources = SlintSources::new(Some(dir.clone()));
        let found = sources
            .find(["ui/a.slint"], r#"Open \"%1\" from %2"#, false)
            .unwrap();
        assert_eq!(found.msgid, "Open \"{0}\" from {1}");
        let found = sources.find(["ui/a.slint"], "%n x", true).unwrap();
        assert_eq!(found.plural.as_deref(), Some("{n} x"));
        assert_eq!(sources.find(["ui/a.slint"], "%n x", false), None);
        let found = sources.find(["ui/a.slint"], "100%% %1", false).unwrap();
        assert_eq!(found.msgid, "100% {}");
        assert_eq!(sources.find(["ui/missing.slint"], "x", false), None);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
