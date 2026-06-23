//! Token-level syntax styling for rendered code blocks.
//!
//! This module classifies text inside an already detected code block line.

mod language;
mod scanner;

use crate::{
    terminal::{
        ANSI_CODE_BODY, ANSI_CODE_COMMENT, ANSI_CODE_KEY, ANSI_CODE_LITERAL, ANSI_CODE_NUMBER,
        ANSI_CODE_STRING,
    },
    theme,
};

use language::normalized_code_language;
use scanner::{push_code_segment, syntax_segments_for_content};

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
