//! Terminal box rendering for code blocks.

use super::{wrap::split_to_width, CODE_BOX_MAX_CONTENT_WIDTH, CODE_BOX_MIN_CONTENT_WIDTH};

pub(super) fn render_boxed_code_block(header: &str, body_lines: &[String]) -> Vec<String> {
    let content_width = code_box_content_width(header, body_lines);
    let header = super::wrap::truncate_to_width(header, content_width);
    let mut lines = vec![code_box_top_line(&header, content_width)];

    for line in body_lines {
        for segment in split_to_width(line, content_width) {
            lines.push(code_box_body_line(&segment, content_width));
        }
    }

    lines.push(code_box_bottom_line(content_width));
    lines
}

fn code_box_content_width(header: &str, body_lines: &[String]) -> usize {
    body_lines
        .iter()
        .map(|line| line.chars().count())
        .chain(std::iter::once(header.chars().count()))
        .max()
        .unwrap_or(0)
        .clamp(CODE_BOX_MIN_CONTENT_WIDTH, CODE_BOX_MAX_CONTENT_WIDTH)
}

fn code_box_top_line(header: &str, content_width: usize) -> String {
    let prefix = format!("─ {header} ");
    let rule_width = content_width + 3;
    let fill = "─".repeat(rule_width.saturating_sub(prefix.chars().count()));
    format!(" ╭{prefix}{fill}╮")
}

fn code_box_body_line(line: &str, content_width: usize) -> String {
    format!(
        " │ {}{} │",
        line,
        " ".repeat(content_width.saturating_sub(line.chars().count()))
    )
}

fn code_box_bottom_line(content_width: usize) -> String {
    format!(" ╰{}╯", "─".repeat(content_width + 3))
}
