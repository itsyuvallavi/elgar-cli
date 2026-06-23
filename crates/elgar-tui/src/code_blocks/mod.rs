//! Code block rendering helpers.
//!
//! This module renders fenced code/script blocks into compact
//! terminal-friendly boxes so long model output does not flood the screen.

mod box_render;
mod fence;
mod wrap;

use box_render::render_boxed_code_block;
use fence::{render_code_header, CodeFenceInfo};

pub(crate) const CODE_BLOCK_COLLAPSE_LINE_THRESHOLD: usize = 80;
pub(crate) const CODE_BLOCK_VISIBLE_LINE_LIMIT: usize = 40;
const CODE_BLOCK_COLLAPSE_CHAR_THRESHOLD: usize = 4_000;
const CODE_BOX_MIN_CONTENT_WIDTH: usize = 64;
const CODE_BOX_MAX_CONTENT_WIDTH: usize = 72;
const CODE_WRAP_CONTINUATION_PREFIX: &str = "  ↳ ";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CodeBlockRender {
    pub(crate) lines: Vec<String>,
    pub(crate) collapsed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CodeBlockInput {
    pub(crate) info: String,
    pub(crate) lines: Vec<String>,
}

impl CodeBlockInput {
    pub(crate) fn new(info: impl Into<String>, lines: Vec<String>) -> Self {
        Self {
            info: info.into(),
            lines,
        }
    }
}

/// Render a code block with optional truncation/collapse behavior.
pub(crate) fn render_code_block(input: CodeBlockInput) -> CodeBlockRender {
    let info = CodeFenceInfo::parse(&input.info);
    let display_lines = compact_display_lines(trim_code_edges(&input.lines));
    let line_count = display_lines.len();
    let collapsed = code_block_would_collapse(&display_lines);
    let shown_line_count = if collapsed {
        CODE_BLOCK_VISIBLE_LINE_LIMIT.min(line_count)
    } else {
        line_count
    };

    let header = render_code_header(&info, line_count, collapsed.then_some(shown_line_count));
    let mut body_lines = display_lines
        .iter()
        .take(shown_line_count)
        .cloned()
        .collect::<Vec<_>>();

    if collapsed {
        let hidden = line_count.saturating_sub(shown_line_count);
        body_lines.push(format!(
            "... {hidden} lines hidden; use /details last or /copy raw"
        ));
    }

    let lines = render_boxed_code_block(&header, &body_lines);

    CodeBlockRender { lines, collapsed }
}

pub(crate) fn code_block_would_collapse(lines: &[String]) -> bool {
    let display_lines = compact_display_lines(trim_code_edges(lines));
    display_lines.len() > CODE_BLOCK_COLLAPSE_LINE_THRESHOLD
        || display_lines
            .iter()
            .map(|line| line.len().saturating_add(1))
            .sum::<usize>()
            > CODE_BLOCK_COLLAPSE_CHAR_THRESHOLD
}

fn trim_code_edges(lines: &[String]) -> &[String] {
    let Some(start) = lines.iter().position(|line| !line.trim().is_empty()) else {
        return &[];
    };
    let end = lines
        .iter()
        .rposition(|line| !line.trim().is_empty())
        .map(|index| index + 1)
        .unwrap_or(start);
    &lines[start..end]
}

fn compact_display_lines(lines: &[String]) -> Vec<String> {
    lines
        .iter()
        .filter(|line| !line.trim().is_empty())
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests;
