//! Help texts and parser error messages in the format of Qt's
//! `QCommandLineParser` (used by upstream apps/librepcb-cli).
//!
//! The command line is parsed by `clap`, but the help texts and error
//! messages are part of the CLI's output which scripts and the upstream CLI
//! tests compare verbatim. `clap`'s own formatting differs, so this module
//! reproduces `QCommandLineParser::helpText()` (including its word
//! wrapping) and `errorText()`. Hand-written because no crate reproduces
//! Qt's output format.

use clap::error::{ContextKind, ContextValue, ErrorKind};
use librepcb_i18n::tr;

/// An option in a help text.
#[derive(Debug, Clone)]
pub struct HelpOption {
    /// Names without dashes, e.g. `["h", "help"]`.
    pub names: Vec<String>,
    /// Value name, if the option takes a value.
    pub value_name: Option<String>,
    /// Description.
    pub description: String,
}

/// A positional argument in a help text.
#[derive(Debug, Clone)]
pub struct HelpPositional {
    /// Name shown in the "Arguments" section.
    pub name: String,
    /// Description.
    pub description: String,
    /// Syntax shown in the usage line.
    pub syntax: String,
}

/// Qt's `QCommandLineParser::helpText()`.
pub fn help_text(
    executable: &str,
    description: &str,
    options: &[HelpOption],
    positionals: &[HelpPositional],
) -> String {
    let mut usage = executable.to_owned();
    if !options.is_empty() {
        usage.push(' ');
        usage.push_str(&tr!("QCommandLineParser", "[options]"));
    }
    for arg in positionals {
        usage.push(' ');
        usage.push_str(&arg.syntax);
    }
    let mut text = tr!("QCommandLineParser", "Usage: {0}", usage);
    text.push('\n');
    if !description.is_empty() {
        text.push_str(description);
        text.push('\n');
    }
    text.push('\n');
    if !options.is_empty() {
        text.push_str(&tr!("QCommandLineParser", "Options:"));
        text.push('\n');
    }
    let names: Vec<String> = options
        .iter()
        .map(|option| {
            let mut s = option
                .names
                .iter()
                .map(|name| {
                    let dashes = if name.chars().count() == 1 { "-" } else { "--" };
                    format!("{dashes}{name}")
                })
                .collect::<Vec<_>>()
                .join(", ");
            if let Some(value_name) = &option.value_name {
                s.push_str(&format!(" <{value_name}>"));
            }
            s
        })
        .collect();
    let longest = names.iter().map(|n| n.chars().count()).max().unwrap_or(0) + 1;
    let width = longest.min(50);
    for (name, option) in names.iter().zip(options) {
        text.push_str(&wrap_text(name, width, &option.description));
    }
    if !positionals.is_empty() {
        if !options.is_empty() {
            text.push('\n');
        }
        text.push_str(&tr!("QCommandLineParser", "Arguments:"));
        text.push('\n');
        for arg in positionals {
            text.push_str(&wrap_text(&arg.name, width, &arg.description));
        }
    }
    text
}

/// Qt's `wrapText()` of `QCommandLineParser` (a line is at most 79
/// characters wide), reproduced exactly including its quirks: the first
/// character of a continuation line is not counted towards the line width.
fn wrap_text(names: &str, width: usize, description: &str) -> String {
    let indentation = "  ";
    let names: Vec<char> = names.chars().collect();
    let mut name_index = 0;
    // Returns the next `width` characters of the names.
    let next_name_section = |name_index: &mut usize| -> String {
        let end = (*name_index + width).min(names.len());
        let section: String = names[*name_index..end].iter().collect();
        *name_index = end;
        section
    };

    let desc: Vec<char> = description.chars().collect();
    let len = desc.len();
    let max = 79_i64 - (indentation.len() + width + 1) as i64;
    let mut text = String::new();
    let mut line_start = 0;
    let mut last_breakable: Option<usize> = None;
    let mut x: i64 = 0;
    let mut i = 0;
    while i < len {
        x += 1;
        let c = desc[i];
        if c.is_whitespace() {
            last_breakable = Some(i);
        }
        let mut brk: Option<(usize, usize)> = None; // (break at, next line start)
        if let (true, Some(lb)) = (x > max, last_breakable) {
            // Time to break and we know where.
            brk = Some((lb, lb + 1));
        } else if (x > max - 1 && last_breakable.is_none()) || i == len - 1 {
            // Time to break but found nowhere [-> break here], or end of
            // last line.
            brk = Some((i + 1, i + 1));
        } else if c == '\n' {
            // Forced break.
            brk = Some((i, i + 1));
        }
        if let Some((break_at, next_line_start)) = brk {
            let section = next_name_section(&mut name_index);
            text.push_str(indentation);
            text.push_str(&format!("{section:<width$}"));
            text.push(' ');
            text.extend(&desc[line_start..break_at.max(line_start)]);
            text.push('\n');
            x = 0;
            last_breakable = None;
            line_start = next_line_start;
            if line_start < len && desc[line_start].is_whitespace() {
                line_start += 1; // Don't start a line with a space.
            }
            i = line_start;
        }
        i += 1;
    }
    while name_index < names.len() {
        let section = next_name_section(&mut name_index);
        text.push_str(indentation);
        text.push_str(&section);
        text.push('\n');
    }
    text
}

/// Converts a `clap` parse error into the error text of Qt's
/// `QCommandLineParser::errorText()`.
pub fn parse_error_text(err: &clap::Error) -> String {
    let arg_name = || -> String {
        match err.get(ContextKind::InvalidArg) {
            Some(ContextValue::String(s)) => s.clone(),
            Some(ContextValue::Strings(v)) => v.first().cloned().unwrap_or_default(),
            _ => String::new(),
        }
    };
    // "--board <name>" -> "--board", "--foo=bar" -> "--foo"
    let option = || -> String {
        let name = arg_name();
        name.split([' ', '=']).next().unwrap_or_default().to_owned()
    };
    match err.kind() {
        ErrorKind::UnknownArgument => {
            let name = option();
            let name = name.trim_start_matches('-');
            tr!("QCommandLineParser", "Unknown option '{0}'.", name)
        }
        ErrorKind::InvalidValue | ErrorKind::NoEquals => {
            tr!("QCommandLineParser", "Missing value after '{0}'.", option())
        }
        ErrorKind::TooManyValues => {
            tr!(
                "QCommandLineParser",
                "Unexpected value after '{0}'.",
                option()
            )
        }
        _ => {
            // Not expected to happen (positional arguments are validated by
            // the CLI itself): show clap's message without its "error: "
            // prefix and usage.
            let rendered = err.render().to_string();
            let first = rendered.lines().next().unwrap_or_default();
            first.strip_prefix("error: ").unwrap_or(first).to_owned()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wrap_short_description() {
        assert_eq!(
            wrap_text("-h, --help", 14, "Print this message."),
            "  -h, --help     Print this message.\n"
        );
    }

    #[test]
    fn wrap_long_description() {
        let text = wrap_text(
            "--erc",
            34,
            "Run the electrical rule check, print all non-approved warnings/errors and report \
             failure (exit code = 1) if there are non-approved messages.",
        );
        assert_eq!(
            text,
            "  --erc                              Run the electrical rule check, print all\n\
            \x20                                    non-approved warnings/errors and report\n\
            \x20                                    failure (exit code = 1) if there are\n\
            \x20                                    non-approved messages.\n"
        );
    }

    #[test]
    fn wrap_empty_description() {
        assert_eq!(wrap_text("--foo", 10, ""), "  --foo\n");
    }
}
