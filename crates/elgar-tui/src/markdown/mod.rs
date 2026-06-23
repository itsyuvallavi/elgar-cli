//! Small markdown-to-terminal renderer.
//!
//! This module handles assistant markdown before it reaches conversation panes.
//! It is intentionally local and lightweight, not a full markdown engine.

mod code;
mod inline;
mod lists;
mod normalize;
mod tables;

use code::{render_code_block_lines, trim_trailing_blank_lines, CodeBlock};
use inline::render_plain_line;
use lists::{is_preformatted_line, render_list_line, render_preformatted_line};
use normalize::normalize_markdown_artifacts;
use tables::{is_table_start, render_table};

/// Render assistant markdown into terminal-friendly plain text.
pub(crate) fn render_assistant_markdown(markdown: &str) -> String {
    let normalized = normalize_markdown_artifacts(markdown);
    let lines: Vec<&str> = normalized.lines().collect();
    let mut rendered = Vec::new();
    let mut index = 0;
    let mut code_block: Option<CodeBlock> = None;
    let mut skip_blank_after_code_block = false;

    while index < lines.len() {
        let line = lines[index];
        let trimmed = line.trim();

        if let Some(block) = code_block.as_mut() {
            if trimmed.starts_with("```") {
                let block = code_block
                    .take()
                    .expect("code block exists while rendering code line");
                trim_trailing_blank_lines(&mut rendered);
                render_code_block_lines(&mut rendered, block);
                skip_blank_after_code_block = true;
            } else {
                block.lines.push(line.to_string());
            }
            index += 1;
            continue;
        }

        if let Some(language) = trimmed.strip_prefix("```") {
            code_block = Some(CodeBlock::new(language.trim()));
            index += 1;
            continue;
        }

        if skip_blank_after_code_block && trimmed.is_empty() {
            index += 1;
            continue;
        }
        skip_blank_after_code_block = false;

        if trimmed.is_empty() {
            index += 1;
            continue;
        }

        if is_table_start(&lines, index) {
            let (table, next_index) = render_table(&lines, index);
            rendered.extend(table);
            index = next_index;
            continue;
        }

        if let Some(list_line) = render_list_line(line) {
            rendered.push(list_line);
        } else if is_preformatted_line(line) {
            rendered.push(render_preformatted_line(line));
        } else {
            rendered.push(render_plain_line(line));
        }

        index += 1;
    }

    if let Some(block) = code_block {
        trim_trailing_blank_lines(&mut rendered);
        render_code_block_lines(&mut rendered, block);
    }

    rendered.join("\n")
}

pub(crate) fn assistant_markdown_has_hidden_details(markdown: &str) -> bool {
    let normalized = normalize_markdown_artifacts(markdown);
    let mut code_block: Option<Vec<String>> = None;

    for line in normalized.lines() {
        let trimmed = line.trim();
        if let Some(lines) = code_block.as_mut() {
            if trimmed.starts_with("```") {
                let lines = code_block
                    .take()
                    .expect("code block exists while checking close fence");
                if crate::code_blocks::code_block_would_collapse(&lines) {
                    return true;
                }
            } else {
                lines.push(line.to_string());
            }
            continue;
        }

        if trimmed.starts_with("```") {
            code_block = Some(Vec::new());
        }
    }

    code_block.is_some_and(|lines| crate::code_blocks::code_block_would_collapse(&lines))
}

pub(crate) fn render_assistant_markdown_details(markdown: &str) -> String {
    let mut details = String::from("Assistant message details\nRaw markdown:\n");
    details.push_str(markdown.trim_end());
    details
}

#[cfg(test)]
mod tests;
