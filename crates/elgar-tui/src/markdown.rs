pub(crate) fn render_assistant_markdown(markdown: &str) -> String {
    let normalized = markdown.replace("\r\n", "\n").replace("<br>", "\n");
    let lines: Vec<&str> = normalized.lines().collect();
    let mut rendered = Vec::new();
    let mut index = 0;
    let mut in_code_block = false;

    while index < lines.len() {
        let line = lines[index];
        let trimmed = line.trim();

        if let Some(language) = trimmed.strip_prefix("```") {
            if in_code_block {
                in_code_block = false;
            } else {
                in_code_block = true;
                let label = language.trim();
                if label.is_empty() {
                    rendered.push("code:".to_string());
                } else {
                    rendered.push(format!("code ({label}):"));
                }
            }
            index += 1;
            continue;
        }

        if in_code_block {
            rendered.push(format!("    {line}"));
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

    rendered.join("\n")
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
    line.replace("**", "")
        .replace("__", "")
        .replace('`', "")
        .replace("\\_", "_")
}

#[cfg(test)]
mod tests {
    use super::render_assistant_markdown;

    #[test]
    fn renders_plain_text_without_changing_content() {
        assert_eq!(
            render_assistant_markdown("Plain assistant text.\nSecond line."),
            "Plain assistant text.\nSecond line."
        );
    }

    #[test]
    fn renders_code_blocks_without_fences() {
        let rendered = render_assistant_markdown("Use this:\n```rust\nfn main() {}\n```");

        assert_eq!(rendered, "Use this:\ncode (rust):\n    fn main() {}");
        assert!(!rendered.contains("```"));
    }

    #[test]
    fn renders_lists_with_clean_markers() {
        let rendered = render_assistant_markdown("- **one**\n  * two\n1. `three`");

        assert_eq!(rendered, "- one\n  - two\n1. three");
    }

    #[test]
    fn renders_tables_without_markdown_separator_rows() {
        let rendered = render_assistant_markdown("| File | State |\n| --- | --- |\n| a.rs | ok |");

        assert_eq!(rendered, "  File | State\n  -----+------\n  a.rs | ok   ");
        assert!(!rendered.contains("| --- |"));
    }

    #[test]
    fn leaves_table_like_text_without_separator_as_plain_text() {
        let rendered = render_assistant_markdown("| File | State |\n| a.rs | ok |");

        assert_eq!(rendered, "| File | State |\n| a.rs | ok |");
    }

    #[test]
    fn renders_preformatted_blocks_as_indented_text() {
        let rendered = render_assistant_markdown("tree:\n    src/\n      lib.rs");

        assert_eq!(rendered, "tree:\n    src/\n      lib.rs");
    }
}
