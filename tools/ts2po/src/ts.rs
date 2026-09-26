//! Parser for Qt Linguist `.ts` files (the subset produced by `lupdate` and
//! Transifex).

use anyhow::{Context as _, Result, bail};
use quick_xml::XmlVersion;
use quick_xml::escape::resolve_predefined_entity;
use quick_xml::events::{BytesStart, Event};
use quick_xml::reader::Reader;

/// A parsed `.ts` file.
#[derive(Debug, Default)]
pub struct TsFile {
    /// Value of the `language` attribute of the `<TS>` element.
    pub language: String,
    /// Value of the `sourcelanguage` attribute of the `<TS>` element.
    pub source_language: String,
    /// All messages in file order, each with its context name.
    pub messages: Vec<TsMessage>,
}

/// State of a `<translation>` element (`type` attribute).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TranslationType {
    #[default]
    Finished,
    Unfinished,
    /// `vanished` or `obsolete`: no longer present in the sources.
    Obsolete,
}

/// A source location of a message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Location {
    pub filename: String,
    pub line: Option<u32>,
}

/// One `<message>` element.
#[derive(Debug, Default, Clone)]
pub struct TsMessage {
    pub context: String,
    pub numerus: bool,
    pub locations: Vec<Location>,
    pub source: String,
    /// Disambiguation (`<comment>`), part of the Qt lookup key.
    pub comment: Option<String>,
    /// Developer comment for translators (`<extracomment>`).
    pub extra_comment: Option<String>,
    /// Translator comment (`<translatorcomment>`).
    pub translator_comment: Option<String>,
    pub translation_type: TranslationType,
    /// Singular translation (non-numerus messages).
    pub translation: String,
    /// Numerus forms in file order (numerus messages).
    pub numerus_forms: Vec<String>,
}

/// Which text-bearing element we are currently collecting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Field {
    ContextName,
    Source,
    Comment,
    ExtraComment,
    TranslatorComment,
    Translation,
    NumerusForm,
    /// `<lengthvariant>` inside a translation or numerus form.
    LengthVariant,
    /// Elements whose content we don't need (e.g. `<oldsource>`).
    Ignored,
}

fn attr(e: &BytesStart<'_>, name: &str) -> Result<Option<String>> {
    for a in e.attributes() {
        let a = a?;
        if a.key.as_ref() == name {
            return Ok(Some(
                a.normalized_value(XmlVersion::Implicit1_0)?.into_owned(),
            ));
        }
    }
    Ok(None)
}

/// Parses the `value` attribute of a Qt `<byte value="x1b"/>` element.
fn parse_byte(value: &str) -> Option<char> {
    let n = match value.strip_prefix('x') {
        Some(hex) => u32::from_str_radix(hex, 16).ok()?,
        None => value.parse().ok()?,
    };
    char::from_u32(n)
}

