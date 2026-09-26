//! Conversion of `.ts` messages into `.po` entries.

use std::collections::HashMap;

use crate::languages::PluralRule;
use crate::placeholders::{ArgMap, from_qt_cpp, from_qtized_slint};
use crate::po::PoEntry;
use crate::slint_src::SlintSources;
use crate::ts::{TranslationType, TsMessage};

/// Context prefix `dev/i18n.py` adds to Slint component names.
const SLINT_CONTEXT_PREFIX: &str = "ui::";

/// Counters and diagnostics collected during conversion.
#[derive(Debug, Default)]
pub struct Report {
    /// Slint strings whose msgid was recovered from the `.slint` sources.
    pub slint_recovered: usize,
    /// Slint strings whose msgid had to be reconstructed heuristically.
    pub slint_heuristic: Vec<String>,
    /// Messages dropped because they are vanished/obsolete.
    pub obsolete: usize,
    /// Duplicate (context, msgid) pairs that were merged.
    pub merged_duplicates: usize,
    pub warnings: Vec<String>,
}

/// How placeholders of a message's translations are converted.
enum Placeholders {
    /// Undo `dev/i18n.py`'s Qt-ization (see [`from_qtized_slint`]).
    Slint,
    /// Qt `tr()` string; numbering derived from the source string.
    Cpp(ArgMap),
}

/// Whether a message comes from a `.slint` file (via `dev/i18n.py`).
pub fn is_slint_origin(m: &TsMessage) -> bool {
    m.context.starts_with(SLINT_CONTEXT_PREFIX)
        && m.locations.iter().any(|l| l.filename.ends_with(".slint"))
}

/// Converts all messages of one `.ts` file.
///
/// `rule` is `None` for the source-language template (`.pot`), in which case
/// all `msgstr`s are empty (two forms for plurals, as usual for templates).
pub fn convert(
    messages: &[TsMessage],
    rule: Option<&PluralRule>,
    slint: &mut SlintSources,
    report: &mut Report,
) -> Vec<PoEntry> {
    let mut entries: Vec<PoEntry> = Vec::with_capacity(messages.len());
    let mut index: HashMap<(String, String), usize> = HashMap::new();
    for m in messages {
        if m.translation_type == TranslationType::Obsolete {
            report.obsolete += 1;
            continue;
        }
        let entry = convert_message(m, rule, slint, report);
        match index.get(&(entry.context.clone(), entry.msgid.clone())) {
            Some(&i) => {
                report.merged_duplicates += 1;
                let existing = &mut entries[i];
                let better = !existing.is_translated() && entry.is_translated();
                existing.locations.extend(entry.locations);
                if better {
                    existing.msgstr = entry.msgstr;
                    existing.fuzzy = false;
                }
            }
            None => {
                index.insert((entry.context.clone(), entry.msgid.clone()), entries.len());
                entries.push(entry);
            }
        }
    }
    entries
}

