//! Code fence styling for terminal transcript output.
//!
//! Shared between inline stdout rendering and ratatui styled conversation text.

use std::io::{self, Write};

use ratatui::text::{Line, Span};

use crate::{
    terminal::{ANSI_CODE_BORDER, ANSI_CODE_HEADER, ANSI_CODE_HINT, ANSI_RESET},
    theme,
};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct CodeLineStyleState {
    language: Option<String>,
}

impl CodeLineStyleState {
    pub(crate) fn reset(&mut self) {
        self.language = None;
    }
}

pub(crate) fn styled_transcript_line(
    line: &str,
    fallback: ratatui::style::Style,
    code_state: &mut CodeLineStyleState,
) -> Line<'static> {
    styled_code_line(line, code_state).unwrap_or_else(|| {
        code_state.language = None;
        Line::styled(line.to_string(), fallback)
    })
}

fn styled_code_line(line: &str, code_state: &mut CodeLineStyleState) -> Option<Line<'static>> {
    if let Some((border_prefix, header, border_suffix)) = split_code_header_line(line) {
        code_state.language = code_header_language(&header);
        return Some(Line::from(vec![
            Span::styled(border_prefix, theme::code_border()),
            Span::styled(header, theme::code_header()),
            Span::styled(border_suffix, theme::code_border()),
        ]));
    }

    if is_code_bottom_line(line) {
        code_state.language = None;
        return Some(Line::styled(line.to_string(), theme::code_border()));
    }

    if let Some((border_prefix, body, border_suffix)) = split_code_body_line(line) {
        let mut spans = Vec::new();
        spans.push(Span::styled(border_prefix, theme::code_border()));
        if is_code_hint_body(&body) {
            spans.push(Span::styled(body, theme::code_hint()));
        } else {
            spans.extend(
                code_syntax_segments(&body, code_state.language.as_deref())
                    .into_iter()
                    .map(|segment| Span::styled(segment.text, segment.style.ratatui_style())),
            );
        }
        spans.push(Span::styled(border_suffix, theme::code_border()));
        return Some(Line::from(spans));
    }

    None
}

pub(crate) fn write_code_line_ansi(
    line: &str,
    newline: &str,
    code_state: &mut CodeLineStyleState,
) -> io::Result<bool> {
    if let Some((border_prefix, header, border_suffix)) = split_code_header_line(line) {
        code_state.language = code_header_language(&header);
        write!(
            io::stdout(),
            "{ANSI_CODE_BORDER}{border_prefix}{ANSI_CODE_HEADER}{header}{ANSI_CODE_BORDER}{border_suffix}{ANSI_RESET}{newline}"
        )?;
        return Ok(true);
    }

    if is_code_bottom_line(line) {
        code_state.language = None;
        write!(
            io::stdout(),
            "{ANSI_CODE_BORDER}{line}{ANSI_RESET}{newline}"
        )?;
        return Ok(true);
    }

    if let Some((border_prefix, body, border_suffix)) = split_code_body_line(line) {
        write!(io::stdout(), "{ANSI_CODE_BORDER}{border_prefix}")?;
        if is_code_hint_body(&body) {
            write!(io::stdout(), "{ANSI_CODE_HINT}{body}")?;
        } else {
            for segment in code_syntax_segments(&body, code_state.language.as_deref()) {
                write!(io::stdout(), "{}{}", segment.style.ansi(), segment.text)?;
            }
        }
        write!(
            io::stdout(),
            "{ANSI_CODE_BORDER}{border_suffix}{ANSI_RESET}{newline}"
        )?;
        return Ok(true);
    }

    Ok(false)
}

fn code_header_language(header: &str) -> Option<String> {
    let language = header
        .strip_prefix("code (")
        .and_then(|rest| {
            rest.split_once(')')
                .map(|(language, _rest)| language.trim())
        })
        .or_else(|| simplified_header_language(header))?;
    if language.is_empty() {
        None
    } else {
        Some(language.to_string())
    }
}

fn simplified_header_language(header: &str) -> Option<&str> {
    let label = header
        .split_once(" · ")
        .map_or(header, |(label, _rest)| label);
    match label {
        "bash" | "sh" | "shell" | "zsh" | "json" | "javascript" | "js" | "jsx" | "markdown"
        | "md" | "python" | "py" | "rust" | "rs" | "toml" | "typescript" | "ts" | "tsx"
        | "yaml" | "yml" => Some(label),
        _ => extension_language(label),
    }
}

fn extension_language(label: &str) -> Option<&str> {
    let extension = label.rsplit_once('.')?.1;
    let extension = extension
        .trim_matches(|character: char| !character.is_ascii_alphanumeric())
        .trim();
    if extension.is_empty() {
        None
    } else {
        Some(extension)
    }
}

use super::code_tokens::code_syntax_segments;

fn split_code_header_line(line: &str) -> Option<(String, String, String)> {
    let body = line.strip_prefix(" ╭─ ")?.strip_suffix('╮')?;
    let header = body.trim_end_matches('─').trim_end();
    if !header.starts_with("code") && simplified_header_language(header).is_none() {
        return None;
    }
    let suffix = &body[header.len()..];
    Some((" ╭─ ".to_string(), header.to_string(), format!("{suffix}╮")))
}

fn split_code_body_line(line: &str) -> Option<(String, String, String)> {
    let body = line.strip_prefix(" │ ")?.strip_suffix(" │")?;
    Some((" │ ".to_string(), body.to_string(), " │".to_string()))
}

fn is_code_bottom_line(line: &str) -> bool {
    line.starts_with(" ╰") && line.ends_with('╯') && line.chars().all(is_code_border_character)
}

fn is_code_border_character(character: char) -> bool {
    matches!(character, ' ' | '╰' | '╯' | '─')
}

fn is_code_hint_body(body: &str) -> bool {
    body.trim_start().starts_with("... ") && body.contains(" hidden; ")
}
