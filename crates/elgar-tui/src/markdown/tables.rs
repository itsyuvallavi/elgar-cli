//! Markdown table rendering.
//!
//! Tables are rendered as aligned plain-text rows with the markdown separator
//! row removed.

use super::inline::render_inline;

pub(super) fn is_table_start(lines: &[&str], index: usize) -> bool {
    index + 1 < lines.len() && is_table_row(lines[index]) && is_table_separator(lines[index + 1])
}

pub(super) fn render_table(lines: &[&str], start: usize) -> (Vec<String>, usize) {
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
