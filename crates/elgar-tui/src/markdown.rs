//! Small markdown-to-terminal renderer.
//!
//! This handles assistant markdown before it reaches conversation panes. It is
//! intentionally local and lightweight, not a full markdown engine.

use crate::code_blocks::{code_block_would_collapse, render_code_block, CodeBlockInput};

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
                if code_block_would_collapse(&lines) {
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

    code_block.is_some_and(|lines| code_block_would_collapse(&lines))
}

pub(crate) fn render_assistant_markdown_details(markdown: &str) -> String {
    let mut details = String::from("Assistant message details\nRaw markdown:\n");
    details.push_str(markdown.trim_end());
    details
}

fn normalize_markdown_artifacts(markdown: &str) -> String {
    let normalized = markdown.replace("\r\n", "\n").replace("<br>", "\n");
    let normalized = expand_inline_fenced_code_blocks(&normalized);
    split_inline_bullet_markers(&normalized)
}

fn expand_inline_fenced_code_blocks(text: &str) -> String {
    text.lines()
        .map(expand_inline_fenced_code_line)
        .collect::<Vec<_>>()
        .join("\n")
}

fn expand_inline_fenced_code_line(line: &str) -> String {
    let mut rendered = Vec::new();
    let mut rest = line;

    while let Some(start) = rest.find("```") {
        let prefix = rest[..start].trim_end();
        if !prefix.is_empty() {
            rendered.push(prefix.to_string());
        }

        let after_open = &rest[start + 3..];
        let Some(end) = after_open.find("```") else {
            rendered.push(rest.to_string());
            return rendered.join("\n");
        };

        let fenced = after_open[..end].trim();
        let (language, code) = split_inline_fence_content(fenced);
        if language.is_empty() {
            rendered.push("```".to_string());
        } else {
            rendered.push(format!("```{language}"));
        }
        rendered.extend(normalize_inline_code_content(code));
        rendered.push("```".to_string());

        rest = after_open[end + 3..].trim_start();
    }

    if !rest.trim().is_empty() {
        rendered.push(rest.to_string());
    }

    if rendered.is_empty() {
        line.to_string()
    } else {
        rendered.join("\n")
    }
}

fn split_inline_fence_content(fenced: &str) -> (&str, &str) {
    let fenced = fenced.trim();
    let Some((first, rest)) = fenced.split_once(char::is_whitespace) else {
        return (fenced, "");
    };

    if first
        .chars()
        .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_'))
    {
        (first, rest.trim())
    } else {
        ("", fenced)
    }
}

fn normalize_inline_code_content(code: &str) -> Vec<String> {
    let mut normalized = code.trim().to_string();
    for marker in [
        "# 1.",
        "# 2.",
        "# 3.",
        "# 4.",
        "# 5.",
        "# Expected",
        "# Output",
        "# Result",
    ] {
        normalized = normalized.replace(&format!(" {marker}"), &format!("\n{marker}"));
    }

    normalized
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .collect()
}

