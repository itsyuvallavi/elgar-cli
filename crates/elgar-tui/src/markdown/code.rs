//! Fenced code block bridge for assistant markdown.
//!
//! Markdown parsing collects fence metadata and lines here, then delegates the
//! terminal box rendering to `crate::code_blocks`.

use crate::code_blocks::{render_code_block, CodeBlockInput};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct CodeBlock {
    language: String,
    pub(super) lines: Vec<String>,
}

impl CodeBlock {
    pub(super) fn new(language: &str) -> Self {
        Self {
            language: language.to_string(),
            lines: Vec::new(),
        }
    }
}

pub(super) fn render_code_block_lines(rendered: &mut Vec<String>, block: CodeBlock) {
    let block = render_code_block(CodeBlockInput::new(block.language, block.lines));
    rendered.extend(block.lines);
}

pub(super) fn trim_trailing_blank_lines(lines: &mut Vec<String>) {
    while lines.last().is_some_and(|line| line.trim().is_empty()) {
        lines.pop();
    }
}
