//! Token-level syntax styling for rendered code blocks.
//!
//! This module classifies text inside an already detected code block line.

use crate::{
    terminal::{
        ANSI_CODE_BODY, ANSI_CODE_COMMENT, ANSI_CODE_KEY, ANSI_CODE_LITERAL, ANSI_CODE_NUMBER,
        ANSI_CODE_STRING,
    },
    theme,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct CodeSyntaxSegment {
    pub(super) text: String,
    pub(super) style: CodeTokenStyle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum CodeTokenStyle {
    Body,
    Key,
    String,
    Number,
    Literal,
    Comment,
}

impl CodeTokenStyle {
    pub(super) fn ratatui_style(self) -> ratatui::style::Style {
        match self {
            CodeTokenStyle::Body => theme::code_body(),
            CodeTokenStyle::Key => theme::code_key(),
            CodeTokenStyle::String => theme::code_string(),
            CodeTokenStyle::Number => theme::code_number(),
            CodeTokenStyle::Literal => theme::code_literal(),
            CodeTokenStyle::Comment => theme::code_comment(),
        }
    }

    pub(super) fn ansi(self) -> &'static str {
        match self {
            CodeTokenStyle::Body => ANSI_CODE_BODY,
            CodeTokenStyle::Key => ANSI_CODE_KEY,
            CodeTokenStyle::String => ANSI_CODE_STRING,
            CodeTokenStyle::Number => ANSI_CODE_NUMBER,
            CodeTokenStyle::Literal => ANSI_CODE_LITERAL,
            CodeTokenStyle::Comment => ANSI_CODE_COMMENT,
        }
    }
}

pub(super) fn code_syntax_segments(body: &str, language: Option<&str>) -> Vec<CodeSyntaxSegment> {
    let Some(language) = normalized_code_language(language) else {
        return vec![CodeSyntaxSegment {
            text: body.to_string(),
            style: CodeTokenStyle::Body,
        }];
    };

    let content = body.trim_end_matches(char::is_whitespace);
    let trailing = &body[content.len()..];
    let mut segments = syntax_segments_for_content(content, &language);
    push_code_segment(&mut segments, trailing, CodeTokenStyle::Body);
    segments
}

fn normalized_code_language(language: Option<&str>) -> Option<String> {
    let normalized = language?
        .trim()
        .trim_start_matches('.')
        .to_ascii_lowercase();
    let normalized = match normalized.as_str() {
        "bash" | "sh" | "shell" | "zsh" => "bash",
        "json" => "json",
        "javascript" | "js" | "jsx" => "javascript",
        "python" | "py" => "python",
        "rust" | "rs" => "rust",
        "toml" => "toml",
        "typescript" | "ts" | "tsx" => "typescript",
        "yaml" | "yml" => "yaml",
        "plain" | "plaintext" | "text" | "txt" => return None,
        _ => return None,
    };
    Some(normalized.to_string())
}

fn syntax_segments_for_content(content: &str, language: &str) -> Vec<CodeSyntaxSegment> {
    if let Some(comment_start) = line_comment_start(content, language) {
        let mut segments = Vec::new();
        push_code_segment(
            &mut segments,
            &content[..comment_start],
            CodeTokenStyle::Body,
        );
        push_code_segment(
            &mut segments,
            &content[comment_start..],
            CodeTokenStyle::Comment,
        );
        return segments;
    }

    if let Some((prefix, key, separator, rest)) = split_config_key(content, language) {
        let mut segments = Vec::new();
        push_code_segment(&mut segments, prefix, CodeTokenStyle::Body);
        push_code_segment(&mut segments, key, CodeTokenStyle::Key);
        push_code_segment(&mut segments, separator, CodeTokenStyle::Body);
        segments.extend(scan_code_tokens(rest, language));
        return segments;
    }

    scan_code_tokens(content, language)
}

fn line_comment_start(content: &str, language: &str) -> Option<usize> {
    let marker = comment_marker(language)?;
    let trimmed = content.trim_start();
    if trimmed.starts_with(marker) {
        Some(content.len() - trimmed.len())
    } else {
        None
    }
}

fn split_config_key<'a>(
    content: &'a str,
    language: &str,
) -> Option<(&'a str, &'a str, &'a str, &'a str)> {
    let value = content.trim_start();
    let prefix_len = content.len() - value.len();
    let prefix = &content[..prefix_len];

    if language == "json" {
        let key_end = closing_quote_index(value, '"')?;
        let after_key = &value[(key_end + 1)..];
        let colon = after_key.find(':')?;
        let after_colon = &after_key[(colon + 1)..];
        let rest = after_colon.trim_start();
        let separator_len = colon + 1 + after_colon.len() - rest.len();
        return Some((
            prefix,
            &value[..=key_end],
            &after_key[..separator_len],
            rest,
        ));
    }

    let separator = match language {
        "toml" => '=',
        "yaml" => ':',
        _ => return None,
    };
    if value.starts_with('[') {
        return None;
    }

    let key_len = value
        .char_indices()
        .take_while(|(_, character)| is_config_key_character(*character))
        .map(|(index, character)| index + character.len_utf8())
        .last()?;
    let key = &value[..key_len];
    let after_key = &value[key_len..];
    let after_key_trimmed = after_key.trim_start();
    if !after_key_trimmed.starts_with(separator) {
        return None;
    }

    let before_separator_len = after_key.len() - after_key_trimmed.len();
    let after_separator = &after_key_trimmed[separator.len_utf8()..];
    let rest = after_separator.trim_start();
    let separator_len =
        before_separator_len + separator.len_utf8() + after_separator.len() - rest.len();
    Some((prefix, key, &after_key[..separator_len], rest))
}

