//! Writer for gettext `.po`/`.pot` files.

use std::fmt::Write as _;

use crate::placeholders::po_escape;

/// One catalog entry.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PoEntry {
    pub context: String,
    pub msgid: String,
    pub msgid_plural: Option<String>,
    /// One string for singular entries, `nplurals` strings for plural ones.
    pub msgstr: Vec<String>,
    pub fuzzy: bool,
    /// `file:line` references.
    pub locations: Vec<String>,
    /// Comments for translators (`#.`).
    pub extracted_comments: Vec<String>,
    /// Translator comments (`# `).
    pub translator_comments: Vec<String>,
}

impl PoEntry {
    /// Whether the entry has a complete, non-fuzzy translation.
    pub fn is_translated(&self) -> bool {
        !self.fuzzy && !self.msgstr.is_empty() && self.msgstr.iter().all(|s| !s.is_empty())
    }
}

/// Writes `keyword "value"`, splitting after embedded newlines like gettext.
fn write_string(out: &mut String, keyword: &str, value: &str) {
    let lines: Vec<&str> = value.split_inclusive('\n').collect();
    if lines.len() <= 1 {
        let _ = writeln!(out, "{keyword} \"{}\"", po_escape(value));
    } else {
        let _ = writeln!(out, "{keyword} \"\"");
        for line in lines {
            let _ = writeln!(out, "\"{}\"", po_escape(line));
        }
    }
}

fn write_comment(out: &mut String, prefix: &str, text: &str) {
    for line in text.lines() {
        let _ = writeln!(out, "{prefix}{line}");
    }
}

/// Serializes a catalog with the given header fields.
pub fn write(header_comment: &str, header: &[(&str, &str)], entries: &[PoEntry]) -> String {
    let mut out = String::new();
    write_comment(&mut out, "# ", header_comment);
    let _ = writeln!(out, "msgid \"\"");
    let _ = writeln!(out, "msgstr \"\"");
    for (key, value) in header {
        let _ = writeln!(out, "\"{}: {}\\n\"", key, po_escape(value));
    }
    for e in entries {
        out.push('\n');
        for c in &e.translator_comments {
            write_comment(&mut out, "# ", c);
        }
        for c in &e.extracted_comments {
            write_comment(&mut out, "#. ", c);
        }
        for l in &e.locations {
            let _ = writeln!(out, "#: {l}");
        }
        if e.fuzzy {
            let _ = writeln!(out, "#, fuzzy");
        }
        write_string(&mut out, "msgctxt", &e.context);
        write_string(&mut out, "msgid", &e.msgid);
        match &e.msgid_plural {
            Some(plural) => {
                write_string(&mut out, "msgid_plural", plural);
                for (i, s) in e.msgstr.iter().enumerate() {
                    write_string(&mut out, &format!("msgstr[{i}]"), s);
                }
            }
            None => write_string(&mut out, "msgstr", e.msgstr.first().map_or("", |s| s)),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_entries() {
        let entries = vec![
            PoEntry {
                context: "Ctx".into(),
                msgid: "Say \"{0}\"".into(),
                msgstr: vec!["Sag \"{0}\"".into()],
                locations: vec!["a.cpp:1".into()],
                extracted_comments: vec!["note".into()],
                ..Default::default()
            },
            PoEntry {
                context: "Ctx".into(),
                msgid: "{n} item(s)".into(),
                msgid_plural: Some("{n} item(s)".into()),
                msgstr: vec!["{n} Element".into(), "a\nb".into()],
                fuzzy: true,
                ..Default::default()
            },
        ];
        let po = write("Header", &[("Language", "de")], &entries);
        assert_eq!(
            po,
            r#"# Header
msgid ""
msgstr ""
"Language: de\n"

#. note
#: a.cpp:1
msgctxt "Ctx"
msgid "Say \"{0}\""
msgstr "Sag \"{0}\""

#, fuzzy
msgctxt "Ctx"
msgid "{n} item(s)"
msgid_plural "{n} item(s)"
msgstr[0] "{n} Element"
msgstr[1] ""
"a\n"
"b"
"#
        );
        assert!(!entries[1].is_translated());
        assert!(entries[0].is_translated());
    }
}
