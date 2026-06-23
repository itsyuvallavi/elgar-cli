//! List and preformatted-line rendering for assistant markdown.

use super::inline::render_inline;

pub(super) fn render_list_line(line: &str) -> Option<String> {
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

pub(super) fn is_preformatted_line(line: &str) -> bool {
    line.starts_with("    ") || line.starts_with('\t')
}

pub(super) fn render_preformatted_line(line: &str) -> String {
    if let Some(rest) = line.strip_prefix('\t') {
        format!("    {rest}")
    } else {
        line.to_string()
    }
}