/// Parses the contents of a `.ts` file.
pub fn parse(xml: &str) -> Result<TsFile> {
    let mut reader = Reader::from_str(xml);
    let mut file = TsFile::default();
    let mut context = String::new();
    let mut msg: Option<TsMessage> = None;
    // Stack of fields being collected; text goes to the innermost one.
    let mut fields: Vec<Field> = Vec::new();
    let mut text = String::new();
    // Whether the current `<translation>`/`<numerusform>` already received its
    // first `<lengthvariant>` (only the first, i.e. longest, one is used).
    let mut had_variant = false;
    let mut variant: Option<String> = None;

    loop {
        let pos = reader.buffer_position();
        let event = reader
            .read_event()
            .with_context(|| format!("XML error at byte {pos}"))?;
        match event {
            Event::Start(ref e) | Event::Empty(ref e) => {
                let is_empty = matches!(event, Event::Empty(_));
                let name = e.name();
                match name.as_ref() {
                    "TS" => {
                        file.language = attr(e, "language")?.unwrap_or_default();
                        file.source_language = attr(e, "sourcelanguage")?.unwrap_or_default();
                    }
                    "context" => context.clear(),
                    "name" if msg.is_none() => {
                        fields.push(Field::ContextName);
                        text.clear();
                    }
                    "message" => {
                        msg = Some(TsMessage {
                            context: context.clone(),
                            numerus: attr(e, "numerus")?.as_deref() == Some("yes"),
                            ..Default::default()
                        });
                    }
                    "location" => {
                        let m = msg.as_mut().context("<location> outside <message>")?;
                        if let Some(filename) = attr(e, "filename")? {
                            let line = attr(e, "line")?.and_then(|l| l.parse().ok());
                            m.locations.push(Location { filename, line });
                        }
                    }
                    "byte" => {
                        let value = attr(e, "value")?.unwrap_or_default();
                        match parse_byte(&value) {
                            Some(c) => text.push(c),
                            None => bail!("invalid <byte value=\"{value}\">"),
                        }
                    }
                    other => {
                        let field = match other {
                            "source" => Field::Source,
                            "comment" => Field::Comment,
                            "extracomment" => Field::ExtraComment,
                            "translatorcomment" => Field::TranslatorComment,
                            "translation" => {
                                let m = msg.as_mut().context("<translation> outside <message>")?;
                                m.translation_type = match attr(e, "type")?.as_deref() {
                                    None | Some("finished") => TranslationType::Finished,
                                    Some("unfinished") => TranslationType::Unfinished,
                                    Some("vanished" | "obsolete") => TranslationType::Obsolete,
                                    Some(t) => bail!("unknown translation type {t:?}"),
                                };
                                had_variant = false;
                                Field::Translation
                            }
                            "numerusform" => {
                                had_variant = false;
                                Field::NumerusForm
                            }
                            "lengthvariant" => Field::LengthVariant,
                            _ => Field::Ignored,
                        };
                        if is_empty {
                            // `<translation/>` etc.: nothing to collect.
                            if field == Field::NumerusForm
                                && let Some(m) = msg.as_mut()
                            {
                                m.numerus_forms.push(String::new());
                            }
                        } else {
                            if field == Field::LengthVariant && had_variant {
                                fields.push(Field::Ignored);
                                continue;
                            }
                            text.clear();
                            fields.push(field);
                        }
                    }
                }
            }
            Event::Text(t) => {
                if fields.last().is_some_and(|f| *f != Field::Ignored) {
                    text.push_str(&t.xml10_content());
                }
            }
            Event::CData(t) => {
                if fields.last().is_some_and(|f| *f != Field::Ignored) {
                    text.push_str(&t.xml10_content());
                }
            }
            Event::GeneralRef(r) => {
                if fields.last().is_some_and(|f| *f != Field::Ignored) {
                    if let Some(c) = r.resolve_char_ref()? {
                        text.push(c);
                    } else {
                        let name = r.xml10_content();
                        let resolved = resolve_predefined_entity(&name)
                            .with_context(|| format!("unknown entity &{name};"))?;
                        text.push_str(resolved);
                    }
                }
            }
            Event::End(e) => match e.name().as_ref() {
                "message" => {
                    let m = msg.take().context("unbalanced </message>")?;
                    file.messages.push(m);
                }
                "location" | "byte" | "context" | "TS" => {}
                _ => {
                    let Some(field) = fields.pop() else { continue };
                    let mut value = std::mem::take(&mut text);
                    if matches!(field, Field::Translation | Field::NumerusForm)
                        && let Some(v) = variant.take()
                    {
                        value = v;
                    }
                    match field {
                        Field::ContextName => context = value,
                        Field::Ignored => {}
                        Field::LengthVariant => {
                            had_variant = true;
                            variant = Some(value);
                        }
                        _ => {
                            let m = msg.as_mut().context("text element outside <message>")?;
                            match field {
                                Field::Source => m.source = value,
                                Field::Comment => m.comment = Some(value),
                                Field::ExtraComment => m.extra_comment = Some(value),
                                Field::TranslatorComment => m.translator_comment = Some(value),
                                Field::NumerusForm => m.numerus_forms.push(value),
                                Field::Translation => {
                                    // For numerus messages the text between
                                    // <numerusform> elements is whitespace only.
                                    if !m.numerus {
                                        m.translation = value;
                                    }
                                }
                                Field::ContextName | Field::Ignored | Field::LengthVariant => {
                                    unreachable!("handled above")
                                }
                            }
                        }
                    }
                }
            },
            Event::Eof => break,
            _ => {}
        }
    }
    if msg.is_some() || !fields.is_empty() {
        bail!("unexpected end of file");
    }
    Ok(file)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_messages() {
        let ts = parse(
            r#"<?xml version="1.0" ?><!DOCTYPE TS><TS version="2.1" language="de" sourcelanguage="en">
<context>
    <name>AttributeKey</name>
    <message>
        <location filename="a.cpp" line="50"/>
        <location filename="b.h"/>
        <source>Invalid key: &apos;%1&apos; &amp; &#x41;<byte value="x9"/></source>
        <comment>disambiguation</comment>
        <extracomment>for translators</extracomment>
        <translation>Ungültig: &apos;%1&apos;</translation>
    </message>
    <message numerus="yes">
        <source>%n item(s)</source>
        <translation type="unfinished">
            <numerusform>%n Element</numerusform>
            <numerusform></numerusform>
        </translation>
    </message>
    <message>
        <source>Old</source>
        <translation type="vanished"/>
    </message>
    <message>
        <source>Variants</source>
        <translation variants="yes"><lengthvariant>Long</lengthvariant><lengthvariant>S</lengthvariant></translation>
    </message>
</context>
</TS>"#,
        )
        .unwrap();
        assert_eq!(ts.language, "de");
        assert_eq!(ts.source_language, "en");
        assert_eq!(ts.messages.len(), 4);
        let m = &ts.messages[0];
        assert_eq!(m.context, "AttributeKey");
        assert_eq!(m.source, "Invalid key: '%1' & A\t");
        assert_eq!(m.comment.as_deref(), Some("disambiguation"));
        assert_eq!(m.extra_comment.as_deref(), Some("for translators"));
        assert_eq!(m.translation, "Ungültig: '%1'");
        assert_eq!(m.translation_type, TranslationType::Finished);
        assert_eq!(
            m.locations,
            vec![
                Location {
                    filename: "a.cpp".into(),
                    line: Some(50)
                },
                Location {
                    filename: "b.h".into(),
                    line: None
                },
            ]
        );
        let m = &ts.messages[1];
        assert!(m.numerus);
        assert_eq!(m.translation_type, TranslationType::Unfinished);
        assert_eq!(
            m.numerus_forms,
            vec!["%n Element".to_owned(), String::new()]
        );
        assert_eq!(ts.messages[2].translation_type, TranslationType::Obsolete);
        assert_eq!(ts.messages[3].translation, "Long");
    }
}