fn convert_message(
    m: &TsMessage,
    rule: Option<&PluralRule>,
    slint: &mut SlintSources,
    report: &mut Report,
) -> PoEntry {
    let slint_origin = is_slint_origin(m);
    let locations = m
        .locations
        .iter()
        .map(|l| match l.line {
            Some(line) => format!("{}:{line}", l.filename),
            None => l.filename.clone(),
        })
        .collect();
    let mut extracted_comments = Vec::new();
    if let Some(c) = m.comment.as_deref().filter(|c| !c.is_empty()) {
        // Qt disambiguation; gettext has no equivalent besides msgctxt. The
        // upstream catalogs have no (context, source) pairs that differ only
        // in the disambiguation, so it is kept as a translator hint only.
        extracted_comments.push(format!("Disambiguation: {c}"));
    }
    if let Some(c) = m.extra_comment.as_deref().filter(|c| !c.is_empty()) {
        extracted_comments.push(c.to_owned());
    }
    let translator_comments = m
        .translator_comment
        .iter()
        .filter(|c| !c.is_empty())
        .cloned()
        .collect();

    // Source strings.
    let (context, msgid, msgid_plural, placeholders) = if slint_origin {
        let context = m.context[SLINT_CONTEXT_PREFIX.len()..].to_owned();
        let slint_files = m
            .locations
            .iter()
            .map(|l| l.filename.as_str())
            .filter(|f| f.ends_with(".slint"));
        let (msgid, plural) = match slint.find(slint_files, &m.source, m.numerus) {
            Some(call) => {
                report.slint_recovered += 1;
                (call.msgid, call.plural)
            }
            None => {
                report
                    .slint_heuristic
                    .push(format!("{}: {:?}", m.context, m.source));
                let s = from_qtized_slint(&m.source);
                // The `.ts` file only has the plural string of plural messages.
                if m.numerus {
                    (String::new(), Some(s))
                } else {
                    (s, None)
                }
            }
        };
        // Upstream writes `@tr("" | "{n} thing(s)" % n)`. An empty msgid
        // cannot be used: several such plurals in one component would collide
        // on (msgctxt, msgid), and English would show "" for n == 1. So the
        // plural string doubles as singular; the Rust port's `.slint` files
        // must use `@tr("{n} thing(s)" | "{n} thing(s)" % n)`.
        let msgid = match &plural {
            Some(p) if msgid.is_empty() => p.clone(),
            _ => msgid,
        };
        (context, msgid, plural, Placeholders::Slint)
    } else {
        let args = ArgMap::from_source(&m.source);
        if args.has_gaps() {
            report.warnings.push(format!(
                "{}: {:?} uses non-contiguous placeholders; renumbered by QString::arg() order",
                m.context, m.source
            ));
        }
        let msgid = from_qt_cpp(&m.source, &args, m.numerus).text;
        let plural = m.numerus.then(|| msgid.clone());
        (m.context.clone(), msgid, plural, Placeholders::Cpp(args))
    };
    let convert_translation = |t: &str, warnings: &mut Vec<String>| match &placeholders {
        Placeholders::Slint => from_qtized_slint(t),
        Placeholders::Cpp(args) => {
            let c = from_qt_cpp(t, args, m.numerus);
            if !c.unknown_args.is_empty() {
                warnings.push(format!(
                    "{}: translation {t:?} uses placeholders {:?} not in the source",
                    m.context, c.unknown_args
                ));
            }
            c.text
        }
    };

    // Translations.
    let raw_forms: Vec<&str> = if m.numerus {
        m.numerus_forms.iter().map(String::as_str).collect()
    } else {
        vec![m.translation.as_str()]
    };
    let has_content = raw_forms.iter().any(|s| !s.is_empty());
    let unfinished = m.translation_type == TranslationType::Unfinished;
    let msgstr: Vec<String> = match (rule, m.numerus) {
        (None, false) => vec![String::new()],
        (None, true) => vec![String::new(), String::new()],
        (Some(rule), numerus) if unfinished && !has_content => {
            vec![
                String::new();
                if numerus {
                    rule.ts_form_for_po_index.len()
                } else {
                    1
                }
            ]
        }
        (Some(_), false) => vec![convert_translation(&m.translation, &mut report.warnings)],
        (Some(rule), true) => {
            if raw_forms.len() != rule.ts_forms && has_content {
                report.warnings.push(format!(
                    "{}: {:?} has {} numerus forms, expected {}",
                    m.context,
                    m.source,
                    raw_forms.len(),
                    rule.ts_forms
                ));
            }
            rule.ts_form_for_po_index
                .iter()
                .map(|&i| {
                    raw_forms
                        .get(i)
                        .map(|s| convert_translation(s, &mut report.warnings))
                        .unwrap_or_default()
                })
                .collect()
        }
    };
    PoEntry {
        context,
        msgid,
        msgid_plural,
        msgstr,
        fuzzy: rule.is_some() && unfinished && has_content,
        locations,
        extracted_comments,
        translator_comments,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::languages::plural_rule;
    use crate::ts::Location;

    fn msg(context: &str, file: &str, source: &str) -> TsMessage {
        TsMessage {
            context: context.into(),
            locations: vec![Location {
                filename: file.into(),
                line: Some(7),
            }],
            source: source.into(),
            ..Default::default()
        }
    }

    fn run(messages: &[TsMessage], lang: &str) -> Vec<PoEntry> {
        let rule = plural_rule(lang);
        convert(
            messages,
            rule.as_ref(),
            &mut SlintSources::new(None),
            &mut Report::default(),
        )
    }

    #[test]
    fn cpp_message() {
        let mut m = msg("AttributeKey", "a.cpp", "Invalid key: '%1' {x}");
        m.translation = "Ungültig: '%1' {x}".into();
        m.comment = Some("dis".into());
        let e = &run(&[m], "de")[0];
        assert_eq!(e.context, "AttributeKey");
        assert_eq!(e.msgid, "Invalid key: '{0}' {{x}}");
        assert_eq!(e.msgstr, vec!["Ungültig: '{0}' {{x}}".to_owned()]);
        assert_eq!(e.locations, vec!["a.cpp:7".to_owned()]);
        assert_eq!(e.extracted_comments, vec!["Disambiguation: dis".to_owned()]);
        assert!(!e.fuzzy);
    }

    #[test]
    fn slint_message_heuristic() {
        let mut m = msg("ui::MainMenuBar", "libs/ui/menu.slint", "Undo: %1 100%%");
        m.translation = "Rückgängig: %1 100%%".into();
        let e = &run(&[m], "de")[0];
        assert_eq!(e.context, "MainMenuBar");
        assert_eq!(e.msgid, "Undo: {0} 100%");
        assert_eq!(e.msgstr, vec!["Rückgängig: {0} 100%".to_owned()]);
    }

    #[test]
    fn ui_context_from_cpp_is_not_stripped() {
        let e = &run(&[msg("ui::MainMenuBar", "tests/x.cpp", "File")], "de")[0];
        assert_eq!(e.context, "ui::MainMenuBar");
    }

    #[test]
    fn unfinished_handling() {
        let mut empty = msg("C", "a.cpp", "A");
        empty.translation_type = TranslationType::Unfinished;
        let mut partial = msg("C", "a.cpp", "B");
        partial.translation_type = TranslationType::Unfinished;
        partial.translation = "Bee".into();
        let mut gone = msg("C", "a.cpp", "C");
        gone.translation_type = TranslationType::Obsolete;
        gone.translation = "Zeh".into();
        let entries = run(&[empty, partial, gone], "de");
        assert_eq!(entries.len(), 2);
        assert!(!entries[0].fuzzy);
        assert_eq!(entries[0].msgstr, vec![String::new()]);
        assert!(entries[1].fuzzy);
        assert_eq!(entries[1].msgstr, vec!["Bee".to_owned()]);
    }

    #[test]
    fn numerus_czech_skips_many() {
        let mut m = msg("C", "a.cpp", "%n file(s), %1");
        m.numerus = true;
        m.numerus_forms = vec![
            "%n soubor %1".into(),
            "%n soubory %1".into(),
            "MANY".into(),
            "%n souborů %1".into(),
        ];
        let e = &run(&[m], "cs")[0];
        assert_eq!(e.msgid, "{n} file(s), {0}");
        assert_eq!(e.msgid_plural.as_deref(), Some("{n} file(s), {0}"));
        assert_eq!(
            e.msgstr,
            vec!["{n} soubor {0}", "{n} soubory {0}", "{n} souborů {0}"]
        );
    }

    #[test]
    fn numerus_russian_skips_fraction_form() {
        let mut m = msg("C", "a.cpp", "%n item(s)");
        m.numerus = true;
        m.numerus_forms = vec!["one".into(), "few".into(), "many".into(), "other".into()];
        assert_eq!(run(&[m], "ru")[0].msgstr, vec!["one", "few", "many"]);
    }

    #[test]
    fn slint_plural_uses_plural_as_msgid() {
        let mut m = msg("ui::OrderPanel", "ui/order.slint", "%n Message(s)");
        m.numerus = true;
        m.numerus_forms = vec!["1 Meldung".into(), "%n Meldungen".into()];
        let e = &run(&[m], "de")[0];
        assert_eq!(e.context, "OrderPanel");
        assert_eq!(e.msgid, "{n} Message(s)");
        assert_eq!(e.msgid_plural.as_deref(), Some("{n} Message(s)"));
        assert_eq!(e.msgstr, vec!["1 Meldung", "{n} Meldungen"]);
    }

    #[test]
    fn template_has_empty_msgstr() {
        let mut m = msg("C", "a.cpp", "%n x");
        m.numerus = true;
        let entries = convert(
            &[m],
            None,
            &mut SlintSources::new(None),
            &mut Report::default(),
        );
        assert_eq!(entries[0].msgstr, vec![String::new(), String::new()]);
        assert!(!entries[0].fuzzy);
    }

    #[test]
    fn duplicates_are_merged() {
        let a = msg("C", "a.cpp", "Same");
        let mut b = msg("C", "b.cpp", "Same");
        b.translation = "Gleich".into();
        let entries = run(&[a, b], "de");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].locations, vec!["a.cpp:7", "b.cpp:7"]);
        assert_eq!(entries[0].msgstr, vec!["Gleich"]);
    }
}