fn split_inline_bullet_markers(text: &str) -> String {
    let mut in_code_block = false;
    text.lines()
        .map(|line| {
            let trimmed = line.trim();
            if trimmed.starts_with("```") {
                in_code_block = !in_code_block;
                return line.to_string();
            }
            if in_code_block {
                return line.to_string();
            }
            split_inline_bullet_line(line)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn split_inline_bullet_line(line: &str) -> String {
    let line = line.replace(": - ", ":\n- ");

    if (line.contains("\n- ") && line.contains(" - ")) || line.matches(" - ").count() >= 2 {
        line.replace(" - ", "\n- ")
    } else {
        line
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CodeBlock {
    language: String,
    lines: Vec<String>,
}

impl CodeBlock {
    fn new(language: &str) -> Self {
        Self {
            language: language.to_string(),
            lines: Vec::new(),
        }
    }
}

fn render_code_block_lines(rendered: &mut Vec<String>, block: CodeBlock) {
    let block = render_code_block(CodeBlockInput::new(block.language, block.lines));
    rendered.extend(block.lines);
}

fn trim_trailing_blank_lines(lines: &mut Vec<String>) {
    while lines.last().is_some_and(|line| line.trim().is_empty()) {
        lines.pop();
    }
}

fn render_list_line(line: &str) -> Option<String> {
    let trimmed_start = line.trim_start();
    let indent = line.len().saturating_sub(trimmed_start.len());
    let rendered_indent = " ".repeat(indent.min(6));

    for marker in ["- ", "* ", "+ "] {
        if let Some(item) = trimmed_start.strip_prefix(marker) {
            return Some(format!("{rendered_indent}- {}", render_inline(item.trim())));
        }
    }

    let (number, item) = trimmed_start.split_once(". ")?;
    if number.chars().all(|character| character.is_ascii_digit()) && !number.is_empty() {
        Some(format!(
            "{rendered_indent}{number}. {}",
            render_inline(item.trim())
        ))
    } else {
        None
    }
}

fn is_preformatted_line(line: &str) -> bool {
    line.starts_with("    ") || line.starts_with('\t')
}

fn render_preformatted_line(line: &str) -> String {
    if let Some(rest) = line.strip_prefix('\t') {
        format!("    {rest}")
    } else {
        line.to_string()
    }
}

fn is_table_start(lines: &[&str], index: usize) -> bool {
    index + 1 < lines.len() && is_table_row(lines[index]) && is_table_separator(lines[index + 1])
}

fn render_table(lines: &[&str], start: usize) -> (Vec<String>, usize) {
    let mut rows = Vec::new();
    let mut index = start;

    while index < lines.len() && is_table_row(lines[index]) {
        if !is_table_separator(lines[index]) {
            rows.push(table_cells(lines[index]));
        }
        index += 1;
    }

    let column_count = rows.iter().map(Vec::len).max().unwrap_or(0);
    let mut widths = vec![0; column_count];
    for row in &rows {
        for (column, cell) in row.iter().enumerate() {
            widths[column] = widths[column].max(cell.len());
        }
    }

    let mut rendered = Vec::new();
    for (row_index, row) in rows.iter().enumerate() {
        rendered.push(render_table_row(row, &widths));
        if row_index == 0 && rows.len() > 1 {
            rendered.push(render_table_rule(&widths));
        }
    }

    (rendered, index)
}

fn is_table_row(line: &str) -> bool {
    let trimmed = line.trim();
    trimmed.contains('|') && !trimmed.is_empty()
}

fn is_table_separator(line: &str) -> bool {
    let cells = table_cells(line);
    !cells.is_empty()
        && cells.iter().all(|cell| {
            let trimmed = cell.trim_matches(':');
            trimmed.len() >= 3 && trimmed.chars().all(|character| character == '-')
        })
}

fn table_cells(line: &str) -> Vec<String> {
    line.trim()
        .trim_matches('|')
        .split('|')
        .map(|cell| render_inline(cell.trim()))
        .collect()
}

fn render_table_row(row: &[String], widths: &[usize]) -> String {
    let cells = widths
        .iter()
        .enumerate()
        .map(|(column, width)| {
            let cell = row.get(column).map(String::as_str).unwrap_or("");
            format!("{cell:<width$}")
        })
        .collect::<Vec<_>>()
        .join(" | ");
    format!("  {cells}")
}

fn render_table_rule(widths: &[usize]) -> String {
    let cells = widths
        .iter()
        .map(|width| "-".repeat((*width).max(3)))
        .collect::<Vec<_>>()
        .join("-+-");
    format!("  {cells}")
}

fn render_plain_line(line: &str) -> String {
    render_inline(line.trim_end())
}

fn render_inline(line: &str) -> String {
    strip_paired_marker(&strip_paired_marker(&line.replace("\\_", "_"), "**"), "__")
}

fn strip_paired_marker(line: &str, marker: &str) -> String {
    let mut rendered = String::new();
    let mut rest = line;
    loop {
        let Some(open) = rest.find(marker) else {
            rendered.push_str(rest);
            break;
        };
        let after_open = &rest[open + marker.len()..];
        let Some(close) = after_open.find(marker) else {
            rendered.push_str(rest);
            break;
        };
        rendered.push_str(&rest[..open]);
        rendered.push_str(&after_open[..close]);
        rest = &after_open[close + marker.len()..];
    }
    rendered
}

#[cfg(test)]
mod tests;
