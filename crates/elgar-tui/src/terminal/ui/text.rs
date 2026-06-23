//! Text formatting helpers for terminal transcript output.
//!
//! This file turns styled conversation lines into printable text blocks.

use crate::panes::ConversationLineStyle;

use super::prompt::{non_empty_lines, wrap_words};

pub(super) fn conversation_print_blocks(
    lines: impl IntoIterator<Item = (String, ConversationLineStyle)>,
    skip_user_and_loading: bool,
    skip_thinking: bool,
) -> Vec<(String, ConversationLineStyle)> {
    let mut blocks = Vec::new();
    let mut current: Option<(String, ConversationLineStyle)> = None;

    for (line, style) in lines {
        if skip_user_and_loading
            && matches!(
                style,
                ConversationLineStyle::User | ConversationLineStyle::Loading
            )
        {
            continue;
        }
        if skip_thinking && matches!(style, ConversationLineStyle::Thinking) {
            continue;
        }

        match current.as_mut() {
            Some((text, current_style)) if *current_style == style => {
                text.push('\n');
                text.push_str(&line);
            }
            Some(_) => {
                let block = current.take().expect("current block should exist");
                blocks.push(block);
                current = Some((line, style));
            }
            None => current = Some((line, style)),
        }
    }

    if let Some(block) = current {
        blocks.push(block);
    }

    blocks
}

pub(super) fn plain_block_lines(text: &str, width: usize) -> Vec<String> {
    let width = width.max(1);
    let mut lines = Vec::new();
    for raw_line in text.split('\n') {
        if preserves_leading_spacing(raw_line) {
            lines.extend(wrap_preserving_spacing(raw_line, width));
        } else {
            lines.extend(wrap_words(raw_line, width));
        }
    }
    non_empty_lines(lines)
}

pub(super) fn pad_line(line: &str, width: usize) -> String {
    let visible_width = line.chars().count();
    if visible_width >= width {
        line.to_string()
    } else {
        format!("{line}{:padding$}", "", padding = width - visible_width)
    }
}

fn preserves_leading_spacing(line: &str) -> bool {
    line.starts_with(' ') || line.starts_with('\t')
}

fn wrap_preserving_spacing(line: &str, width: usize) -> Vec<String> {
    let width = width.max(1);
    if line.is_empty() {
        return vec![String::new()];
    }

    let mut lines = Vec::new();
    let mut current = String::new();
    for character in line.chars() {
        if current.chars().count() == width {
            lines.push(std::mem::take(&mut current));
        }
        current.push(character);
    }
    if !current.is_empty() {
        lines.push(current);
    }
    lines
}
