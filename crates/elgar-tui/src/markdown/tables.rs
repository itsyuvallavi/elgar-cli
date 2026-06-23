//! Markdown table rendering.
//!
//! Tables are rendered as compact, readable plain text with separator rows
//! removed.

use super::inline::render_inline;

pub(super) fn is_table_start(lines: &[&str], index: usize) -> bool {
    index + 1 < lines.len()
        && is_table_row(lines[index])
        && (is_table_separator(lines[index + 1]) || is_loose_table_separator(lines[index + 1]))
}

pub(super) fn render_table(lines: &[&str], start: usize) -> (Vec<String>, usize) {
    if is_loose_table_separator(lines[start + 1]) {
        return render_loose_table(lines, start);
    }

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

fn is_loose_table_separator(line: &str) -> bool {
    let trimmed = line.trim();
    !trimmed.is_empty()
        && trimmed.contains('+')
        && trimmed
            .chars()
            .all(|character| matches!(character, '-' | '+' | '|' | ':' | ' '))
        && trimmed
            .chars()
            .filter(|character| *character == '-')
            .count()
            >= 3
}

fn table_cells(line: &str) -> Vec<String> {
    line.trim()
        .trim_matches('|')
        .split('|')
        .map(|cell| render_inline(cell.trim()))
        .collect()
}

fn render_loose_table(lines: &[&str], start: usize) -> (Vec<String>, usize) {
    let header = table_cells(lines[start]);
    let mut rows = Vec::new();
    let mut index = start + 2;

    while index < lines.len() {
        let line = lines[index];
        if line.trim().is_empty() {
            break;
        }

        if is_table_row(line) {
            rows.push(table_cells(line));
            index += 1;
            continue;
        }

        if append_loose_continuation(&mut rows, line) {
            index += 1;
            continue;
        }

        break;
    }

    let mut rendered = Vec::new();
    if let Some(title) = header.first().filter(|title| !title.is_empty()) {
        rendered.push(title.to_string());
    }

    for row in rows {
        rendered.push(render_loose_table_row(&row));
    }

    (rendered, index)
}

fn append_loose_continuation(rows: &mut [Vec<String>], line: &str) -> bool {
    let continuation = render_inline(line.trim());
    let Some(last_row) = rows.last_mut() else {
        return false;
    };
    let Some(last_cell) = last_row.last_mut() else {
        return false;
    };

    if !should_continue_loose_cell(last_cell, &continuation) {
        return false;
    }

    if !last_cell.ends_with(' ') {
        last_cell.push(' ');
    }
    last_cell.push_str(&continuation);
    true
}

fn should_continue_loose_cell(previous: &str, continuation: &str) -> bool {
    !continuation.is_empty()
        && (previous.ends_with(',')
            || previous.ends_with('(')
            || previous.matches('(').count() > previous.matches(')').count())
}

fn render_loose_table_row(row: &[String]) -> String {
    match row {
        [] => String::new(),
        [entry] => format!("  {entry}"),
        [entry, description, ..] if description.is_empty() => format!("  {entry}"),
        [entry, description, ..] => format!("  {entry} - {description}"),
    }
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