fn scan_code_tokens(content: &str, language: &str) -> Vec<CodeSyntaxSegment> {
    let mut segments = Vec::new();
    let mut index = 0;
    while index < content.len() {
        let rest = &content[index..];
        if starts_inline_comment(content, index, language) {
            push_code_segment(&mut segments, rest, CodeTokenStyle::Comment);
            break;
        }

        let character = rest.chars().next().expect("non-empty rest");
        if matches!(character, '"' | '\'') {
            let length = closing_quote_index(rest, character)
                .map(|quote_index| quote_index + character.len_utf8())
                .unwrap_or(rest.len());
            push_code_segment(&mut segments, &rest[..length], CodeTokenStyle::String);
            index += length;
            continue;
        }

        if let Some(length) = number_literal_len(rest) {
            push_code_segment(&mut segments, &rest[..length], CodeTokenStyle::Number);
            index += length;
            continue;
        }

        if is_identifier_start(character) {
            let length = identifier_len(rest);
            let identifier = &rest[..length];
            let style = if is_literal_identifier(identifier) {
                CodeTokenStyle::Literal
            } else {
                CodeTokenStyle::Body
            };
            push_code_segment(&mut segments, identifier, style);
            index += length;
            continue;
        }

        push_code_segment(
            &mut segments,
            &rest[..character.len_utf8()],
            CodeTokenStyle::Body,
        );
        index += character.len_utf8();
    }
    segments
}

fn push_code_segment(segments: &mut Vec<CodeSyntaxSegment>, text: &str, style: CodeTokenStyle) {
    if text.is_empty() {
        return;
    }
    if let Some(previous) = segments.last_mut().filter(|segment| segment.style == style) {
        previous.text.push_str(text);
    } else {
        segments.push(CodeSyntaxSegment {
            text: text.to_string(),
            style,
        });
    }
}

fn comment_marker(language: &str) -> Option<&'static str> {
    match language {
        "bash" | "python" | "toml" | "yaml" => Some("#"),
        "javascript" | "rust" | "typescript" => Some("//"),
        _ => None,
    }
}

fn starts_inline_comment(content: &str, index: usize, language: &str) -> bool {
    let Some(marker) = comment_marker(language) else {
        return false;
    };
    if !content[index..].starts_with(marker) {
        return false;
    }
    index == 0
        || content[..index]
            .chars()
            .last()
            .is_some_and(|character| character.is_whitespace())
}

fn closing_quote_index(value: &str, quote: char) -> Option<usize> {
    if !value.starts_with(quote) {
        return None;
    }

    let mut escaped = false;
    for (index, character) in value.char_indices().skip(1) {
        if escaped {
            escaped = false;
            continue;
        }
        if character == '\\' {
            escaped = true;
            continue;
        }
        if character == quote {
            return Some(index);
        }
    }
    None
}

fn number_literal_len(value: &str) -> Option<usize> {
    let mut chars = value.char_indices().peekable();
    if chars.peek().is_some_and(|(_, character)| *character == '-') {
        chars.next();
    }
    if !chars
        .peek()
        .is_some_and(|(_, character)| character.is_ascii_digit())
    {
        return None;
    }

    let mut end = 0;
    for (index, character) in chars {
        if character.is_ascii_alphanumeric() || matches!(character, '_' | '.' | '+' | '-') {
            end = index + character.len_utf8();
        } else {
            break;
        }
    }
    Some(end)
}

fn identifier_len(value: &str) -> usize {
    value
        .char_indices()
        .take_while(|(_, character)| is_identifier_character(*character))
        .map(|(index, character)| index + character.len_utf8())
        .last()
        .unwrap_or(0)
}

fn is_identifier_start(character: char) -> bool {
    character.is_ascii_alphabetic() || character == '_'
}

fn is_identifier_character(character: char) -> bool {
    character.is_ascii_alphanumeric() || character == '_'
}

fn is_literal_identifier(identifier: &str) -> bool {
    matches!(
        identifier,
        "Err" | "False" | "None" | "Ok" | "Some" | "True" | "false" | "null" | "true" | "undefined"
    )
}

fn is_config_key_character(character: char) -> bool {
    character.is_ascii_alphanumeric() || matches!(character, '_' | '-' | '.')
}
